//! Dragging an effect over the Timeline lights up the layer it would land on, not the whole panel.
use effectcraft_engine::Session;
use effectcraft_ui_egui::{EffectcraftApp, panels::DragPayload};
use egui::{Event, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn rect(h: &Harness<'_, EffectcraftApp>, id: &str) -> Rect {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}"));
    Rect::from_min_size(pos2(e.rect[0], e.rect[1]), vec2(e.rect[2], e.rect[3]))
}

fn drag_to(h: &mut Harness<'_, EffectcraftApp>, p: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    egui::DragAndDrop::set_payload(&h.ctx, DragPayload::Effect("Two-Point Bend".into()));
    h.run_steps(2);
}

#[test]
fn effect_drag_highlights_the_layer_under_the_pointer() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 640, "height": 360, "duration": 4})).unwrap();
    let a = s.execute("layer.newNull", json!({})).unwrap()["layer"].as_u64().unwrap();
    let b = s.execute("layer.newNull", json!({})).unwrap()["layer"].as_u64().unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    assert!(h.state().auto.find("timeline.dropTarget").is_none(), "nothing is lit without a drag");
    let (row_a, row_b) = (rect(&h, &format!("timeline.layer.{a}.row")), rect(&h, &format!("timeline.layer.{b}.row")));
    let panel = rect(&h, "timeline.layerMarquee");

    drag_to(&mut h, row_b.center());
    let lit = rect(&h, "timeline.dropTarget");
    assert!(lit.contains(row_b.center()) && !lit.contains(row_a.center()), "lit {lit:?} for row {row_b:?}");
    assert!(lit.height() < panel.height(), "only the layer is lit, not the whole timeline");

    drag_to(&mut h, row_a.center());
    let lit = rect(&h, "timeline.dropTarget");
    assert!(lit.contains(row_a.center()) && !lit.contains(row_b.center()), "the highlight follows the pointer: {lit:?}");
}

#[test]
fn collapsed_folder_is_not_an_effect_target_for_its_hidden_first_child() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name":"Folders","width":640,"height":360,"duration":4})).unwrap();
    let a = s.execute("layer.newNull", json!({})).unwrap()["layer"].as_u64().unwrap();
    let b = s.execute("layer.newNull", json!({})).unwrap()["layer"].as_u64().unwrap();
    let group = s.execute("layer.group", json!({"layers":[a,b]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group":group,"collapsed":true})).unwrap();
    let before = s.project.to_file_json().unwrap();
    let mut h = Harness::builder().with_size(vec2(1600., 1000.)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    let at = rect(&h, &format!("timeline.folder.{group}.row")).right_center() - vec2(10., 0.);
    drag_to(&mut h, at);
    assert!(h.state().auto.find("timeline.dropTarget").is_none(), "a folder header must not expose its hidden child as an effect target");
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
}
