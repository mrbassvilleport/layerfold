//! Trimming layers in the Timeline as in After Effects: the pointer over either end of a layer
//! bar (a few points inside it or just outside) shows the trim cursor, and dragging that end
//! moves the layer's In or Out point.
use effectcraft_engine::Session;
use effectcraft_engine::project::LayerId;
use effectcraft_ui_egui::EffectcraftApp;
use egui::{CursorIcon, Event, Modifiers, Pos2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

/// A 10 s comp at 30 fps with a solid trimmed to 2–6 s and a 20 s precomp trimmed to 3–7 s (its
/// source runs past both ends, so the slip zones sit right beside its bar's ends).
fn harness() -> (Harness<'static, EffectcraftApp>, u64, u64) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Inner", "width": 320, "height": 180, "frameRate": 30, "duration": 20})).unwrap();
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 10})).unwrap();
    let solid = s.execute("layer.newSolid", json!({"name": "Plate", "color": "#406080"})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.timing", json!({"layers": [solid], "in": 2.0, "out": 6.0})).unwrap();
    let pre = s.execute("layer.addItem", json!({"item": "Inner"})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.timing", json!({"layers": [pre], "in": 3.0, "out": 7.0})).unwrap();
    s.execute("layer.select", json!({"layers": []})).unwrap();
    let app = EffectcraftApp::new(s);
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| app);
    h.run_steps(4);
    (h, solid, pre)
}

fn bar(h: &Harness<'_, EffectcraftApp>, layer: u64) -> egui::Rect {
    let e = h.state().auto.find(&format!("timeline.layer.{layer}.bar")).expect("layer bar").clone();
    egui::Rect::from_min_size(pos2(e.rect[0], e.rect[1]), vec2(e.rect[2], e.rect[3]))
}

/// The cursor shown with the pointer resting at `p`.
fn cursor_at(h: &mut Harness<'_, EffectcraftApp>, p: Pos2) -> CursorIcon {
    h.event(Event::PointerMoved(p));
    h.run_steps(3);
    h.output().platform_output.cursor_icon
}

/// (in, out, start) of `layer` in seconds.
fn timing(h: &Harness<'_, EffectcraftApp>, layer: u64) -> (f64, f64, f64) {
    let comp = h.state().session.active_comp().unwrap();
    let l = comp.layer(LayerId(layer)).unwrap();
    (l.in_point.seconds(), l.out_point.seconds(), l.start_time.seconds())
}

fn drag(h: &mut Harness<'_, EffectcraftApp>, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    for k in 1..=10 {
        h.event(Event::PointerMoved(from + (to - from) * (k as f32 / 10.0)));
        h.step();
    }
    h.event(Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

/// Both ends of every bar show the trim cursor, just inside and just outside the end; the middle
/// doesn't. The same holds for a selected layer, a twirled-open layer and a short bar.
fn assert_trim_cursor_at_ends(h: &mut Harness<'_, EffectcraftApp>, layer: u64, case: &str) {
    let b = bar(h, layer);
    let y = b.center().y;
    for (what, x) in [("in, outside", b.min.x - 3.0), ("in, inside", b.min.x + 2.0), ("out, inside", b.max.x - 2.0), ("out, outside", b.max.x + 3.0)] {
        assert_eq!(cursor_at(h, pos2(x, y)), CursorIcon::ResizeHorizontal, "{case}: layer {layer} {what} (bar {b:?})");
    }
    if b.width() > 40.0 {
        assert_ne!(cursor_at(h, b.center()), CursorIcon::ResizeHorizontal, "{case}: the middle of the bar moves, it doesn't trim");
    }
}

#[test]
fn bar_ends_show_the_trim_cursor() {
    let (mut h, solid, pre) = harness();
    assert_trim_cursor_at_ends(&mut h, solid, "unselected solid");
    assert_trim_cursor_at_ends(&mut h, pre, "precomp with slip zones beside its ends");
    h.state_mut().session.execute("layer.select", json!({"layers": [solid]})).unwrap();
    h.run_steps(2);
    assert_trim_cursor_at_ends(&mut h, solid, "selected");
    h.state_mut().ui.timeline.open_layers.insert(solid);
    h.run_steps(2);
    assert_trim_cursor_at_ends(&mut h, solid, "twirled open");
    assert_trim_cursor_at_ends(&mut h, pre, "below a twirled-open layer");
    // Three frames long: still trims from both ends.
    h.state_mut().session.execute("layer.timing", json!({"layers": [solid], "in": 2.0, "out": 2.1})).unwrap();
    h.run_steps(2);
    assert_trim_cursor_at_ends(&mut h, solid, "short bar");
}

/// The trim zones show the horizontal resize cursor; the bar body doesn't.
#[test]
fn trim_zones_show_the_resize_cursor() {
    let (mut h, solid, _) = harness();
    let b = bar(&h, solid);
    assert_eq!(cursor_at(&mut h, pos2(b.max.x, b.center().y)), CursorIcon::ResizeHorizontal);
    assert_ne!(cursor_at(&mut h, b.center()), CursorIcon::ResizeHorizontal, "trim cursor leaked onto the bar body");
}

#[test]
fn dragging_bar_ends_trims_in_and_out() {
    let (mut h, solid, pre) = harness();
    let b = bar(&h, solid);
    let pps = b.width() as f64 / 4.0;
    let y = b.center().y;
    // Out end, grabbed just inside, one second to the left.
    drag(&mut h, pos2(b.max.x - 2.0, y), pos2(b.max.x - 2.0 - pps as f32, y));
    let (i, o, s) = timing(&h, solid);
    assert!((i - 2.0).abs() < 1e-6 && (o - 5.0).abs() < 1.5 / 30.0 && s.abs() < 1e-6, "out trimmed to ~5 s: in {i} out {o} start {s}");
    // In end, grabbed just inside, one second to the right.
    let b = bar(&h, solid);
    drag(&mut h, pos2(b.min.x + 2.0, y), pos2(b.min.x + 2.0 + pps as f32, y));
    let (i, o2, s) = timing(&h, solid);
    assert!((i - 3.0).abs() < 1.5 / 30.0 && (o2 - o).abs() < 1e-6 && s.abs() < 1e-6, "in trimmed to ~3 s: in {i} out {o2} start {s}");
    // The precomp's Out end grabbed just outside the bar (over its slip zone) extends it.
    let b = bar(&h, pre);
    let y = b.center().y;
    drag(&mut h, pos2(b.max.x + 3.0, y), pos2(b.max.x + 3.0 + pps as f32, y));
    let (i, o, s) = timing(&h, pre);
    assert!((i - 3.0).abs() < 1e-6 && (o - 8.0).abs() < 1.5 / 30.0 && s.abs() < 1e-6, "out extended to ~8 s, not slipped: in {i} out {o} start {s}");
}

/// Like [`drag`], with `modifiers` held once the button is down.
fn drag_with(h: &mut Harness<'_, EffectcraftApp>, from: Pos2, to: Pos2, modifiers: Modifiers) {
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::ModifiersChanged(modifiers));
    for k in 1..=10 {
        h.event(Event::PointerMoved(from + (to - from) * (k as f32 / 10.0)));
        h.step();
    }
    h.event(Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers });
    h.run_steps(3);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
}

/// Alt+drag on the bar moves the source under it: In, Out and keyframes stay, and it's one undo.
/// A plain drag still moves the layer.
#[test]
fn alt_drag_slips_the_source_and_keeps_keys_in_place() {
    let (mut h, _, pre) = harness();
    let s = &mut h.state_mut().session;
    s.execute("time.set", json!({"time": 5.0})).unwrap();
    s.execute("prop.addKey", json!({"layer": pre, "path": "transform/scale", "value": [50.0, 50.0]})).unwrap();
    h.run_steps(3);
    let key_at = |h: &Harness<'_, EffectcraftApp>| {
        let l = h.state().session.active_comp().unwrap().layer(LayerId(pre)).unwrap().clone();
        l.comp_time(l.props.sub("transform").unwrap().get("scale").unwrap().keys[0].time).seconds()
    };
    let b = bar(&h, pre);
    let pps = b.width() / 4.0;
    drag_with(&mut h, b.center(), b.center() + vec2(pps, 0.0), Modifiers::ALT);
    let (i, o, st) = timing(&h, pre);
    assert_eq!((i, o), (3.0, 7.0), "the bar stays");
    assert!((st - 1.0).abs() < 1.5 / 30.0, "the source moved ~1 s later: start {st}");
    assert!((key_at(&h) - 5.0).abs() < 1e-6, "the key stays at 5 s, found {}", key_at(&h));
    // Past the limit it stops where the source still covers the bar: start can't pass In.
    drag_with(&mut h, b.center(), b.center() + vec2(pps * 8.0, 0.0), Modifiers::ALT);
    assert_eq!(timing(&h, pre).2, 3.0, "stops with the source's first frame at In");
    assert!(h.state_mut().session.undo() && h.state_mut().session.undo());
    h.run_steps(2);
    assert_eq!(timing(&h, pre), (3.0, 7.0, 0.0), "each drag is one undo step");
    // A plain drag moves the whole layer.
    drag(&mut h, b.center(), b.center() + vec2(pps, 0.0));
    let (i, _, st) = timing(&h, pre);
    assert!((i - 4.0).abs() < 1.5 / 30.0 && (st - 1.0).abs() < 1.5 / 30.0, "moved: in {i} start {st}");
}

/// The Alt hint explains the slip before the drag and gets out of the way during it, when it would
/// cover the keys being lined up.
#[test]
fn the_slip_hint_shows_on_hover_and_not_during_the_drag() {
    const HINT: &str = "the bar and its keyframes stay";
    let (mut h, _, pre) = harness();
    let b = bar(&h, pre);
    let from = b.center();
    h.event(Event::ModifiersChanged(Modifiers::ALT));
    h.event(Event::PointerMoved(from));
    h.run_steps(12);
    assert!(h.query_all_by_label_contains(HINT).next().is_some(), "the hint shows while hovering with Alt");
    h.event(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::ALT });
    h.step();
    for k in 1..=10 {
        h.event(Event::PointerMoved(from + vec2(k as f32 * 10.0, 0.0)));
        h.step();
        h.step();
        // The first move stays within egui's click tolerance: not a drag yet.
        if k > 1 {
            assert!(h.query_all_by_label_contains(HINT).next().is_none(), "no hint during the drag, step {k}");
        }
    }
    h.event(Event::PointerButton { pos: from + vec2(100.0, 0.0), button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::ALT });
    h.run_steps(3);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
}

/// An edge dragged near the end of the source sticks to it without Shift; dragged further it
/// passes through.
#[test]
fn trim_edges_stick_to_the_source_ends() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Inner", "width": 320, "height": 180, "frameRate": 30, "duration": 5})).unwrap();
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 10})).unwrap();
    let pre = s.execute("layer.addItem", json!({"item": "Inner"})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.timing", json!({"layers": [pre], "start": 1.0})).unwrap();
    s.execute("layer.timing", json!({"layers": [pre], "in": 2.0, "out": 4.0})).unwrap();
    s.execute("layer.select", json!({"layers": []})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(4);
    let b = bar(&h, pre);
    let (pps, y) = (b.width() / 2.0, b.center().y);
    // Source is 1 s .. 6 s. Out dropped 5 points short of 6 s lands exactly on it.
    drag(&mut h, pos2(b.max.x - 2.0, y), pos2(b.max.x - 2.0 + pps * 2.0 - 5.0, y));
    assert_eq!(timing(&h, pre).1, 6.0, "out sticks to the source's end");
    // In, 5 points short of 1 s.
    drag(&mut h, pos2(b.min.x + 2.0, y), pos2(b.min.x + 2.0 - pps + 5.0, y));
    assert_eq!(timing(&h, pre).0, 1.0, "in sticks to the source's start");
    // Far past the end: the edge goes through, as it did before.
    let b = bar(&h, pre);
    drag(&mut h, pos2(b.max.x - 2.0, y), pos2(b.max.x - 2.0 + pps, y));
    let o = timing(&h, pre).1;
    assert!((o - 7.0).abs() < 1.5 / 30.0, "out passed the source's end: {o}");
}

/// With Frames and Frame Count Start at 1, In moves to 00061 but the Duration column still counts
/// the layer's 120 frames (#1807).
#[test]
fn duration_column_counts_frames_from_any_frame_count() {
    let (mut h, solid, _) = harness();
    effectcraft_ui_egui::panels::timeline::set_column(&mut h.state_mut().ui.timeline, "in", true).unwrap();
    effectcraft_ui_egui::panels::timeline::set_column(&mut h.state_mut().ui.timeline, "duration", true).unwrap();
    h.state_mut().session.execute("file.projectSettings", json!({"timeDisplay": "frames", "frameStart": 1})).unwrap();
    h.run_steps(3);
    let cell = |h: &Harness<'_, EffectcraftApp>, kind: &str| h.state().auto.find(&format!("timeline.layer.{solid}.{kind}")).expect("cell").label.clone();
    assert_eq!(cell(&h, "in"), "00061");
    assert_eq!(cell(&h, "duration"), "00120");
}

/// A source with a frame for every time, so the bar has tiles to show.
struct Frames;

impl effectcraft_engine::render::FootageSource for Frames {
    fn frame(
        &self,
        _: effectcraft_engine::project::ItemId,
        _: &effectcraft_engine::project::Footage,
        _: effectcraft_time::Tick,
    ) -> Option<std::sync::Arc<effectcraft_render::Image>> {
        Some(std::sync::Arc::new(effectcraft_render::Image::new(16, 9)))
    }
}

/// A press and release at `p`.
fn click(h: &mut Harness<'_, EffectcraftApp>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    h.step();
    h.event(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

fn center_of(h: &Harness<'_, EffectcraftApp>, id: &str) -> Pos2 {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("no {id}")).clone();
    pos2(e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0)
}

/// A footage layer shows its video's frames on the bar, loaded in the background, and the button
/// above the layers turns them off and back on.
#[test]
fn footage_bars_show_frames_and_can_turn_them_off() {
    use effectcraft_engine::project::{Footage, FootageKind, ItemKind};
    let mut s = Session { footage: std::sync::Arc::new(Frames), ..Default::default() };
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 10})).unwrap();
    let footage = Footage {
        kind: FootageKind::Video,
        path: "clip.mp4".into(),
        width: 64,
        height: 36,
        has_video: true,
        duration: effectcraft_time::Tick::from_seconds_f64(4.0),
        ..Default::default()
    };
    let clip = std::sync::Arc::make_mut(&mut s.project).add_item("Clip", Default::default(), None, ItemKind::Footage(footage));
    let layer = s.execute("layer.addItem", json!({"item": clip.0})).unwrap()["layer"].as_u64().unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    let id = format!("timeline.layer.{layer}.frames");
    let shown = |h: &Harness<'_, EffectcraftApp>| h.state().auto.find(&id).map(|e| e.label.clone());
    let start = std::time::Instant::now();
    while shown(&h).is_none_or(|label| label.starts_with("0 ")) {
        assert!(start.elapsed().as_secs() < 30, "no frames on the bar: {:?}", shown(&h));
        h.run_steps(2);
    }
    assert!(h.state_mut().ui.timeline.layer_frames);
    let button = center_of(&h, "timeline.switch.layerFrames");
    click(&mut h, button);
    assert!(!h.state_mut().ui.timeline.layer_frames);
    assert_eq!(shown(&h), None);
    click(&mut h, button);
    assert!(h.state_mut().ui.timeline.layer_frames);
    assert!(shown(&h).is_some());
}

/// Records which files get decoded and which of those reads were the bars' tiles (the viewer reads
/// too); a read takes `delay`.
#[derive(Default)]
struct Recording(std::sync::Mutex<Vec<String>>, std::sync::Mutex<Vec<String>>, std::time::Duration);

impl Recording {
    fn tiles(&self) -> usize {
        self.1.lock().unwrap().len()
    }
}

fn is_tile_thread() -> bool {
    std::thread::current().name() == Some("ec-filmstrip")
}

impl effectcraft_engine::render::FootageSource for Recording {
    fn frame(
        &self,
        _: effectcraft_engine::project::ItemId,
        f: &effectcraft_engine::project::Footage,
        _: effectcraft_time::Tick,
    ) -> Option<std::sync::Arc<effectcraft_render::Image>> {
        self.0.lock().unwrap().push(f.path.clone());
        if is_tile_thread() {
            self.1.lock().unwrap().push(f.path.clone());
            std::thread::sleep(self.2);
        }
        Some(std::sync::Arc::new(effectcraft_render::Image::new(16, 9)))
    }
}

fn video(path: &str) -> effectcraft_engine::project::Footage {
    effectcraft_engine::project::Footage {
        kind: effectcraft_engine::project::FootageKind::Video,
        path: path.into(),
        width: 64,
        height: 36,
        has_video: true,
        duration: effectcraft_time::Tick::from_seconds_f64(4.0),
        ..Default::default()
    }
}

/// Waits for the bar to show frames.
fn wait_for_frames(h: &mut Harness<'static, EffectcraftApp>, layer: u64) {
    let id = format!("timeline.layer.{layer}.frames");
    let start = std::time::Instant::now();
    while h.state().auto.find(&id).is_none_or(|e| e.label.starts_with("0 ")) {
        assert!(start.elapsed().as_secs() < 30, "no frames on the bar");
        h.run_steps(2);
    }
}

/// A purge (what Reload Footage and a changed file send) brings the frames back from the file.
#[test]
fn purged_caches_decode_the_frames_again() {
    let source = std::sync::Arc::new(Recording::default());
    let mut s = Session { footage: source.clone(), ..Default::default() };
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 10})).unwrap();
    let clip =
        std::sync::Arc::make_mut(&mut s.project).add_item("Clip", Default::default(), None, effectcraft_engine::project::ItemKind::Footage(video("clip.mp4")));
    let layer = s.execute("layer.addItem", json!({"item": clip.0})).unwrap()["layer"].as_u64().unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    wait_for_frames(&mut h, layer);
    // Until every tile is in: those still on their way would look like decoding again.
    let mut before = source.tiles();
    let mut quiet = std::time::Instant::now();
    while quiet.elapsed().as_millis() < 500 {
        h.run_steps(2);
        if source.tiles() != before {
            before = source.tiles();
            quiet = std::time::Instant::now();
        }
    }
    assert!(before > 0);
    h.state_mut().session.execute("edit.purge", json!({"what": "all"})).unwrap();
    h.run_steps(2);
    let start = std::time::Instant::now();
    while source.tiles() <= before {
        assert!(start.elapsed().as_secs() < 30, "the frames were not decoded again");
        h.run_steps(2);
    }
}

/// A Timeline no longer drawn (another panel maximized, Full Screen preview) takes its queue along:
/// at most the tile in hand finishes.
#[test]
fn a_hidden_timeline_stops_decoding_frames() {
    let source = std::sync::Arc::new(Recording(Default::default(), Default::default(), std::time::Duration::from_millis(100)));
    let mut s = Session { footage: source.clone(), ..Default::default() };
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 10})).unwrap();
    let clip =
        std::sync::Arc::make_mut(&mut s.project).add_item("Clip", Default::default(), None, effectcraft_engine::project::ItemKind::Footage(video("clip.mp4")));
    let layer = s.execute("layer.addItem", json!({"item": clip.0})).unwrap()["layer"].as_u64().unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    let start = std::time::Instant::now();
    while source.tiles() == 0 {
        assert!(start.elapsed().as_secs() < 30, "no tile was asked for");
        h.step();
    }
    h.state_mut().ui.maximized = Some(effectcraft_ui_egui::dock::PanelKind::Project);
    h.run_steps(2);
    assert!(h.state().auto.find(&format!("timeline.layer.{layer}.bar")).is_none(), "the Timeline is hidden");
    let at_hide = source.tiles();
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < 500 {
        h.step();
    }
    assert!(source.tiles() <= at_hide + 1, "kept decoding: {at_hide} then {}", source.tiles());
}

/// A decoder that panics costs that tile; the others still come.
#[test]
fn a_panicking_decoder_leaves_the_other_frames() {
    #[derive(Default)]
    struct PanicsOnce(std::sync::atomic::AtomicBool);
    impl effectcraft_engine::render::FootageSource for PanicsOnce {
        fn frame(
            &self,
            _: effectcraft_engine::project::ItemId,
            _: &effectcraft_engine::project::Footage,
            _: effectcraft_time::Tick,
        ) -> Option<std::sync::Arc<effectcraft_render::Image>> {
            if is_tile_thread() && !self.0.swap(true, std::sync::atomic::Ordering::Relaxed) {
                panic!("expected: a decoder that panics once");
            }
            Some(std::sync::Arc::new(effectcraft_render::Image::new(16, 9)))
        }
    }
    let mut s = Session { footage: std::sync::Arc::new(PanicsOnce::default()), ..Default::default() };
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 10})).unwrap();
    let clip =
        std::sync::Arc::make_mut(&mut s.project).add_item("Clip", Default::default(), None, effectcraft_engine::project::ItemKind::Footage(video("clip.mp4")));
    let layer = s.execute("layer.addItem", json!({"item": clip.0})).unwrap()["layer"].as_u64().unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    wait_for_frames(&mut h, layer);
}

/// A layer's Frames switch turns off that layer alone, nothing of it is decoded, and it's one undo
/// step like any switch. Projects saved before the switch existed show frames.
#[test]
fn a_layers_switch_turns_its_frames_off() {
    let source = std::sync::Arc::new(Recording::default());
    let mut s = Session { footage: source.clone(), ..Default::default() };
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 10})).unwrap();
    let p = std::sync::Arc::make_mut(&mut s.project);
    let a = p.add_item("A", Default::default(), None, effectcraft_engine::project::ItemKind::Footage(video("a.mp4")));
    let b = p.add_item("B", Default::default(), None, effectcraft_engine::project::ItemKind::Footage(video("b.mp4")));
    let off = s.execute("layer.addItem", json!({"item": a.0})).unwrap()["layer"].as_u64().unwrap();
    let on = s.execute("layer.addItem", json!({"item": b.0})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.setSwitch", json!({"layers": [off], "switch": "frames", "value": false})).unwrap();
    s.execute("layer.select", json!({"layers": []})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    wait_for_frames(&mut h, on);
    let frames = |h: &Harness<'_, EffectcraftApp>, l: u64| h.state().auto.find(&format!("timeline.layer.{l}.frames")).is_some();
    assert!(!frames(&h, off));
    assert!(!source.1.lock().unwrap().iter().any(|p| p == "a.mp4"), "nothing of the layer turned off is decoded");
    let switch = center_of(&h, &format!("timeline.layer.{on}.switch.frames"));
    click(&mut h, switch);
    assert!(!frames(&h, on), "the row's switch turns its layer off");
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert!(frames(&h, on));

    let mut old = serde_json::to_value(effectcraft_engine::project::Switches::default()).unwrap();
    old.as_object_mut().unwrap().remove("frames");
    assert!(serde_json::from_value::<effectcraft_engine::project::Switches>(old).unwrap().frames);
}

/// With Shift the nearest target wins, the source's end included.
#[test]
fn shift_trims_to_the_nearest_target() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 10})).unwrap();
    let clip =
        std::sync::Arc::make_mut(&mut s.project).add_item("Clip", Default::default(), None, effectcraft_engine::project::ItemKind::Footage(video("clip.mp4")));
    let plate = s.execute("layer.newSolid", json!({"name": "Plate", "color": "#406080"})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.timing", json!({"layers": [plate], "in": 0.0, "out": 119.0 / 30.0})).unwrap();
    let layer = s.execute("layer.addItem", json!({"item": clip.0})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.timing", json!({"layers": [layer], "in": 0.0, "out": 2.0})).unwrap();
    s.execute("layer.select", json!({"layers": []})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(4);
    let b = bar(&h, layer);
    let pps = b.width() / 2.0;
    // The source ends at 4 s, the other layer one frame earlier; the pointer goes nearer the layer.
    let from = pos2(b.max.x - 1.0, b.center().y);
    let to = from + vec2(pps * 1.96, 0.0);
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    for k in 1..=10 {
        h.event(Event::PointerMoved(from + (to - from) * (k as f32 / 10.0)));
        h.step();
    }
    let (_, out, _) = timing(&h, layer);
    h.event(Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::SHIFT });
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(3);
    assert!((out - 119.0 / 30.0).abs() < 1e-6, "out {out}");
}

/// A slip that used up the zone it started in lost it under the pointer and never saw its stop: the
/// next one is its own undo step all the same.
#[test]
fn a_slip_to_the_limit_ends_its_undo_step() {
    let (mut h, _, pre) = harness();
    let b = bar(&h, pre);
    let pps = b.width() / 4.0;
    let left = center_of(&h, &format!("timeline.layer.{pre}.sourcel"));
    drag(&mut h, left, left + vec2(pps * 3.5, 0.0));
    let (_, _, limit) = timing(&h, pre);
    assert!((limit - 3.0).abs() < 1.5 / 30.0, "slipped to the limit: {limit}");
    assert!(h.state().auto.find(&format!("timeline.layer.{pre}.sourcel")).is_none(), "the zone is gone");
    // Just past the bar: the zone runs on far beyond the panel.
    let right = pos2(bar(&h, pre).max.x + 20.0, bar(&h, pre).center().y);
    drag(&mut h, right, right - vec2(pps, 0.0));
    assert!((timing(&h, pre).2 - limit + 1.0).abs() < 1.5 / 30.0, "slipped back: {:?}", timing(&h, pre));
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    h.run_steps(2);
    assert!((timing(&h, pre).2 - limit).abs() < 1e-6, "undo takes back the second slip alone: {:?}", timing(&h, pre));
}

/// With a proxy on, as in the render, the bar's frames come from the proxy, even with the original gone.
#[test]
fn frames_come_from_an_enabled_proxy() {
    use effectcraft_engine::project::{ItemKind, Proxy};
    let source = std::sync::Arc::new(Recording::default());
    let mut s = Session { footage: source.clone(), ..Default::default() };
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 10})).unwrap();
    let mut original = video("original.mp4");
    original.missing = true;
    let p = std::sync::Arc::make_mut(&mut s.project);
    let clip = p.add_item("Clip", Default::default(), None, ItemKind::Footage(original));
    p.items.get_mut(&clip).unwrap().proxy = Some(Box::new(Proxy { footage: video("proxy.mp4"), enabled: true }));
    let layer = s.execute("layer.addItem", json!({"item": clip.0})).unwrap()["layer"].as_u64().unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    wait_for_frames(&mut h, layer);
    let seen = source.0.lock().unwrap();
    assert!(seen.iter().any(|p| p == "proxy.mp4") && !seen.iter().any(|p| p == "original.mp4"), "{seen:?}");
}

/// An Alt-drag released with its row scrolled away never ends on the bar: the next plain drag of
/// that layer moves it all the same.
#[test]
fn a_slip_released_offscreen_does_not_stick() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Inner", "width": 320, "height": 180, "frameRate": 30, "duration": 20})).unwrap();
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 10})).unwrap();
    for _ in 0..80 {
        s.execute("layer.newSolid", json!({"name": "Plate", "color": "#406080"})).unwrap();
    }
    let pre = s.execute("layer.addItem", json!({"item": "Inner"})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.timing", json!({"layers": [pre], "in": 3.0, "out": 7.0})).unwrap();
    s.execute("layer.select", json!({"layers": []})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(4);
    let b = bar(&h, pre);
    let pps = b.width() / 4.0;
    // Press with Alt, drag, scroll the row away, release there.
    h.event(Event::PointerMoved(b.center()));
    h.step();
    h.event(Event::PointerButton { pos: b.center(), button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::ModifiersChanged(Modifiers::ALT));
    for k in 1..=10 {
        h.event(Event::PointerMoved(b.center() + vec2(pps * k as f32 / 40.0, 0.0)));
        h.step();
    }
    assert!(timing(&h, pre).2 != 0.0, "slipping");
    h.state_mut().ui.timeline.scroll_y = 1200.0;
    h.run_steps(2);
    h.event(Event::PointerButton { pos: b.center(), button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::ALT });
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(3);
    h.state_mut().ui.timeline.scroll_y = 0.0;
    h.run_steps(3);
    let (in_before, _, start_before) = timing(&h, pre);
    let b = bar(&h, pre);
    drag(&mut h, b.center(), b.center() + vec2(pps, 0.0));
    let (i, _, st) = timing(&h, pre);
    assert!((i - in_before - 1.0).abs() < 1.5 / 30.0 && (st - start_before - 1.0).abs() < 1.5 / 30.0, "moved: in {i} start {st}");
}

/// A layer can be shorter than one frame: made at 60 fps, its comp then changed to 24. Trimming its
/// edge left the allowed range empty, which used to panic and close the app.
#[test]
fn trimming_a_layer_shorter_than_a_frame_does_not_crash() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 60, "duration": 10})).unwrap();
    let layer = s.execute("layer.newSolid", json!({"name": "Plate", "color": "#406080"})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.timing", json!({"layers": [layer], "in": 0.0, "out": 1.0 / 60.0})).unwrap();
    s.execute("comp.settings", json!({"frameRate": 24})).unwrap();
    let app = EffectcraftApp::new(s);
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| app);
    h.run_steps(4);
    let (_, out, _) = timing(&h, layer);
    assert!(out < 1.0 / 24.0, "the layer stays shorter than a frame: {out}");
    let b = bar(&h, layer);
    let y = b.center().y;
    drag(&mut h, pos2(b.min.x, y), pos2(b.min.x + 60.0, y));
    let b = bar(&h, layer);
    drag(&mut h, pos2(b.max.x, y), pos2(b.max.x - 60.0, y));
    let (in_point, out, _) = timing(&h, layer);
    assert!(in_point >= 0.0 && out >= in_point, "{in_point} {out}");
}
