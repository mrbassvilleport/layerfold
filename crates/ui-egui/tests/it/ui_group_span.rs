//! Read-only coverage bars of timeline folders: drawn from the live document, never edited.
use effectcraft_engine::Session;
use effectcraft_ui_egui::EffectcraftApp;
use egui::{Event, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// Folder G = {A [0,2), B [1.5,3), C [4,5)} above a free layer X [0,0.5). Returns (h, [A,B,C,X], G).
fn harness() -> (Harness<'static, EffectcraftApp>, [u64; 4], u64) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name":"Span","width":320,"height":180,"duration":6,"frameRate":30})).unwrap();
    let mut ids = [0; 4];
    for (i, (name, a, b)) in [("X", 0.0, 0.5), ("C", 4.0, 5.0), ("B", 1.5, 3.0), ("A", 0.0, 2.0)].into_iter().enumerate() {
        let id = s.execute("layer.newSolid", json!({"name":name,"color":"#406080"})).unwrap()["layer"].as_u64().unwrap();
        s.execute("layer.timing", json!({"layers":[id],"in":a,"out":b})).unwrap();
        ids[3 - i] = id;
    }
    let g = s.execute("layer.group", json!({"layers":[ids[0],ids[1],ids[2]],"name":"G"})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.select", json!({"layers":[ids[3]]})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1900., 1000.)).with_step_dt(1.0 / 60.0).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    (h, ids, g)
}

fn rect(h: &Harness<'_, EffectcraftApp>, id: &str) -> Rect {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}"));
    Rect::from_min_size(pos2(e.rect[0], e.rect[1]), vec2(e.rect[2], e.rect[3]))
}

fn press(h: &mut Harness<'_, EffectcraftApp>, p: Pos2, pressed: bool) {
    h.event(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.step();
}

fn click(h: &mut Harness<'_, EffectcraftApp>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    h.step();
    press(h, p, true);
    press(h, p, false);
}

fn collapsed(h: &Harness<'_, EffectcraftApp>, g: u64) -> bool {
    h.state().session.active_comp().unwrap().layer_groups.iter().find(|x| x.id == g).unwrap().collapsed
}

#[test]
fn span_shows_for_collapsed_and_expanded_groups() {
    let (mut h, [a, _, c, _], g) = harness();
    let expanded = rect(&h, &format!("timeline.folder.{g}.span"));
    assert!((expanded.min.x - rect(&h, &format!("timeline.layer.{a}.bar")).min.x).abs() <= 1.0);
    assert!((expanded.max.x - rect(&h, &format!("timeline.layer.{c}.bar")).max.x).abs() <= 1.0);
    h.state_mut().session.execute("layer.groupUpdate", json!({"group":g,"collapsed":true})).unwrap();
    h.run_steps(3);
    assert!(h.state().auto.find(&format!("timeline.layer.{a}.bar")).is_none(), "children are hidden");
    let folded = rect(&h, &format!("timeline.folder.{g}.span"));
    assert!((folded.min.x - expanded.min.x).abs() <= 1.0 && (folded.max.x - expanded.max.x).abs() <= 1.0);
}

#[test]
fn span_follows_a_held_move_and_undo() {
    let (mut h, [_, _, c, _], g) = harness();
    let before = rect(&h, &format!("timeline.folder.{g}.span"));
    let from = rect(&h, &format!("timeline.layer.{c}.bar")).center();
    h.event(Event::PointerMoved(from));
    h.step();
    press(&mut h, from, true);
    for i in 1..=10 {
        h.event(Event::PointerMoved(from + vec2(6.0 * i as f32, 0.0)));
        h.step();
    }
    h.run_steps(2);
    let moved = rect(&h, &format!("timeline.layer.{c}.bar")).max.x;
    assert!(moved > before.max.x + 20.0, "C moved while held");
    assert!((rect(&h, &format!("timeline.folder.{g}.span")).max.x - moved).abs() <= 1.0, "span follows during the gesture");
    press(&mut h, from + vec2(60.0, 0.0), false);
    h.run_steps(3);
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert!((rect(&h, &format!("timeline.folder.{g}.span")).max.x - before.max.x).abs() <= 1.0, "undo restores the span");
}

#[test]
fn dragging_the_span_changes_nothing() {
    let (mut h, _, g) = harness();
    let span = rect(&h, &format!("timeline.folder.{g}.span"));
    let before = h.state().session.project.to_file_json().unwrap();
    let selection = h.state().session.state.selected_layers.clone();
    let undo = h.state().session.history.undo.len();
    let from = pos2(span.min.x + 10.0, span.center().y);
    h.event(Event::PointerMoved(from));
    h.step();
    press(&mut h, from, true);
    for i in 1..=12 {
        h.event(Event::PointerMoved(from + vec2(10.0 * i as f32, 0.0)));
        h.step();
    }
    press(&mut h, from + vec2(120.0, 0.0), false);
    h.run_steps(3);
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
    assert_eq!(h.state().session.state.selected_layers, selection);
    assert_eq!(h.state().session.history.undo.len(), undo);
}

#[test]
fn click_selects_members_and_double_click_toggles_the_group() {
    let (mut h, [a, b, c, _], g) = harness();
    let span = rect(&h, &format!("timeline.folder.{g}.span"));
    let p = pos2(span.min.x + 10.0, span.center().y);
    click(&mut h, p);
    h.run_steps(30);
    let mut selected: Vec<_> = h.state().session.state.selected_layers.iter().map(|l| l.0).collect();
    selected.sort();
    let mut want = vec![a, b, c];
    want.sort();
    assert_eq!(selected, want);
    for expect in [true, false] {
        let span = rect(&h, &format!("timeline.folder.{g}.span"));
        let p = pos2(span.min.x + 10.0, span.center().y);
        click(&mut h, p);
        click(&mut h, p);
        h.run_steps(30);
        assert_eq!(collapsed(&h, g), expect);
    }
}

/// Screen x of comp time `t` from the span drawn for G (0 s .. 5 s).
fn x_at(h: &Harness<'_, EffectcraftApp>, g: u64, t: f64) -> f32 {
    let span = rect(h, &format!("timeline.folder.{g}.span"));
    span.min.x + (span.width() * (t / 5.0) as f32)
}

fn hint(h: &Harness<'_, EffectcraftApp>) -> Option<String> {
    h.state().auto.find("timeline.snap").map(|e| e.label.clone())
}

/// Shift-held drag; returns the snap hints seen while held, then releases.
fn shift_drag(h: &mut Harness<'_, EffectcraftApp>, from: Pos2, to: Pos2) -> Vec<Option<String>> {
    let m = Modifiers::SHIFT;
    h.event(Event::ModifiersChanged(m));
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: m });
    h.step();
    let mut hints = Vec::new();
    for i in 1..=12 {
        h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 12.)));
        h.step();
        h.step();
        hints.push(hint(h));
    }
    h.event(Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: m });
    h.step();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(3);
    hints
}

#[test]
fn layer_edge_snaps_to_collapsed_group_gap_edge() {
    let (mut h, [_, _, _, x], g) = harness();
    h.state_mut().session.execute("layer.groupUpdate", json!({"group":g,"collapsed":true})).unwrap();
    h.run_steps(3);
    let undo = h.state().session.history.undo.len();
    let from = rect(&h, &format!("timeline.layer.{x}.bar.out")).center();
    let to = pos2(x_at(&h, g, 3.0) - 4.0, from.y);
    let hints = shift_drag(&mut h, from, to);
    assert_eq!(hints.last().cloned().flatten().as_deref(), Some("Group: G"), "{hints:?}");
    let out = h.state().session.active_comp().unwrap().layer(effectcraft_engine::project::LayerId(x)).unwrap().out_point.seconds();
    assert!((out - 3.0).abs() < 1e-9, "out={out}");
    assert_eq!(h.state().session.history.undo.len(), undo + 1);
}

#[test]
fn key_snaps_to_collapsed_group_edge() {
    let (mut h, [_, _, _, x], g) = harness();
    h.state_mut().session.execute("prop.addKey", json!({"layer":x,"path":"transform/opacity","time":0.2,"value":40})).unwrap();
    h.state_mut().session.execute("layer.groupUpdate", json!({"group":g,"collapsed":true})).unwrap();
    h.state_mut().session.execute("layer.select", json!({"layers":[x]})).unwrap();
    h.run_steps(3);
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "timeline.reveal.opacity", json!({})).unwrap();
    h.run_steps(3);
    let uid = h.state().session.active_comp().unwrap().layer(effectcraft_engine::project::LayerId(x)).unwrap().props.prop("transform/opacity").unwrap().uid;
    let from = rect(&h, &format!("timeline.key.{uid}.6")).center();
    let to = pos2(x_at(&h, g, 4.0) + 3.0, from.y);
    let hints = shift_drag(&mut h, from, to);
    assert_eq!(hints.last().cloned().flatten().as_deref(), Some("Group: G"), "{hints:?}");
    let l = h.state().session.active_comp().unwrap().layer(effectcraft_engine::project::LayerId(x)).unwrap().clone();
    let key = l.comp_time(l.props.prop("transform/opacity").unwrap().keys[0].time).seconds();
    assert!((key - 4.0).abs() < 1e-9, "key={key}");
}

#[test]
fn moving_all_members_removes_group_targets() {
    let (mut h, [a, b, c, _], _) = harness();
    h.state_mut().session.execute("layer.select", json!({"layers":[a,b,c]})).unwrap();
    h.run_steps(3);
    let from = rect(&h, &format!("timeline.layer.{a}.bar")).center();
    let hints = shift_drag(&mut h, from, from + vec2(40.0, 0.0));
    assert!(hints.iter().all(|h| h.as_deref() != Some("Group: G")), "moving layers must not snap to their own folder: {hints:?}");
}
