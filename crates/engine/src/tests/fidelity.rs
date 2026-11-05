//! Render-fidelity commands: project colour settings, footage colour profile, slip edit.

use effectcraft_project::{BitDepth, ColorSpace};
use serde_json::{Value, json};

use crate::Session;

#[test]
fn project_colour_settings_are_undoable() {
    let mut s = Session::default();
    s.execute("file.projectSettings", json!({"bitDepth": 32, "workingSpace": "rec2020", "linearize": true, "blendLinear": true})).unwrap();
    let st = &s.project.settings;
    assert_eq!((st.bit_depth, st.working_space, st.linearize, st.blend_linear), (BitDepth::Bpc32, Some(ColorSpace::Rec2020), true, true));
    s.execute("file.projectSettings", json!({"workingSpace": "Display P3"})).unwrap();
    assert_eq!(s.project.settings.working_space, Some(ColorSpace::DisplayP3));
    s.execute("file.projectSettings", json!({"workingSpace": "none"})).unwrap();
    assert_eq!(s.project.settings.working_space, None);
    assert!(s.execute("file.projectSettings", json!({"workingSpace": "acescg-ish"})).is_err());
    assert!(s.undo());
    assert_eq!(s.project.settings.working_space, Some(ColorSpace::DisplayP3));
    assert!(s.undo() && s.undo());
    assert_eq!(s.project.settings.bit_depth, BitDepth::Bpc8);
    assert!(!s.project.settings.blend_linear);
    // The Project panel's depth button cycles 8 → 16 → 32 → 8.
    for want in ["16 bpc", "32 bpc", "8 bpc"] {
        assert_eq!(s.execute("file.cycleBitDepth", json!({})).unwrap(), json!(want));
    }
}

#[test]
fn footage_colour_profile_is_interpreted() {
    let mut s = Session::default();
    let id = s.execute("file.importPlaceholder", json!({"name": "clip", "width": 64, "height": 36, "frameRate": 24, "duration": 2})).unwrap();
    let item = id["item"].as_u64().or_else(|| id.as_u64()).map(effectcraft_project::ItemId).unwrap_or_else(|| *s.project.items.keys().last().unwrap());
    let profile = |s: &Session| match &s.project.item(item).unwrap().kind {
        effectcraft_project::ItemKind::Footage(f) => f.color_profile,
        _ => panic!("not footage"),
    };
    s.execute("file.interpretFootage", json!({"items": [item.0], "colorProfile": "rec2020"})).unwrap();
    assert_eq!(profile(&s), Some(ColorSpace::Rec2020));
    s.execute("file.interpretFootage", json!({"items": [item.0], "colorProfile": "auto"})).unwrap();
    assert_eq!(profile(&s), None);
    assert!(s.undo());
    assert_eq!(profile(&s), Some(ColorSpace::Rec2020));
}

/// A 10 s comp with a 4 s nested comp shown 1 s .. 3 s into its span, and a scale key in the middle.
fn slip_scene(timing: Option<(&str, Value)>) -> (Session, u64) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 100, "height": 100, "frameRate": 30, "duration": 10})).unwrap();
    let nested = s.execute("comp.new", json!({"name": "Nested", "duration": 4, "open": false})).unwrap()["comp"].as_u64().unwrap();
    s.execute("layer.addItem", json!({"item": nested})).unwrap();
    let id = s.active_comp().unwrap().layers[0].id.0;
    if let Some((command, params)) = timing {
        s.execute(command, params).unwrap();
    }
    let l = s.active_comp().unwrap().layers[0].clone();
    let lo = l.source_span(&s.project, s.active_comp().unwrap().frame_duration()).unwrap().0.seconds();
    s.execute("layer.timing", json!({"layers": [id], "in": lo + 1.0, "out": lo + 3.0})).unwrap();
    s.execute("time.set", json!({"time": lo + 2.0})).unwrap();
    s.execute("prop.addKey", json!({"layer": id, "path": "transform/scale", "value": [50.0, 50.0]})).unwrap();
    (s, id)
}

/// The comp time of the layer's only scale key.
fn scale_key_at(s: &Session, id: u64) -> f64 {
    let l = s.active_comp().unwrap().layer(effectcraft_project::LayerId(id)).unwrap().clone();
    let keys = &l.props.sub("transform").unwrap().get("scale").unwrap().keys;
    assert_eq!(keys.len(), 1);
    l.comp_time(keys[0].time).seconds()
}

#[test]
fn slip_with_hold_keys_leaves_keyframes_in_place_in_the_comp() {
    for (case, timing) in
        [("as is", None), ("stretched", Some(("layer.timeStretch", json!({"percent": 200})))), ("reversed", Some(("layer.timeReverse", json!({}))))]
    {
        let (mut s, id) = slip_scene(timing);
        let before = scale_key_at(&s, id);
        s.execute("layer.slip", json!({"layers": [id], "delta": 0.5, "holdKeys": true})).unwrap();
        assert!((scale_key_at(&s, id) - before).abs() < 1e-6, "{case}");
        // Without the option the key travels with the source, as in After Effects.
        s.execute("layer.slip", json!({"layers": [id], "delta": -0.5})).unwrap();
        assert!((scale_key_at(&s, id) - (before - 0.5)).abs() < 1e-6, "{case}");
    }
    // One undo step puts the key's layer time back with the source.
    let (mut s, id) = slip_scene(None);
    let l0 = s.active_comp().unwrap().layers[0].clone();
    s.execute("layer.slip", json!({"layers": [id], "delta": 0.5, "holdKeys": true})).unwrap();
    assert!(s.undo());
    let l1 = s.active_comp().unwrap().layers[0].clone();
    assert_eq!(l0.start_time, l1.start_time);
    assert_eq!(l0.props.sub("transform").unwrap().get("scale").unwrap().keys[0].time, l1.props.sub("transform").unwrap().get("scale").unwrap().keys[0].time);
}

/// Played backwards, the source's first frame is the layer's last, and no frame past its end shows.
#[test]
fn trim_to_source_of_a_reversed_layer_shows_only_source_frames() {
    let (mut s, id) = slip_scene(Some(("layer.timeReverse", json!({}))));
    s.execute("layer.trimToSource", json!({"layers": [id]})).unwrap();
    let c = s.active_comp().unwrap().clone();
    let l = c.layers[0].clone();
    let frame = c.frame_duration();
    assert_eq!(l.layer_time(l.out_point - frame).seconds(), 0.0, "ends on the source's first frame");
    assert!((l.layer_time(l.in_point).seconds() - (4.0 - 1.0 / 30.0)).abs() < 1e-6, "starts on the source's last frame");
}

/// A Write On stroke's visible span is fixed in layer time: its keys stay with it and the picture.
#[test]
fn slip_with_hold_keys_leaves_paint_on_the_source() {
    let (mut s, id) = slip_scene(None);
    s.execute("paint.stroke", json!({"layer": id, "points": [[10.0, 10.0], [20.0, 20.0]], "time": 2.0, "duration": 0.5, "durationMode": "writeOn"})).unwrap();
    let paint_keys = |s: &Session| {
        let l = s.active_comp().unwrap().layers[0].clone();
        let mut keys = vec![];
        l.props.sub("effects").unwrap().walk("", &mut |_, p| keys.extend(p.keys.iter().map(|k| k.time)));
        keys
    };
    let before = paint_keys(&s);
    assert!(!before.is_empty(), "the stroke has Write On keys");
    let scale = scale_key_at(&s, id);
    // A selected paint key stays selected: its reference holds the same time as the key.
    let mut paint_prop = None;
    s.active_comp().unwrap().layers[0].props.sub("effects").unwrap().walk("", &mut |_, p| {
        if !p.keys.is_empty() {
            paint_prop = Some((p.uid, p.keys[0].time));
        }
    });
    let (prop, time) = paint_prop.unwrap();
    s.state.selected_keys.push(crate::doc_state::KeyRef { layer: effectcraft_project::LayerId(id), prop, time });
    s.execute("layer.slip", json!({"layers": [id], "delta": 0.5, "holdKeys": true})).unwrap();
    assert_eq!(paint_keys(&s), before, "paint keys travel with the source");
    assert_eq!(s.state.selected_keys[0].time, time, "the selection follows the key");
    assert!((scale_key_at(&s, id) - scale).abs() < 1e-6, "the scale key holds in the comp");
}

#[test]
fn trim_to_source_covers_the_whole_source() {
    let (mut s, id) = slip_scene(None);
    s.execute("layer.slip", json!({"layers": [id], "delta": 0.5})).unwrap();
    s.execute("layer.trimToSource", json!({"layers": [id]})).unwrap();
    let l = s.active_comp().unwrap().layers[0].clone();
    assert_eq!((l.in_point.seconds(), l.out_point.seconds(), l.start_time.seconds()), (0.5, 4.5, 0.5));
    // No source of its own: refused, nothing changes.
    s.execute("layer.newSolid", json!({"color": "#ff0000"})).unwrap();
    assert!(s.execute("layer.trimToSource", json!({})).is_err());
}

#[test]
fn slip_moves_the_source_under_fixed_in_and_out() {
    use effectcraft_time::Tick;
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 100, "height": 100, "frameRate": 30, "duration": 10})).unwrap();
    let nested = s.execute("comp.new", json!({"name": "Nested", "duration": 4, "open": false})).unwrap()["comp"].as_u64().unwrap();
    s.execute("layer.addItem", json!({"item": nested})).unwrap();
    let id = s.active_comp().unwrap().layers[0].id.0;
    // Trim to 1 s .. 3 s of a 4 s source.
    s.execute("layer.timing", json!({"layers": [id], "in": 1.0, "out": 3.0})).unwrap();
    let get = |s: &Session| s.active_comp().unwrap().layers[0].clone();
    let before = get(&s);
    assert_eq!((before.start_time, before.in_point.seconds(), before.out_point.seconds()), (Tick::ZERO, 1.0, 3.0));
    // Slip the source 0.5 s later: in/out stay, the source (start) moves.
    let r = s.execute("layer.slip", json!({"layers": [id], "delta": 0.5})).unwrap();
    assert_eq!(r["slipped"][0]["clamped"], json!(false));
    let l = get(&s);
    assert_eq!((l.start_time.seconds(), l.in_point, l.out_point), (0.5, before.in_point, before.out_point));
    // The source time at comp 2 s is now 1.5 s.
    assert_eq!(l.layer_time(Tick::from_seconds_f64(2.0)).seconds(), 1.5);
    // Clamped so the source still covers in..out: at most start = in (1 s).
    let r = s.execute("layer.slip", json!({"layers": [id], "delta": 5.0})).unwrap();
    assert_eq!(get(&s).start_time.seconds(), 1.0);
    assert_eq!(r["slipped"][0]["delta"], json!(0.5));
    // The response says the 5 s asked for was cut short (#2371).
    assert_eq!((r["requested"].clone(), r["slipped"][0]["clamped"].clone()), (json!(5.0), json!(true)));
    // Slip by a frame.
    s.execute("layer.slipBack", json!({"layers": [id]})).unwrap();
    assert!((get(&s).start_time.seconds() - (1.0 - 1.0 / 30.0)).abs() < 1e-9);
    s.execute("layer.slipForward", json!({"layers": [id]})).unwrap();
    assert_eq!(get(&s).start_time.seconds(), 1.0);
    // Undo walks back through the slips.
    assert!(s.undo() && s.undo() && s.undo() && s.undo());
    assert_eq!(get(&s).start_time, Tick::ZERO);
    // Alt+PageUp / Alt+PageDown move the layer; the one-frame slips have no default key.
    assert_eq!(crate::find_command("layer.slipBack").and_then(|c| c.shortcut), None);
    assert_eq!(crate::find_command("layer.slipForward").and_then(|c| c.shortcut), None);
    // Layers without a source duration can't slip.
    s.execute("layer.newSolid", json!({"color": "#ff0000"})).unwrap();
    assert!(s.execute("layer.slip", json!({"delta": 0.5})).is_err());
}
