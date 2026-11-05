//! Video frames on footage layer bars in the Timeline, as in Premiere.
//!
//! Tiles are laid from the source's first frame, so they ride along with a slip and a move. One
//! background thread decodes them, newest request first; a request nobody repeats (scrolled away,
//! zoomed past, the Timeline hidden) is dropped. Nothing is requested during playback: the decoder
//! belongs to the preview then. The Timeline's button turns the frames off for all layers, each
//! layer's Frames switch for that layer: what isn't drawn isn't decoded.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use effectcraft_engine::project::{Footage, FootageKind, ItemId, ItemKind, Layer, LayerSource, Project};
use effectcraft_engine::render::{FootageSource, cineon_frame};
use effectcraft_engine::time::Tick;
use egui::{Color32, Painter, Rect, TextureHandle, pos2};

use super::timeline::TMap;
use crate::EffectcraftApp;

/// Long side of a decoded tile: enough for a tall row on a high-density screen.
const TILE_SIDE: u32 = 96;
/// Source times are rounded to this many tiles per second, so zooming a little reuses tiles.
const STEPS_PER_SEC: f64 = 8.0;
/// Textures kept (at most about 37 KB each).
const MAX_TEXTURES: usize = 600;
/// Tiles uploaded per frame, so a burst of finished decodes can't stall one.
const MAX_UPLOADS: usize = 16;
/// Narrowest tile: tall footage shows its middle at this width instead of an unreadable sliver.
const MIN_TILE_WIDTH: f32 = 28.0;
const MAX_TILE_WIDTH: f32 = 240.0;
/// Seconds without new tiles after the viewer last waited for a frame. A seek in long-GOP footage
/// costs a second and shares the machine and the decoder, so scrubbing goes first (measured: tiles
/// made viewer frames take 2.5 times as long); the bars catch up when it settles.
const VIEWER_HOLD: f64 = 1.0;

/// A file's identity, a tile's position in it.
type Key = (u64, i64);
type Decoded = Option<(u32, u32, Vec<u8>)>;

struct Job {
    key: Key,
    item: ItemId,
    footage: Arc<Footage>,
    time: Tick,
    source: Arc<dyn FootageSource>,
    ctx: egui::Context,
}

#[derive(Default)]
struct Queue {
    wanted: Vec<Job>,
    busy: Option<Key>,
    done: Vec<(Key, Decoded)>,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    stop: AtomicBool,
}

/// Decoded tiles and the thread that decodes them.
#[derive(Default)]
pub struct Cache {
    shared: Arc<Shared>,
    started: bool,
    /// `None`: the file has no frame there, so it isn't asked again.
    textures: HashMap<Key, (Option<TextureHandle>, u64)>,
    pass: u64,
    /// This frame's requests, filled while the rows are drawn.
    wanted: Vec<Job>,
    queued: HashSet<Key>,
    /// One copy per file: a sequence's description holds every file name.
    footages: HashMap<u64, Arc<Footage>>,
    /// Part of every key, so a decode still running when the files changed can't come back as new.
    generation: u64,
    /// When the viewer last waited for a frame.
    viewer_busy_at: Option<f64>,
}

impl Drop for Cache {
    fn drop(&mut self) {
        // Under the lock, so the thread can't be between checking the flag and waiting.
        let _queue = self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.wake.notify_all();
    }
}

impl Cache {
    /// Forgets every tile: footage on disk changed, or the caches were purged.
    pub fn clear(&mut self) {
        self.generation += 1;
        self.textures.clear();
        self.footages.clear();
        let mut q = self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
        q.wanted.clear();
        q.done.clear();
    }

    /// Whether new tiles wait for the viewer, which is `busy` with a frame at `now` (seconds).
    fn viewer_hold(&mut self, now: f64, busy: bool) -> bool {
        if busy {
            self.viewer_busy_at = Some(now);
        }
        self.viewer_busy_at.is_some_and(|at| now - at < VIEWER_HOLD)
    }

    /// Hands this frame's requests to the thread and takes in what it finished. Called once per
    /// frame after the panels, also when none asked for anything: that's what drops the queue of a
    /// Timeline no longer drawn.
    pub fn end_frame(&mut self, ctx: &egui::Context) {
        self.pass += 1;
        let wanted = std::mem::take(&mut self.wanted);
        self.queued.clear();
        let (done, work_left) = {
            let mut q = self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
            let take = q.done.len().min(MAX_UPLOADS);
            let done: Vec<_> = q.done.drain(..take).collect();
            // Skip what's being decoded or already waits to be uploaded.
            let taken: HashSet<Key> = done.iter().chain(q.done.iter()).map(|(key, _)| *key).chain(q.busy).collect();
            q.wanted = wanted.into_iter().filter(|j| !taken.contains(&j.key)).collect();
            // Only what's queued needs a description; a sequence's holds every file name.
            self.footages.retain(|_, f| Arc::strong_count(f) > 1);
            if !q.done.is_empty() {
                ctx.request_repaint();
            }
            self.shared.wake.notify_one();
            (done, !q.wanted.is_empty())
        };
        if work_left {
            self.start();
        }
        for (key, tile) in done {
            let texture = tile.map(|(w, h, px)| {
                let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
                ctx.load_texture(format!("filmstrip-{}-{}", key.0, key.1), image, egui::TextureOptions::LINEAR)
            });
            self.textures.insert(key, (texture, self.pass));
        }
        if self.textures.len() > MAX_TEXTURES {
            let mut ages: Vec<u64> = self.textures.values().map(|(_, used)| *used).collect();
            ages.sort_unstable();
            let cut = ages.get(self.textures.len() - MAX_TEXTURES).copied().unwrap_or(0);
            self.textures.retain(|_, (_, used)| *used >= cut);
        }
    }

    /// Starts the decoding thread on first use. It can't start on the web, where the bars just
    /// stay plain.
    fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let shared = self.shared.clone();
        let spawned = std::thread::Builder::new().name("ec-filmstrip".into()).spawn(move || work(&shared));
        if let Err(e) = spawned {
            log::warn!("filmstrip: {e}");
        }
    }

    /// Takes the tile for `key`: its texture when decoded, `None` when the request goes out.
    fn tile(&mut self, key: Key) -> Option<&Option<TextureHandle>> {
        let pass = self.pass;
        self.textures.get_mut(&key).map(|e| {
            e.1 = pass;
            &e.0
        })
    }
}

fn work(shared: &Shared) {
    loop {
        let job = {
            let mut q = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
            q.busy = None;
            let job = loop {
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                if !q.wanted.is_empty() {
                    break q.wanted.remove(0);
                }
                q = shared.wake.wait(q).unwrap_or_else(PoisonError::into_inner);
            };
            q.busy = Some(job.key);
            job
        };
        // A decoder that panics costs this tile, not every tile until the app restarts.
        let tile = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode(&job))).unwrap_or_else(|_| {
            log::warn!("filmstrip: decoding {} panicked", job.footage.path);
            None
        });
        shared.queue.lock().unwrap_or_else(PoisonError::into_inner).done.push((job.key, tile));
        job.ctx.request_repaint();
    }
}

/// The tile's pixels, point-sampled down to at most [`TILE_SIDE`] on the long side.
fn decode(job: &Job) -> Decoded {
    let f = &job.footage;
    let (w, h) = (f.width.max(1), f.height.max(1));
    let scale = (TILE_SIDE as f32 / w.max(h) as f32).min(1.0);
    let size = (((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1));
    let mut frame = None;
    job.source.background(&mut || {
        frame = if scale <= 0.5 { job.source.frame_at_size(job.item, f, job.time, size.0, size.1) } else { job.source.frame(job.item, f, job.time) };
    });
    let frame = frame?;
    let image = cineon_frame(frame, f);
    let step = (image.width.max(image.height) / TILE_SIDE).max(1);
    let (out_w, out_h) = (image.width.div_ceil(step), image.height.div_ceil(step));
    let full = image.to_rgba8();
    let mut px = Vec::with_capacity((out_w as usize) * (out_h as usize) * 4);
    for y in 0..out_h {
        for x in 0..out_w {
            let at = ((y * step) as usize * image.width as usize + (x * step) as usize) * 4;
            px.extend_from_slice(full.get(at..at + 4)?);
        }
    }
    Some((out_w, out_h, px))
}

/// The tiles covering `visible`, in a grid of `width` starting at `origin`: each tile's left edge.
pub(crate) fn tile_edges(origin: f32, width: f32, visible: (f32, f32)) -> impl Iterator<Item = f32> {
    let first = if width > 0.0 { ((visible.0 - origin) / width).floor() as i64 } else { 0 };
    let last = if width > 0.0 { ((visible.1 - origin) / width).ceil() as i64 } else { 0 };
    (first..last).map(move |i| origin + i as f32 * width)
}

/// The rounding-grid position of source time `seconds`, inside the source.
pub(crate) fn tile_step(seconds: f64, duration: Tick) -> i64 {
    let last = (duration.seconds() * STEPS_PER_SEC).floor() as i64;
    ((seconds * STEPS_PER_SEC).round() as i64).clamp(0, (last - 1).max(0))
}

/// The footage a layer's frames come from, when the layer plainly shows it.
pub(crate) fn footage_of<'a>(project: &'a Project, layer: &Layer) -> Option<(ItemId, &'a Footage)> {
    let LayerSource::Footage { item } = &layer.source else { return None };
    if layer.props.get("timeRemap").is_some() {
        return None;
    }
    let it = project.item(*item)?;
    // As the render: an enabled proxy stands in for the original, missing or not, and a missing
    // proxy shows nothing there either.
    if let Some(px) = it.proxy.as_deref().filter(|px| px.enabled && px.footage.has_video) {
        return (!px.footage.missing).then_some((*item, &px.footage));
    }
    match &it.kind {
        ItemKind::Footage(f) if f.has_video && !f.missing => Some((*item, f)),
        _ => None,
    }
}

/// What in `f` changes its frames: the file and how it's interpreted. Not the sequence's names,
/// which are many and are covered by the path and the duration.
fn file_key(f: &Footage, generation: u64) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (generation, &f.path, f.width, f.height, f.duration.0, f.page, f.invert_alpha, f.linear_light, f.preserve_rgb).hash(&mut hasher);
    format!("{:?} {:?} {:?} {:?} {:?}", f.alpha, f.premul_color, f.color_profile, f.cineon, f.layer).hash(&mut hasher);
    format!("{:?} {:?} {:?}", f.loop_count, f.frame_rate, f.native_rate).hash(&mut hasher);
    hasher.finish()
}

/// The part of a frame of `image_aspect` that fills a tile of `tile_aspect`, centred, in texture
/// coordinates.
fn tile_uv(image_aspect: f32, tile_aspect: f32) -> Rect {
    let (w, h) = if image_aspect > tile_aspect { (tile_aspect / image_aspect, 1.0) } else { (1.0, image_aspect / tile_aspect) };
    Rect::from_center_size(pos2(0.5, 0.5), egui::vec2(w, h))
}

/// Draws the frames over `area` (the part of the bar the source covers), with `source_x` where the
/// source's span starts. Returns how many tiles are on screen, `None` when the layer shows no
/// footage frames.
pub(crate) fn paint(app: &mut EffectcraftApp, ctx: &egui::Context, painter: &Painter, tm: &TMap, layer: &Layer, source_x: f32, area: Rect) -> Option<usize> {
    let (item, footage) = footage_of(&app.session.project, layer)?;
    let inner = area.shrink2(egui::vec2(0.0, 1.0));
    let visible = inner.intersect(painter.clip_rect());
    if !visible.is_positive() {
        return Some(0);
    }
    let aspect = footage.width.max(1) as f32 / footage.height.max(1) as f32;
    let width = (inner.height() * aspect).clamp(MIN_TILE_WIDTH, MAX_TILE_WIDTH);
    let uv = tile_uv(aspect, width / inner.height());
    let file = file_key(footage, app.filmstrips.generation);
    let duration = if footage.kind == FootageKind::Still { Tick::ZERO } else { footage.looped_duration() };
    let playing = app.playback.playing;
    let viewer_busy = app.frames.urgent_pending();
    let hold = app.filmstrips.viewer_hold(ctx.input(|i| i.time), viewer_busy);
    let clip = painter.with_clip_rect(visible);
    let mut shown = 0;
    for left in tile_edges(source_x, width, (visible.min.x, visible.max.x)) {
        let seconds = layer.layer_time(Tick::from_seconds_f64(tm.t(left + width / 2.0))).seconds();
        let key = (file, tile_step(seconds, duration));
        let rect = Rect::from_min_max(pos2(left, inner.min.y), pos2(left + width, inner.max.y));
        match app.filmstrips.tile(key) {
            Some(Some(texture)) => {
                clip.image(texture.id(), rect, uv, Color32::WHITE);
                shown += 1;
            }
            Some(None) => {}
            None if playing => {}
            None if hold => ctx.request_repaint_after(std::time::Duration::from_secs_f64(VIEWER_HOLD)),
            None => {
                if app.filmstrips.queued.insert(key) {
                    let time = Tick::from_seconds_f64(key.1 as f64 / STEPS_PER_SEC);
                    let source = app.session.footage.clone();
                    let footage = app.filmstrips.footages.entry(file).or_insert_with(|| Arc::new(footage.clone())).clone();
                    app.filmstrips.wanted.push(Job { key, item, footage, time, source, ctx: ctx.clone() });
                }
            }
        }
    }
    Some(shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_cover_the_visible_part_from_the_grid() {
        let edges: Vec<f32> = tile_edges(100.0, 30.0, (145.0, 200.0)).collect();
        assert_eq!(edges, [130.0, 160.0, 190.0]);
        // A source starting right of the panel's left edge, and a degenerate width.
        assert_eq!(tile_edges(0.0, 10.0, (0.0, 20.0)).collect::<Vec<_>>(), [0.0, 10.0]);
        assert_eq!(tile_edges(0.0, 0.0, (0.0, 20.0)).count(), 0);
    }

    /// Vertical video in a wide tile shows its middle band undistorted; a frame that fits shows whole.
    #[test]
    fn a_tile_fills_with_the_middle_of_the_frame() {
        let uv = tile_uv(9.0 / 16.0, 28.0 / 17.0);
        assert!((uv.width() - 1.0).abs() < 1e-6 && (uv.center().y - 0.5).abs() < 1e-6);
        assert!((uv.height() - (9.0 / 16.0) / (28.0 / 17.0)).abs() < 1e-6);
        assert_eq!(tile_uv(1.5, 1.5), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
        // A panorama capped by the widest tile shows its middle part.
        let uv = tile_uv(20.0, 10.0);
        assert!((uv.width() - 0.5).abs() < 1e-6 && (uv.height() - 1.0).abs() < 1e-6);
    }

    /// Looping decides which frame a time shows, so a changed loop count is new tiles.
    #[test]
    fn a_changed_interpretation_is_new_tiles() {
        let base = Footage::default();
        let looped = Footage { loop_count: base.loop_count + 1, ..Footage::default() };
        assert_ne!(file_key(&base, 0), file_key(&looped, 0));
        assert_eq!(file_key(&base, 0), file_key(&Footage::default(), 0));
    }

    #[test]
    fn new_tiles_wait_for_the_viewer_to_settle() {
        let mut cache = Cache::default();
        assert!(!cache.viewer_hold(10.0, false));
        assert!(cache.viewer_hold(11.0, true));
        assert!(cache.viewer_hold(11.5, false));
        assert!(!cache.viewer_hold(11.0 + VIEWER_HOLD, false));
    }

    #[test]
    fn a_tile_stays_inside_the_source() {
        let duration = Tick::from_seconds_f64(2.0);
        assert_eq!(tile_step(1.0, duration), 8);
        assert_eq!(tile_step(-5.0, duration), 0);
        assert_eq!(tile_step(99.0, duration), 15, "last tile before the source's end");
        // A still has no length: always its one frame.
        assert_eq!(tile_step(1.5, Tick::ZERO), 0);
    }
}
