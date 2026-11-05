//! Shift snapping is a real pointer gesture, with frame timing and one undo step.
use effectcraft_engine::Session;
use effectcraft_engine::project::LayerId;
use effectcraft_ui_egui::EffectcraftApp;
use egui::{Event, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness(pps: f64) -> (Harness<'static, EffectcraftApp>, LayerId, LayerId, u64) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name":"Snap", "width":320, "height":180, "duration":6, "frameRate":30})).unwrap();
    let moving = LayerId(s.execute("layer.newSolid", json!({"name":"Moving", "color":"#406080"})).unwrap()["layer"].as_u64().unwrap());
    let other = LayerId(s.execute("layer.newSolid", json!({"name":"Target", "color":"#804060"})).unwrap()["layer"].as_u64().unwrap());
    s.execute("layer.timing", json!({"layers":[moving.0], "in":0.5, "out":3.5})).unwrap();
    s.execute("layer.timing", json!({"layers":[other.0], "in":2.5, "out":3.0})).unwrap();
    for (layer, time) in [(moving, 1.0), (other, 2.0)] {
        s.execute("prop.addKey", json!({"layer":layer.0, "path":"transform/opacity", "time":time, "value":70})).unwrap();
    }
    s.execute("layer.select", json!({"layers":[moving.0,other.0]})).unwrap();
    s.execute("time.set", json!({"time":1.5})).unwrap();
    let uid = s.active_comp().unwrap().layer(moving).unwrap().props.prop("transform/opacity").unwrap().uid;
    let mut h = Harness::builder().with_size(vec2(1900.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "timeline.reveal.opacity", json!({})).unwrap();
    h.state_mut().session.execute("layer.select", json!({"layers":[moving.0]})).unwrap();
    h.state_mut().ui.timeline.pps = Some(pps);
    h.run_steps(3);
    (h, moving, other, uid)
}

fn rect(h: &Harness<'_, EffectcraftApp>, id: &str) -> Rect {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}"));
    Rect::from_min_size(pos2(e.rect[0], e.rect[1]), vec2(e.rect[2], e.rect[3]))
}

fn start(h: &Harness<'_, EffectcraftApp>, layer: LayerId, uid: u64, kind: &str) -> (Pos2, f64) {
    match kind {
        "key" => (rect(h, &format!("timeline.key.{uid}.30")).center(), 1.0),
        "move" => (rect(h, &format!("timeline.layer.{}.bar", layer.0)).center(), 0.5),
        "in" => (rect(h, &format!("timeline.layer.{}.bar.in", layer.0)).center(), 0.5),
        _ => (rect(h, &format!("timeline.layer.{}.bar.out", layer.0)).center(), 3.5),
    }
}

fn at(h: &Harness<'_, EffectcraftApp>, layer: LayerId, kind: &str) -> f64 {
    let l = h.state().session.active_comp().unwrap().layer(layer).unwrap();
    match kind {
        "key" => l.comp_time(l.props.prop("transform/opacity").unwrap().keys[0].time).seconds(),
        "in" | "move" => l.in_point.seconds(),
        _ => l.out_point.seconds(),
    }
}

fn held_drag(h: &mut Harness<'_, EffectcraftApp>, from: Pos2, to: Pos2, modifiers: Modifiers) {
    h.event(Event::ModifiersChanged(modifiers));
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers });
    h.step();
    let first = from + vec2(12.0, 0.0);
    h.event(Event::PointerMoved(first));
    h.step();
    for i in 1..=12 {
        h.event(Event::PointerMoved(first + (to - first) * (i as f32 / 12.0)));
        h.step();
    }
    h.step();
}

fn release(h: &mut Harness<'_, EffectcraftApp>, pos: Pos2) {
    h.event(Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(3);
    assert!(h.state().auto.find("timeline.snap").is_none(), "the snap guide ends with the gesture");
}

fn open_graph_at_scale(h: &mut Harness<'_, EffectcraftApp>, pps: f64) {
    h.state_mut().ui.timeline.graph_editor = true;
    // Opening the graph now fits its selection first; choose the gesture's scale afterwards.
    h.run_steps(3);
    h.state_mut().ui.timeline.pps = Some(pps);
    h.state_mut().ui.timeline.start = 0.0;
    h.run_steps(3);
    assert_eq!(h.state().ui.timeline.pps, Some(pps));
}

#[test]
fn collapsed_layer_folder_hides_keys_but_keeps_its_coverage_edges_as_targets() {
    for kind in ["key", "move", "in", "out"] {
        for target in [2.0, 2.5, 3.0] {
            let (mut h, moving, other, uid) = harness(100.0);
            let group = h.state_mut().session.execute("layer.group", json!({"layers":[other.0]})).unwrap()["group"].as_u64().unwrap();
            h.state_mut().session.execute("layer.groupUpdate", json!({"group":group,"collapsed":true})).unwrap();
            if kind == "move" {
                h.state_mut().session.execute("layer.timing", json!({"layers":[moving.0],"out":1.6})).unwrap();
            }
            h.run_steps(3);
            assert!(h.state().auto.find(&format!("timeline.layer.{}.row", other.0)).is_none());
            let (from, anchor) = start(&h, moving, uid, kind);
            let to = from + vec2(((target - anchor) * 100.) as f32 + 7.5, 0.);
            held_drag(&mut h, from, to, Modifiers::SHIFT);
            if target == 2.0 {
                // The hidden layer's key stays out of reach.
                assert!((at(&h, moving, kind) - target).abs() > 1e-6, "hidden {kind} target {target}");
                assert!(h.state().auto.find("timeline.snap").is_none());
            } else {
                // Its in/out edges are the folder's coverage edges, which remain targets.
                assert!((at(&h, moving, kind) - target).abs() < 1e-6, "{kind} snaps to the folder edge {target}");
                assert_eq!(h.state().auto.find("timeline.snap").map(|e| e.label.as_str()), Some("Group: Group"));
            }
            release(&mut h, to);
        }
    }
}

#[test]
fn keys_and_both_layer_edges_snap_to_all_requested_targets_only_with_shift() {
    for pps in [100.0, 200.0] {
        for kind in ["key", "in", "out", "move"] {
            for (target, label) in [(1.5, "Current Time"), (2.0, "Keyframe"), (2.5, "In"), (3.0, "Out")] {
                for shift in [false, true] {
                    let (mut h, moving, _, uid) = harness(pps);
                    if kind == "move" {
                        h.state_mut().session.execute("layer.timing", json!({"layers":[moving.0],"out":1.6})).unwrap();
                        h.run_steps(2);
                    }
                    let before = h.state().session.project.to_file_json().unwrap();
                    let undo = h.state().session.history.undo.len();
                    let (from, anchor) = start(&h, moving, uid, kind);
                    let to = from + vec2(((target - anchor) * pps) as f32 + 7.5, 0.0);
                    held_drag(&mut h, from, to, Modifiers { shift, ..Default::default() });
                    let hit = h.state().auto.find("timeline.snap");
                    if shift {
                        assert!((at(&h, moving, kind) - target).abs() < 1e-6, "{kind} at {pps}: expected {target}, got {}", at(&h, moving, kind));
                        assert!(hit.is_some_and(|e| e.label.starts_with(label)), "{kind}: {hit:?}");
                    } else {
                        assert!((at(&h, moving, kind) - target).abs() > 1e-6, "free {kind} must not snap");
                        assert!(hit.is_none());
                    }
                    release(&mut h, to);
                    assert_eq!(h.state().session.time().seconds(), 1.5, "drag leaves the playhead alone");
                    assert_eq!(h.state().session.history.undo.len(), undo + 1, "one undo per gesture");
                    let after = h.state().session.project.to_file_json().unwrap();
                    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
                    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
                    h.state_mut().session.execute("edit.redo", json!({})).unwrap();
                    assert_eq!(h.state().session.project.to_file_json().unwrap(), after);
                }
            }
        }
    }
}

#[test]
fn shift_can_toggle_during_a_drag_and_does_not_hold_outside_eight_points() {
    for kind in ["key", "in", "out", "move"] {
        let (mut h, moving, _, uid) = harness(100.0);
        let (from, anchor) = start(&h, moving, uid, kind);
        let to = from + vec2(((2.0 - anchor) * 100.0) as f32 + 7.5, 0.0);
        held_drag(&mut h, from, to, Modifiers::NONE);
        h.event(Event::ModifiersChanged(Modifiers::SHIFT));
        h.run_steps(2);
        assert!((at(&h, moving, kind) - 2.0).abs() < 1e-6);
        h.event(Event::ModifiersChanged(Modifiers::NONE));
        h.run_steps(2);
        assert!((at(&h, moving, kind) - 2.0).abs() > 1e-6, "releasing Shift restores free motion");
        h.event(Event::ModifiersChanged(Modifiers::SHIFT));
        let far = to + vec2(15.0, 0.0);
        h.event(Event::PointerMoved(far));
        h.run_steps(2);
        assert!((at(&h, moving, kind) - 2.0).abs() > 0.1);
        assert!(h.state().auto.find("timeline.snap").is_none());
        release(&mut h, far);
    }
}

#[test]
fn a_stationary_key_at_the_moving_keys_time_remains_a_target() {
    let (mut h, moving, other, uid) = harness(100.0);
    h.state_mut().session.execute("prop.addKey", json!({"layer":other.0, "path":"transform/opacity", "time":1.0, "value":50})).unwrap();
    h.run_steps(2);
    let (from, _) = start(&h, moving, uid, "key");
    let to = from + vec2(7.5, 0.0);
    held_drag(&mut h, from, to, Modifiers::SHIFT);
    assert!((at(&h, moving, "key") - 1.0).abs() < 1e-6);
    assert!(h.state().auto.find("timeline.snap").is_some_and(|e| e.label == "Keyframe: Target"));
    release(&mut h, to);
}

#[test]
fn group_key_drag_preserves_spacing_and_ignores_all_moving_keys() {
    let (mut h, moving, _, uid) = harness(100.0);
    h.state_mut().session.execute("prop.addKey", json!({"layer":moving.0, "path":"transform/opacity", "time":1.2, "value":80})).unwrap();
    h.state_mut().session.execute("keys.select", json!({"keys":[{"layer":moving.0,"prop":uid,"time":1.0},{"layer":moving.0,"prop":uid,"time":1.2}]})).unwrap();
    h.run_steps(2);
    let (from, _) = start(&h, moving, uid, "key");
    let to = from + vec2(107.5, 0.0);
    held_drag(&mut h, from, to, Modifiers::SHIFT);
    release(&mut h, to);
    let layer = h.state().session.active_comp().unwrap().layer(moving).unwrap();
    let times: Vec<_> = layer.props.prop("transform/opacity").unwrap().keys.iter().map(|k| k.time.seconds()).collect();
    assert!((times[0] - 2.0).abs() < 1e-6 && (times[1] - 2.2).abs() < 1e-6, "{times:?}");
}

#[test]
fn shifted_layers_supply_keys_in_composition_time() {
    let (mut h, moving, other, uid) = harness(100.0);
    h.state_mut().session.execute("layer.timing", json!({"layers":[other.0],"start":0.5})).unwrap();
    h.run_steps(2);
    let (from, _) = start(&h, moving, uid, "key");
    let to = from + vec2(157.5, 0.0);
    held_drag(&mut h, from, to, Modifiers::SHIFT);
    assert!((at(&h, moving, "key") - 2.5).abs() < 1e-6);
    assert!(h.state().auto.find("timeline.snap").is_some_and(|e| e.label == "Keyframe: Target"));
    release(&mut h, to);
}

#[test]
fn trimming_selected_layers_preserves_their_relative_edges() {
    let (mut h, moving, _, uid) = harness(100.0);
    let group = LayerId(h.state_mut().session.execute("layer.newSolid", json!({"name":"Group", "color":"#406080"})).unwrap()["layer"].as_u64().unwrap());
    h.state_mut().session.execute("layer.timing", json!({"layers":[group.0], "in":0.3, "out":5.0})).unwrap();
    h.state_mut().session.execute("layer.select", json!({"layers":[moving.0,group.0]})).unwrap();
    h.run_steps(3);
    let (from, _) = start(&h, moving, uid, "in");
    let to = from + vec2(107.5, 0.0);
    held_drag(&mut h, from, to, Modifiers::SHIFT);
    release(&mut h, to);
    assert!((at(&h, moving, "in") - 1.5).abs() < 1e-6);
    assert!((at(&h, group, "in") - 1.3).abs() < 1e-6);
}

#[test]
fn trimming_cannot_snap_a_layer_to_zero_duration() {
    for kind in ["in", "out"] {
        let (mut h, moving, _, uid) = harness(100.0);
        let (from, anchor) = start(&h, moving, uid, kind);
        let target = if kind == "in" { 3.5 } else { 0.5 };
        let to = from + vec2(((target - anchor) * 100.0) as f32, 0.0);
        held_drag(&mut h, from, to, Modifiers::SHIFT);
        let layer = h.state().session.active_comp().unwrap().layer(moving).unwrap();
        assert!(layer.out_point - layer.in_point >= h.state().session.active_comp().unwrap().frame_duration());
        assert!(h.state().auto.find("timeline.snap").is_none(), "cannot claim a snap to an invalid edge");
        release(&mut h, to);
    }
}

#[test]
fn shift_snaps_graph_keys_even_with_the_graph_snap_toggle_off() {
    for shift in [false, true] {
        let (mut h, moving, other, uid) = harness(100.0);
        let other_uid = h.state().session.active_comp().unwrap().layer(other).unwrap().props.prop("transform/opacity").unwrap().uid;
        h.state_mut().session.execute("prop.select", json!({"layer":other.0,"prop":other_uid})).unwrap();
        h.state_mut().session.execute("prop.select", json!({"layer":moving.0,"prop":uid,"add":true})).unwrap();
        h.state_mut().session.execute("keys.select", json!({"keys":[{"layer":moving.0,"prop":uid,"time":1.0}]})).unwrap();
        h.state_mut().ui.timeline.graph_snap = false;
        open_graph_at_scale(&mut h, 100.0);
        assert!(h.state().auto.find(&format!("timeline.graph.key.{other_uid}.0.0")).is_some(), "the snap target is visible");
        let from = rect(&h, &format!("timeline.graph.key.{uid}.0.0")).center();
        let to = from + vec2(107.5, 0.0);
        held_drag(&mut h, from, to, Modifiers { shift, ..Default::default() });
        assert_eq!((at(&h, moving, "key") - 2.0).abs() < 1e-6, shift);
        assert_eq!(h.state().auto.find("timeline.snap").is_some(), shift);
        release(&mut h, to);
        let layer = h.state().session.active_comp().unwrap().layer(moving).unwrap();
        assert_eq!(layer.props.prop("transform/opacity").unwrap().keys[0].value.as_f64(), 70.0);
    }
}

#[test]
fn snapping_to_another_key_on_the_same_property_stays_stable_until_release() {
    let (mut h, moving, _, uid) = harness(100.0);
    h.state_mut().session.execute("prop.addKey", json!({"layer":moving.0,"path":"transform/opacity","time":1.8,"value":90})).unwrap();
    h.run_steps(2);
    let (from, _) = start(&h, moving, uid, "key");
    let to = from + vec2(87.5, 0.0);
    held_drag(&mut h, from, to, Modifiers::SHIFT);
    for _ in 0..6 {
        let selected = &h.state().session.state.selected_keys;
        assert_eq!(selected.len(), 1);
        assert!((selected[0].time.seconds() - 1.8).abs() < 1e-6, "moving key: {selected:?}");
        assert!(h.state().auto.find("timeline.snap").is_some());
        h.step();
    }
    release(&mut h, to);
}

#[test]
fn moving_by_the_body_snaps_either_edge_and_preserves_duration_source_and_keys() {
    for pps in [100.0, 200.0] {
        for out in [false, true] {
            for target in [1.5, 2.0, 2.5, 3.0] {
                let (mut h, moving, _, uid) = harness(pps);
                h.state_mut().session.execute("layer.timing", json!({"layers":[moving.0],"out":1.3})).unwrap();
                h.run_steps(2);
                let before = h.state().session.active_comp().unwrap().layer(moving).unwrap().clone();
                let (from, _) = start(&h, moving, uid, "move");
                let anchor = if out { 1.3 } else { 0.5 };
                let to = from + vec2(((target - anchor) * pps) as f32 + 7.5, 0.0);
                held_drag(&mut h, from, to, Modifiers::SHIFT);
                let l = h.state().session.active_comp().unwrap().layer(moving).unwrap();
                let edge = if out { l.out_point } else { l.in_point };
                assert!((edge.seconds() - target).abs() < 1e-6, "out={out}, target={target}: {}", edge.seconds());
                assert_eq!(l.out_point - l.in_point, before.out_point - before.in_point);
                assert_eq!(l.start_time - before.start_time, l.in_point - before.in_point);
                assert_eq!(serde_json::to_value(&l.props).unwrap(), serde_json::to_value(&before.props).unwrap());
                assert!(h.state().auto.find("timeline.snap").is_some());
                release(&mut h, to);
            }
        }
    }
}

#[test]
fn hidden_keys_are_not_targets_for_keys_trims_or_body_moves() {
    for kind in ["key", "in", "out", "move"] {
        for hidden in ["collapsed", "group", "property", "shy", "search", "below", "above"] {
            // A name-only search hides the moving key too; its bar is still draggable.
            if kind == "key" && hidden == "search" {
                continue;
            }
            let (mut h, moving, other, uid) = harness(100.0);
            let target_uid = h.state().session.active_comp().unwrap().layer(other).unwrap().props.prop("transform/opacity").unwrap().uid;
            match hidden {
                "collapsed" => {
                    h.state_mut().ui.timeline.open_layers.remove(&other.0);
                    h.state_mut().ui.timeline.layer_reveal.remove(&other.0);
                }
                "property" => {
                    h.state_mut().ui.timeline.layer_reveal.insert(other.0, vec!["position".into()]);
                }
                "group" => {
                    h.state_mut().ui.timeline.layer_reveal.remove(&other.0);
                    h.state_mut().ui.timeline.open_groups.clear();
                }
                "shy" => {
                    h.state_mut().session.execute("layer.setSwitch", json!({"layers":[other.0],"switch":"shy","value":true})).unwrap();
                    h.state_mut().session.execute("comp.setSwitch", json!({"switch":"hideShy","value":true})).unwrap();
                }
                "search" => h.state_mut().ui.timeline.search = "Moving".into(),
                "above" | "below" => {
                    // Lots of layers put the target outside the vertical viewport.
                    for _ in 0..30 {
                        h.state_mut().session.execute("layer.newSolid", json!({"color":"#406080"})).unwrap();
                    }
                    if hidden == "below" {
                        h.state_mut().session.execute("layer.arrange", json!({"layers":[moving.0],"index":0})).unwrap();
                        h.state_mut().session.execute("layer.arrange", json!({"layers":[other.0],"to":"back"})).unwrap();
                    } else {
                        h.state_mut().session.execute("layer.arrange", json!({"layers":[other.0],"index":0})).unwrap();
                        h.state_mut().ui.timeline.scroll_y = 10000.0;
                    }
                    h.state_mut().session.execute("layer.select", json!({"layers":[moving.0]})).unwrap();
                }
                _ => unreachable!(),
            }
            h.run_steps(3);
            assert!(h.state().auto.find(&format!("timeline.key.{target_uid}.60")).is_none(), "{hidden}");
            let (from, anchor) = start(&h, moving, uid, kind);
            let to = from + vec2(((2.0 - anchor) * 100.0) as f32 + 7.5, 0.0);
            held_drag(&mut h, from, to, Modifiers::SHIFT);
            assert!((at(&h, moving, kind) - 2.0).abs() > 1e-6, "{kind} / {hidden}");
            assert!(h.state().auto.find("timeline.snap").is_none(), "{kind} / {hidden}");
            release(&mut h, to);
        }
    }
}

#[test]
fn body_move_excludes_all_targets_of_the_moving_selection() {
    let (mut h, moving, other, uid) = harness(100.0);
    h.state_mut().session.execute("time.set", json!({"time":4.0})).unwrap();
    h.state_mut().session.execute("layer.select", json!({"layers":[moving.0,other.0]})).unwrap();
    h.run_steps(2);
    let before = h.state().session.project.to_file_json().unwrap();
    let undo = h.state().session.history.undo.len();
    let (from, _) = start(&h, moving, uid, "move");
    let to = from + vec2(7.5, 0.0);
    held_drag(&mut h, from, to, Modifiers::SHIFT);
    assert!(h.state().auto.find("timeline.snap").is_none());
    assert!((at(&h, moving, "move") - (0.5 + 2.0 / 30.0)).abs() < 1e-6);
    assert!((at(&h, other, "move") - at(&h, moving, "move") - 2.0).abs() < 1e-6);
    release(&mut h, to);
    assert_eq!(h.state().session.history.undo.len(), undo + 1);
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
}

#[test]
fn graph_keys_only_snap_to_shown_curves_and_visible_values() {
    for hidden in ["unselected", "vertical", "bar"] {
        let (mut h, moving, other, uid) = harness(100.0);
        let target_uid = h.state().session.active_comp().unwrap().layer(other).unwrap().props.prop("transform/opacity").unwrap().uid;
        h.state_mut().session.execute("prop.select", json!({"layer":moving.0,"prop":uid})).unwrap();
        if hidden == "vertical" {
            h.state_mut().session.execute("prop.select", json!({"layer":other.0,"prop":target_uid,"add":true})).unwrap();
            h.state_mut().session.execute("prop.addKey", json!({"layer":other.0,"path":"transform/opacity","time":2.0,"value":10})).unwrap();
        }
        open_graph_at_scale(&mut h, 100.0);
        h.state_mut().ui.timeline.graph_auto_zoom = false;
        h.state_mut().ui.timeline.graph_range = Some((50.0, 90.0));
        h.run_steps(3);
        assert!(h.state().auto.find(&format!("timeline.graph.key.{target_uid}.0.0")).is_none(), "{hidden}: the target is hidden");
        let from = rect(&h, &format!("timeline.graph.key.{uid}.0.0")).center();
        let target = if hidden == "bar" { 2.5 } else { 2.0 };
        let to = from + vec2(((target - 1.0) * 100.0) as f32 + 7.5, 0.0);
        held_drag(&mut h, from, to, Modifiers::SHIFT);
        assert!((at(&h, moving, "key") - target).abs() > 1e-6, "{hidden}");
        assert!(h.state().auto.find("timeline.snap").is_none(), "{hidden}");
        release(&mut h, to);
    }
}
#[test]
fn keys_outside_the_time_view_are_not_targets() {
    for kind in ["key", "in", "out", "move"] {
        let (mut h, moving, other, uid) = harness(100.0);
        // The view is clamped to the comp's end in 0.10.0; leave room to scroll past the key.
        h.state_mut().session.execute("comp.settings", json!({"duration":20})).unwrap();
        h.state_mut().session.execute("layer.timing", json!({"layers":[moving.0],"in":3.0,"out":4.1})).unwrap();
        h.state_mut().session.execute("prop.addKey", json!({"layer":moving.0,"prop":uid,"time":3.5,"value":60})).unwrap();
        h.state_mut().ui.timeline.start = 2.2;
        h.run_steps(3);
        let other_uid = h.state().session.active_comp().unwrap().layer(other).unwrap().props.prop("transform/opacity").unwrap().uid;
        assert!(h.state().auto.find(&format!("timeline.key.{other_uid}.60")).is_none());
        let (from, anchor) = match kind {
            "key" => (rect(&h, &format!("timeline.key.{uid}.105")).center(), 3.5),
            "in" => (rect(&h, &format!("timeline.layer.{}.bar.in", moving.0)).center(), 3.0),
            "out" => (rect(&h, &format!("timeline.layer.{}.bar.out", moving.0)).center(), 4.1),
            _ => (rect(&h, &format!("timeline.layer.{}.bar", moving.0)).center(), 3.0),
        };
        let to = from + vec2(((2.0 - anchor) * 100.0) as f32 + 7.5, 0.0);
        held_drag(&mut h, from, to, Modifiers::SHIFT);
        let time = if kind == "key" { h.state().session.state.selected_keys[0].time.seconds() } else { at(&h, moving, kind) };
        assert!((time - 2.0).abs() > 1e-6, "{kind}");
        assert!(h.state().auto.find("timeline.snap").is_none(), "{kind}");
        release(&mut h, to);
    }
}
#[test]
fn ruler_and_work_area_share_the_visible_key_targets() {
    for work_area in [false, true] {
        for visible in [false, true] {
            let (mut h, moving, other, uid) = harness(100.0);
            if !visible {
                h.state_mut().ui.timeline.open_layers.remove(&other.0);
                h.state_mut().ui.timeline.layer_reveal.remove(&other.0);
            }
            h.run_steps(2);
            let key_x = rect(&h, &format!("timeline.key.{uid}.30")).center().x;
            let (from, anchor) =
                if work_area { (rect(&h, "timeline.workArea.end").center(), 6.0) } else { (pos2(key_x + 50.0, rect(&h, "timeline.ruler").center().y), 1.5) };
            let to = from + vec2(((2.0 - anchor) * 100.0) as f32 + 7.5, 0.0);
            held_drag(&mut h, from, to, Modifiers::SHIFT);
            let time = if work_area { h.state().session.active_comp().unwrap().work_area.1.seconds() } else { h.state().session.time().seconds() };
            assert_eq!((time - 2.0).abs() < 1e-6, visible, "work area={work_area}, visible={visible}, time={time}");
            release(&mut h, to);
            assert_eq!(h.state().session.state.selected_layers, vec![moving]);
        }
    }
}

#[test]
fn graph_collision_with_a_visible_key_stays_snapped() {
    let (mut h, moving, _, uid) = harness(100.0);
    h.state_mut().session.execute("prop.addKey", json!({"layer":moving.0,"prop":uid,"time":1.8,"value":90})).unwrap();
    h.state_mut().session.execute("prop.select", json!({"layer":moving.0,"prop":uid})).unwrap();
    h.state_mut().session.execute("keys.select", json!({"keys":[{"layer":moving.0,"prop":uid,"time":1.0}]})).unwrap();
    open_graph_at_scale(&mut h, 100.0);
    assert!(h.state().auto.find(&format!("timeline.graph.key.{uid}.0.1")).is_some(), "the stationary key is visible");
    let from = rect(&h, &format!("timeline.graph.key.{uid}.0.0")).center();
    let to = from + vec2(87.5, 0.0);
    held_drag(&mut h, from, to, Modifiers::SHIFT);
    for _ in 0..6 {
        assert!((h.state().session.state.selected_keys[0].time.seconds() - 1.8).abs() < 1e-6);
        assert!(h.state().auto.find("timeline.snap").is_some());
        h.step();
    }
    release(&mut h, to);
}
