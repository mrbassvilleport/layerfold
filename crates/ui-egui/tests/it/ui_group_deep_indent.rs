//! Deeply nested layer groups in a narrow Name column: a row's indent stops where its twirl,
//! icon, guide mark and name still fit inside the column (16 levels at 14 px push them past a narrow
//! column into the next ones).

use effectcraft_engine::Session;
use effectcraft_engine::project::LayerId;
use effectcraft_ui_egui::EffectcraftApp;
use egui::{Rect, pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn rect(h: &Harness<'_, EffectcraftApp>, id: &str) -> Rect {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}"));
    Rect::from_min_size(pos2(e.rect[0], e.rect[1]), vec2(e.rect[2], e.rect[3]))
}

#[test]
fn deep_nesting_stays_inside_the_name_column() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Deep", "width": 640, "height": 360, "duration": 4})).unwrap();
    let l = s.execute("layer.newSolid", json!({"name": "Leaf", "color": "#406080"})).unwrap()["layer"].as_u64().unwrap();
    // Each grouping of the lone layer wraps the groups it is already in: one level deeper.
    let groups: Vec<u64> =
        (0..16).map(|i| s.execute("layer.group", json!({"layers": [l], "name": format!("G{i}")})).unwrap()["group"].as_u64().unwrap()).collect();
    s.execute("layer.setSwitch", json!({"layers": [l], "switch": "guide", "value": true})).unwrap();
    s.state.selected_layers = vec![LayerId(l)];
    let mut h = Harness::builder().with_size(vec2(1100.0, 2000.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(4);
    let row = rect(&h, &format!("timeline.layer.{l}.row"));
    let mark = rect(&h, &format!("timeline.layer.{l}.guide"));
    assert!(mark.max.x <= row.max.x, "guide mark {mark:?} inside the Name column ending at {}", row.max.x);
    let twirl = rect(&h, &format!("timeline.layer.{l}.twirl"));
    assert!(twirl.max.x < mark.min.x, "the twirl stays before the mark: {twirl:?} vs {mark:?}");
    for g in &groups {
        let tw = rect(&h, &format!("timeline.folder.{g}.twirl"));
        assert!(tw.max.x <= row.max.x, "group {g} twirl {tw:?} inside the Name column ending at {}", row.max.x);
    }
    // The layer's properties nest one level under the layer's own (capped) indent.
    let transform = h.state().session.active_comp().unwrap().layer(LayerId(l)).unwrap().props.sub("transform").unwrap().uid;
    h.state_mut().ui.timeline.open_layers.insert(l);
    h.run_steps(3);
    let twirl = rect(&h, &format!("timeline.layer.{l}.twirl"));
    let tw = rect(&h, &format!("timeline.group.{transform}.twirl"));
    assert!(tw.min.x > twirl.min.x && tw.min.x < twirl.max.x + 20.0, "Transform {tw:?} one level under the layer {twirl:?}");
    assert!(tw.max.x <= row.max.x, "Transform twirl {tw:?} inside the Name column ending at {}", row.max.x);
}
