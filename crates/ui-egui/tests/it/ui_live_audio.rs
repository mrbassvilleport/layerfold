//! Audio switches update a running preview without reopening the device or resetting its clock.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use effectcraft_engine::Session;
use effectcraft_ui_egui::EffectcraftApp;
use effectcraft_ui_egui::audio::{AudioDevice, AudioFeed};
use egui::{Event, Modifiers, pos2};
use egui_kittest::Harness;
use serde_json::json;

#[derive(Clone, Default)]
struct Output(Arc<Mutex<(usize, usize)>>);

impl AudioDevice for Output {
    fn sample_rate(&self) -> u32 {
        48000
    }
    fn start(&mut self, _: Arc<AudioFeed>) -> Result<(), String> {
        self.0.lock().unwrap().0 += 1;
        Ok(())
    }
    fn stop(&mut self) {
        self.0.lock().unwrap().1 += 1;
    }
}

fn scene(muted: bool) -> (Harness<'static, EffectcraftApp>, Output, u64, u64, u64) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name":"Tone Source","width":64,"height":36,"duration":10,"frameRate":30})).unwrap();
    let source = s.active_comp_id().unwrap();
    let layer = s.execute("layer.newSolid", json!({"name":"Tone","color":"#406080"})).unwrap()["layer"].as_u64().unwrap();
    s.execute("effect.apply", json!({"layers":[layer],"effect":"ec.audio.tone"})).unwrap();
    // A precomp has the same Audio Levels group and speaker button as imported sound.
    s.execute("comp.new", json!({"name":"Preview","width":64,"height":36,"duration":10,"frameRate":30})).unwrap();
    let layer = s.execute("layer.addItem", json!({"item":source.0})).unwrap()["layer"].as_u64().unwrap();
    let inner = s.execute("layer.group", json!({"layers":[layer],"name":"Inner"})).unwrap()["group"].as_u64().unwrap();
    let outer = s.execute("layer.group", json!({"layers":[layer],"name":"Outer"})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group":outer,"audio":!muted})).unwrap();
    let preset = s.prefs.preview.current;
    s.prefs.preview.get_mut(preset).include_video = false;
    let output = Output::default();
    let factory = output.clone();
    let mut app = EffectcraftApp::new(s);
    app.hooks.audio_device = Some(Box::new(move |_| Some(Box::new(factory.clone()))));
    let mut h = Harness::builder().with_size(egui::vec2(1600., 900.)).with_step_dt(1.0 / 30.0).build_eframe(|_| app);
    h.run_steps(3);
    // Playback measures time with the app clock, not egui's input time.
    let now = h.state().now();
    h.state_mut().play(now);
    h.run_steps(2);
    (h, output, layer, inner, outer)
}

fn click(h: &mut Harness<'_, EffectcraftApp>, id: &str) {
    let r = h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}")).rect;
    let p = pos2(r[0] + r[2] / 2., r[1] + r[3] / 2.);
    h.event(Event::PointerMoved(p));
    h.step();
    h.event(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

fn samples(h: &Harness<'_, EffectcraftApp>) -> Vec<f32> {
    let feed = &h.state().audio.as_ref().expect("preview still has an output").feed;
    let start = Instant::now();
    while feed.queued_frames() < 512 {
        assert!(start.elapsed() < Duration::from_secs(10), "feeder did not deliver samples");
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut out = vec![0.; 1024];
    feed.pull(&mut out);
    out
}

fn audible(h: &Harness<'_, EffectcraftApp>) -> bool {
    samples(h).iter().any(|v| v.abs() > 0.001)
}

#[test]
fn group_and_leaf_switches_remix_running_audio_and_undo_without_restarting() {
    let (mut h, output, layer, inner, outer) = scene(false);
    assert!(audible(&h));
    let feed = h.state().audio.as_ref().unwrap().feed.clone();
    let span = h.state().audio.as_ref().unwrap().span;
    for id in [format!("timeline.folder.{outer}.audio"), format!("timeline.folder.{inner}.audio"), format!("timeline.layer.{layer}.audio")] {
        let before = feed.consumed();
        click(&mut h, &id);
        assert!(!audible(&h), "{id} must mute the next samples during Play");
        assert!(feed.consumed() > before, "silence must advance the clock");
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        h.run_steps(3);
        assert!(audible(&h), "Undo must restore the running mix: {id}");
        assert!(Arc::ptr_eq(&feed, &h.state().audio.as_ref().unwrap().feed));
        assert_eq!(h.state().audio.as_ref().unwrap().span, span);
        assert!(h.state().playback.playing);
        assert_eq!(*output.0.lock().unwrap(), (1, 0));
    }
    click(&mut h, &format!("timeline.folder.{inner}.audio"));
    for _ in 0..2 {
        click(&mut h, &format!("timeline.folder.{outer}.audio"));
    }
    assert!(!audible(&h), "enabling the parent retains the muted child");
    click(&mut h, &format!("timeline.folder.{inner}.audio"));
    assert!(audible(&h));
    h.state_mut().stop();
    assert_eq!(*output.0.lock().unwrap(), (1, 1));
}

#[test]
fn enabling_initially_muted_audio_starts_at_the_current_preview_position() {
    let (mut h, output, _, _, outer) = scene(true);
    assert!(h.state().audio.is_none());
    let until = Instant::now() + Duration::from_secs(10);
    while h.state().playback.audio_frame.is_none_or(|f| f < 30) {
        assert!(Instant::now() < until, "the silent preview clock did not run");
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    let frame = h.state().playback.audio_frame.unwrap();
    click(&mut h, &format!("timeline.folder.{outer}.audio"));
    assert!(audible(&h));
    let a = h.state().audio.as_ref().unwrap();
    assert!(a.span.start >= frame * 1600, "audio must join the running preview, not start over");
    assert_eq!(*output.0.lock().unwrap(), (1, 0));
    assert!(h.state().playback.playing);
    h.state_mut().stop();
}

#[test]
fn enabling_audio_during_video_preview_joins_at_the_current_frame() {
    let (mut h, output, _, _, outer) = scene(true);
    h.state_mut().stop();
    let sc = h.state().session.prefs.preview.current;
    h.state_mut().session.prefs.preview.get_mut(sc).include_video = true;
    let now = h.state().now();
    h.state_mut().play(now);
    let until = Instant::now() + Duration::from_secs(10);
    while h.state().session.time().seconds() < 0.2 {
        assert!(Instant::now() < until, "silent video did not advance");
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(h.state().audio.is_none());
    let time = h.state().session.time();
    click(&mut h, &format!("timeline.folder.{outer}.audio"));
    while h.state().audio.is_none() {
        assert!(Instant::now() < until, "audio did not join the video preview");
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(audible(&h));
    let a = h.state().audio.as_ref().unwrap();
    assert!(a.span.start >= time.to_units_floor(a.rate as i64));
    assert_eq!(*output.0.lock().unwrap(), (1, 0));
    assert!(h.state().playback.playing);
    h.state_mut().stop();
}

#[test]
fn command_changes_inside_a_precomp_update_the_running_parent_mix() {
    let (mut h, output, _, _, _) = scene(false);
    assert!(audible(&h));
    let cid = h.state().session.project.find_by_name("Tone Source").unwrap().id;
    let lid = h.state().session.project.comp(cid).unwrap().layers[0].id;
    h.state_mut().session.execute("layer.setSwitch", json!({"comp":cid.0,"layer":lid.0,"switch":"audio","value":false})).unwrap();
    h.run_steps(3);
    assert!(!audible(&h));
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert!(audible(&h));
    assert_eq!(*output.0.lock().unwrap(), (1, 0));
    h.state_mut().stop();
}

#[test]
fn folding_a_group_does_not_discard_buffered_audio() {
    let (mut h, _, _, _, outer) = scene(false);
    assert!(audible(&h));
    let feed = h.state().audio.as_ref().unwrap().feed.clone();
    let until = Instant::now() + Duration::from_secs(10);
    while feed.queued_frames() < 9600 {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
    let queued = feed.queued_frames();
    click(&mut h, &format!("timeline.folder.{outer}.twirl"));
    assert_eq!(feed.queued_frames(), queued);
    assert!(audible(&h));
    h.state_mut().stop();
}
