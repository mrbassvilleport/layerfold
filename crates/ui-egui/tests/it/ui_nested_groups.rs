use effectcraft_engine::Session;
use effectcraft_ui_egui::EffectcraftApp;
use egui::{Event, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

struct Scene {
    h: Harness<'static, EffectcraftApp>,
    layers: Vec<u64>,
    outer: u64,
    inner: u64,
    deep: u64,
}

fn scene(nested: bool) -> Scene {
    let mut s = Session::default();
    s.execute("comp.new", json!({"width":640,"height":360,"duration":6})).unwrap();
    for (name, x) in [("One", 120), ("Two", 200), ("Three", 280), ("Four", 360), ("Five", 440), ("Six", 520)] {
        let id = s.execute("layer.newSolid", json!({"name":name,"color":"#406080","width":60,"height":60})).unwrap()["layer"].as_u64().unwrap();
        s.execute("prop.set", json!({"layer":id,"path":"transform/position","value":[x,180]})).unwrap();
        s.execute("prop.addKey", json!({"layer":id,"path":"transform/opacity","time":2,"value":50})).unwrap();
    }
    let layers: Vec<_> = s.active_comp().unwrap().layers.iter().map(|l| l.id.0).collect();
    let deep = s.execute("layer.group", json!({"layers":[layers[0]],"name":"Deep"})).unwrap()["group"].as_u64().unwrap();
    let inner = s.execute("layer.group", json!({"layers":[layers[0],layers[1]],"name":"Inner"})).unwrap()["group"].as_u64().unwrap();
    let outer = s.execute("layer.group", json!({"layers":[layers[2]],"name":"Outer"})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group":deep,"collapsed":true,"video":false})).unwrap();
    s.execute("layer.groupUpdate", json!({"group":inner,"audio":false})).unwrap();
    s.execute("layer.setSwitch", json!({"layers":[layers[0]],"switch":"video","value":false})).unwrap();
    s.execute("layer.setSwitch", json!({"layers":[layers[1]],"switch":"audio","value":false})).unwrap();
    if nested {
        s.execute("layer.drop", json!({"group":inner,"into":outer})).unwrap();
    }
    s.execute("layer.select", json!({"layers":[layers[1]]})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1900., 1000.)).with_step_dt(1.0 / 60.0).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    Scene { h, layers, outer, inner, deep }
}

fn rect(h: &Harness<'_, EffectcraftApp>, id: &str) -> Rect {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}"));
    Rect::from_min_size(pos2(e.rect[0], e.rect[1]), vec2(e.rect[2], e.rect[3]))
}

fn click(h: &mut Harness<'_, EffectcraftApp>, at: Pos2) {
    h.event(Event::PointerMoved(at));
    h.step();
    h.event(Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(4);
}

fn drag(h: &mut Harness<'_, EffectcraftApp>, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    for i in 1..=12 {
        h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 12.)));
        h.step();
    }
    h.event(Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(4);
}

fn parent(h: &Harness<'_, EffectcraftApp>, group: u64) -> Option<u64> {
    h.state().session.active_comp().unwrap().layer_groups.iter().find(|g| g.id == group).unwrap().parent
}

fn folder_flags(h: &Harness<'_, EffectcraftApp>) -> Vec<(u64, bool, bool, bool, bool)> {
    h.state().session.active_comp().unwrap().layer_groups.iter().map(|g| (g.id, g.collapsed, g.video, g.audio, g.locked)).collect()
}

#[test]
fn master_header_switches_preserve_children_and_each_undo_restores_exact_json() {
    let Scene { mut h, outer, .. } = scene(true);
    for key in ["video", "audio", "locked"] {
        let before = h.state().session.project.to_file_json().unwrap();
        let undo = h.state().session.history.undo.len();
        let layers = h.state().session.active_comp().unwrap().layers.clone();
        let children: Vec<_> = h.state().session.active_comp().unwrap().layer_groups.iter().filter(|g| g.id != outer).cloned().collect();
        let at = rect(&h, &format!("timeline.folder.{outer}.{key}")).center();
        click(&mut h, at);
        let c = h.state().session.active_comp().unwrap();
        let group = c.layer_groups.iter().find(|g| g.id == outer).unwrap();
        match key {
            "video" => assert!(!group.video),
            "audio" => assert!(!group.audio),
            "locked" => assert!(group.locked),
            _ => unreachable!(),
        }
        assert_eq!(c.layers, layers, "{key} must retain every leaf's raw switches");
        assert_eq!(c.layer_groups.iter().filter(|g| g.id != outer).cloned().collect::<Vec<_>>(), children);
        assert_eq!(h.state().session.history.undo.len(), undo + 1, "{key}");
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.project.to_file_json().unwrap(), before, "{key}");
        h.run_steps(3);
    }
}

#[test]
fn nested_header_drop_into_center_and_back_out_preserves_subtree_and_one_undo() {
    let Scene { mut h, layers, outer, inner, deep } = scene(false);
    let leaves = h.state().session.active_comp().unwrap().layers.clone();
    let flags = folder_flags(&h);
    let before = h.state().session.project.to_file_json().unwrap();
    let undo = h.state().session.history.undo.len();
    let from = rect(&h, &format!("timeline.folder.{inner}.row")).center();
    let to = rect(&h, &format!("timeline.folder.{outer}.row")).center();
    drag(&mut h, from, to);
    assert_eq!(parent(&h, inner), Some(outer));
    assert_eq!(parent(&h, deep), Some(inner));
    assert_eq!(folder_flags(&h), flags);
    let c = h.state().session.active_comp().unwrap();
    for leaf in &leaves {
        assert_eq!(c.layer(leaf.id).unwrap(), leaf);
    }
    assert!(c.layer_groups_are_valid());
    assert_eq!(h.state().session.history.undo.len(), undo + 1);
    let nested = h.state().session.project.to_file_json().unwrap();
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
    h.state_mut().session.execute("edit.redo", json!({})).unwrap();
    assert_eq!(h.state().session.project.to_file_json().unwrap(), nested);
    h.run_steps(3);
    let undo = h.state().session.history.undo.len();
    let from = rect(&h, &format!("timeline.folder.{inner}.row")).center();
    let target = rect(&h, &format!("timeline.folder.{outer}.row"));
    drag(&mut h, from, pos2(target.center().x, target.min.y + 1.));
    assert_eq!(parent(&h, inner), None);
    assert_eq!(parent(&h, deep), Some(inner));
    assert_eq!(folder_flags(&h), flags);
    let c = h.state().session.active_comp().unwrap();
    assert_eq!(c.layer_groups.iter().find(|g| g.id == outer).unwrap().members.iter().map(|id| id.0).collect::<Vec<_>>(), [layers[2]]);
    assert!(c.layer_groups_are_valid());
    assert_eq!(h.state().session.history.undo.len(), undo + 1);
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(h.state().session.project.to_file_json().unwrap(), nested);
}

#[test]
fn collapsed_ancestor_hides_nested_headers_leaves_keys_and_keeps_inner_fold_state() {
    let Scene { mut h, layers, outer, inner, deep } = scene(true);
    h.state_mut().session.execute("layer.select", json!({"layers":[layers[0],layers[1],layers[2]]})).unwrap();
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "timeline.reveal.opacity", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.state().auto.find(&format!("timeline.folder.{inner}.row")).is_some());
    assert!(h.state().auto.find(&format!("timeline.folder.{deep}.row")).is_some());
    assert!(!h.state().auto.query("timeline.key.").is_empty());
    let layers_before = h.state().session.active_comp().unwrap().layers.clone();
    let at = rect(&h, &format!("timeline.folder.{outer}.twirl")).center();
    click(&mut h, at);
    for group in [inner, deep] {
        assert!(h.state().auto.find(&format!("timeline.folder.{group}.row")).is_none());
    }
    for layer in &layers[..3] {
        assert!(h.state().auto.find(&format!("timeline.layer.{layer}.row")).is_none());
    }
    assert!(h.state().auto.query("timeline.key.").is_empty());
    let at = rect(&h, &format!("timeline.folder.{outer}.twirl")).center();
    click(&mut h, at);
    assert!(h.state().auto.find(&format!("timeline.folder.{inner}.row")).is_some());
    let at = rect(&h, &format!("timeline.folder.{inner}.twirl")).center();
    click(&mut h, at);
    for _ in 0..2 {
        let at = rect(&h, &format!("timeline.folder.{outer}.twirl")).center();
        click(&mut h, at);
    }
    let c = h.state().session.active_comp().unwrap();
    assert!(!c.layer_groups.iter().find(|g| g.id == outer).unwrap().collapsed);
    assert!(c.layer_groups.iter().find(|g| g.id == inner).unwrap().collapsed);
    assert!(c.layer_groups.iter().find(|g| g.id == deep).unwrap().collapsed);
    assert!(h.state().auto.find(&format!("timeline.folder.{inner}.row")).is_some());
    assert!(h.state().auto.find(&format!("timeline.folder.{deep}.row")).is_none());
    assert!(h.state().auto.find(&format!("timeline.layer.{}.row", layers[1])).is_none());
    assert_eq!(c.layers, layers_before);
}

#[test]
fn group_drop_into_descendant_is_a_no_op() {
    let Scene { mut h, outer, deep, .. } = scene(true);
    let before = h.state().session.project.to_file_json().unwrap();
    let undo = h.state().session.history.undo.len();
    let from = rect(&h, &format!("timeline.folder.{outer}.row")).center();
    let to = rect(&h, &format!("timeline.folder.{deep}.row")).center();
    drag(&mut h, from, to);
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
    assert_eq!(h.state().session.history.undo.len(), undo);
}

#[test]
fn rejected_locked_drag_does_not_cancel_the_next_unlocked_folder_drag() {
    for header in [true, false] {
        let Scene { mut h, layers, outer, inner, deep } = scene(true);
        let before = h.state().session.project.to_file_json().unwrap();
        let flags = folder_flags(&h);
        h.state_mut().session.execute("layer.groupUpdate", json!({"group":outer,"locked":true})).unwrap();
        h.run_steps(3);
        let locked = h.state().session.project.to_file_json().unwrap();
        let undo = h.state().session.history.undo.len();
        let source = if header { format!("timeline.folder.{inner}.row") } else { format!("timeline.layer.{}.row", layers[1]) };
        let from = rect(&h, &source).center();
        let target = rect(&h, &format!("timeline.layer.{}.row", layers[5]));
        drag(&mut h, from, pos2(target.center().x, target.min.y + 1.));
        assert_eq!(h.state().session.project.to_file_json().unwrap(), locked);
        assert_eq!(h.state().session.history.undo.len(), undo);
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        h.run_steps(3);
        assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
        let undo = h.state().session.history.undo.len();
        let from = rect(&h, &format!("timeline.folder.{inner}.row")).center();
        let target = rect(&h, &format!("timeline.folder.{outer}.row"));
        drag(&mut h, from, pos2(target.center().x, target.min.y + 1.));
        assert_eq!(parent(&h, inner), None, "after rejected source {source}");
        assert_eq!(parent(&h, deep), Some(inner));
        assert_eq!(folder_flags(&h), flags);
        assert_eq!(h.state().session.history.undo.len(), undo + 1);
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
    }
}

#[test]
fn locked_ancestor_prevents_layer_row_folder_and_viewport_drags() {
    let Scene { mut h, layers, outer, inner, .. } = scene(true);
    h.state_mut().session.execute("layer.groupUpdate", json!({"group":outer,"locked":true})).unwrap();
    h.run_steps(3);
    let before = h.state().session.project.to_file_json().unwrap();
    let undo = h.state().session.history.undo.len();
    let target = rect(&h, &format!("timeline.layer.{}.row", layers[5]));
    for source in [format!("timeline.layer.{}.row", layers[1]), format!("timeline.folder.{inner}.row")] {
        let from = rect(&h, &source).center();
        drag(&mut h, from, pos2(target.center().x, target.min.y + 1.));
        assert_eq!(h.state().session.project.to_file_json().unwrap(), before, "{source}");
        assert_eq!(h.state().session.history.undo.len(), undo);
    }
    let viewer = rect(&h, "viewer.comp");
    let from = viewer.min + vec2(viewer.width() * 440. / 640., viewer.height() * 180. / 360.);
    drag(&mut h, from, from + vec2(40., 0.));
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
    assert_eq!(h.state().session.history.undo.len(), undo);
}
