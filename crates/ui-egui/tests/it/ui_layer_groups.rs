use effectcraft_engine::Session;
use effectcraft_ui_egui::EffectcraftApp;
use egui::{Event, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> (Harness<'static, EffectcraftApp>, Vec<u64>) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"width":640,"height":360,"duration":6})).unwrap();
    for (name, x) in [("Left", 120), ("Right", 320), ("Other", 520)] {
        let id = s.execute("layer.newSolid", json!({"name":name,"color":"#406080","width":80,"height":80})).unwrap()["layer"].as_u64().unwrap();
        s.execute("prop.set", json!({"layer":id,"path":"transform/position","value":[x,180]})).unwrap();
        s.execute("prop.addKey", json!({"layer":id,"path":"transform/opacity","time":2,"value":50})).unwrap();
    }
    let ids = s.active_comp().unwrap().layers.iter().map(|l| l.id.0).collect::<Vec<_>>();
    s.execute("layer.select", json!({"layers":[ids[2],ids[1]]})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1900., 1000.)).with_step_dt(1.0 / 60.0).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    (h, ids)
}
fn rect(h: &Harness<'_, EffectcraftApp>, id: &str) -> Rect {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}"));
    Rect::from_min_size(pos2(e.rect[0], e.rect[1]), vec2(e.rect[2], e.rect[3]))
}
fn click(h: &mut Harness<'_, EffectcraftApp>, p: Pos2, modifiers: Modifiers) {
    h.event(Event::ModifiersChanged(modifiers));
    h.event(Event::PointerMoved(p));
    h.step();
    h.event(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers });
    h.step();
    h.event(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers });
    h.step();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(3);
}

fn drag(h: &mut Harness<'_, EffectcraftApp>, from: Pos2, to: Pos2, modifiers: Modifiers) {
    h.event(Event::ModifiersChanged(modifiers));
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers });
    h.step();
    for i in 1..=12 {
        h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 12.)));
        h.step();
    }
    h.event(Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers });
    h.step();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(3);
}

#[test]
fn shift_range_excludes_children_of_collapsed_folders() {
    for body in [false, true] {
        for nested in [false, true] {
            for reverse in [false, true] {
                let mut s = Session::default();
                s.execute("comp.new", json!({"name":"Range","width":320,"height":180,"duration":4})).unwrap();
                for name in ["D", "C", "B", "A"] {
                    s.execute("layer.newSolid", json!({"name":name,"color":"#406080"})).unwrap();
                }
                let ids: Vec<_> = s.active_comp().unwrap().layers.iter().map(|l| l.id.0).collect();
                let group = s.execute("layer.group", json!({"layers":[ids[1],ids[2]]})).unwrap()["group"].as_u64().unwrap();
                if nested {
                    s.execute("layer.group", json!({"layers":[ids[1],ids[2]]})).unwrap();
                }
                s.execute("layer.groupUpdate", json!({"group":group,"collapsed":true})).unwrap();
                s.execute("edit.deselectAll", json!({})).unwrap();
                let original = s.project.to_file_json().unwrap();
                let undo = s.history.undo.len();
                let mut h = Harness::builder().with_size(vec2(1900., 1000.)).with_step_dt(1.0 / 60.0).build_eframe(|_| EffectcraftApp::new(s));
                h.run_steps(3);
                let (anchor, end) = if reverse { (ids[3], ids[0]) } else { (ids[0], ids[3]) };
                let part = if body { "bar" } else { "row" };
                let p = rect(&h, &format!("timeline.layer.{anchor}.{part}")).center();
                click(&mut h, p, Modifiers::NONE);
                h.run_steps(40);
                let p = rect(&h, &format!("timeline.layer.{end}.{part}")).center();
                click(&mut h, p, Modifiers::SHIFT);
                assert_eq!(
                    h.state().session.state.selected_layers.iter().map(|l| l.0).collect::<Vec<_>>(),
                    vec![ids[0], ids[3]],
                    "body={body}, nested={nested}, reverse={reverse}"
                );
                assert_eq!(h.state().session.history.undo.len(), undo);
                assert_eq!(h.state().session.project.to_file_json().unwrap(), original);
            }
        }
    }
}

#[test]
fn alt_drag_still_moves_layers_in_timeline_and_viewer() {
    for viewer in [false, true] {
        let (mut h, ids) = harness();
        let active = h.state().session.active_layer();
        let selected = h.state().session.state.selected_layers.clone();
        let before = h.state().session.project.to_file_json().unwrap();
        let from = if viewer {
            let c = rect(&h, "viewer.comp");
            c.min + vec2(c.width() * 130. / 640., c.height() * 190. / 360.)
        } else {
            rect(&h, &format!("timeline.layer.{}.bar", ids[1])).center()
        };
        drag(&mut h, from, from + vec2(40., 0.), Modifiers { alt: true, ..Default::default() });
        assert_eq!(h.state().session.active_layer(), active, "viewer={viewer}");
        assert_eq!(h.state().session.state.selected_layers, selected);
        assert_ne!(h.state().session.project.to_file_json().unwrap(), before, "drag must still work; viewer={viewer}");
    }
}

#[test]
fn fresh_viewport_marquee_chooses_topmost_and_folder_click_skips_locked_children() {
    let (mut h, ids) = harness();
    let c = rect(&h, "viewer.comp");
    let screen = |x: f32, y: f32| c.min + vec2(c.width() * x / 640., c.height() * y / 360.);
    drag(&mut h, screen(50., 100.), screen(390., 260.), Modifiers::NONE);
    assert_eq!(h.state().session.active_layer().unwrap().0, ids[1]);
    assert_eq!(h.state().session.state.selected_layers.len(), 2);
    let group = h.state_mut().session.execute("layer.group", json!({})).unwrap()["group"].as_u64().unwrap();
    h.state_mut().session.execute("layer.setSwitch", json!({"layers":[ids[1]],"switch":"lock","value":true})).unwrap();
    h.state_mut().session.execute("edit.deselectAll", json!({})).unwrap();
    h.run_steps(3);
    let row = rect(&h, &format!("timeline.folder.{group}.row"));
    click(&mut h, row.right_center() - vec2(10., 0.), Modifiers::NONE);
    assert_eq!(h.state().session.state.selected_layers.iter().map(|l| l.0).collect::<Vec<_>>(), vec![ids[2]]);
}

#[test]
fn alt_click_row_bar_and_viewport_changes_only_active_layer() {
    let (mut h, ids) = harness();
    let selected = h.state().session.state.selected_layers.clone();
    let before = h.state().session.project.to_file_json().unwrap();
    let undo = h.state().session.history.undo.len();
    let alt = Modifiers { alt: true, ..Default::default() };
    for target in [format!("timeline.layer.{}.row", ids[1]), format!("timeline.layer.{}.bar", ids[2])] {
        let p = rect(&h, &target).center();
        click(&mut h, p, alt);
        let expected = if target.contains(&format!(".{}.", ids[2])) { ids[2] } else { ids[1] };
        assert_eq!(h.state().session.active_layer().unwrap().0, expected, "{target}");
        assert_eq!(h.state().session.state.selected_layers, selected);
        assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
        assert_eq!(h.state().session.history.undo.len(), undo);
    }
    let comp = rect(&h, "viewer.comp");
    let p = comp.min + vec2(comp.width() * 120. / 640., comp.height() * 180. / 360.);
    click(&mut h, p, alt);
    assert_eq!(h.state().session.active_layer().unwrap().0, ids[2]);
    assert!(h.state().auto.find(&format!("viewer.active.{}", ids[2])).is_some());
    let unselected = rect(&h, &format!("timeline.layer.{}.bar", ids[0])).center();
    click(&mut h, unselected, alt);
    let p = comp.min + vec2(comp.width() * 520. / 640., comp.height() * 180. / 360.);
    click(&mut h, p, alt);
    assert_eq!(h.state().session.state.selected_layers, selected);
    assert_eq!(h.state().session.active_layer().unwrap().0, ids[2]);
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
}

/// A selected layer hidden by its own or its folder's eye keeps its viewer box (#1769), so an
/// Alt-click on it in the viewer makes it active, as in the Timeline; a locked one does not.
#[test]
fn alt_click_in_the_viewer_activates_a_hidden_selected_layer() {
    let alt = Modifiers { alt: true, ..Default::default() };
    for case in ["own Video", "folder Video", "own Lock"] {
        let (mut h, ids) = harness();
        let right = ids[1];
        let s = &mut h.state_mut().session;
        match case {
            "own Video" => s.execute("layer.setSwitch", json!({"layers":[right],"switch":"video","value":false})).map(|_| ()),
            "own Lock" => s.execute("layer.setSwitch", json!({"layers":[right],"switch":"lock","value":true})).map(|_| ()),
            _ => {
                let g = s.execute("layer.group", json!({"layers":[right]})).unwrap()["group"].as_u64().unwrap();
                s.execute("layer.groupUpdate", json!({"group":g,"video":false})).map(|_| ())
            }
        }
        .unwrap();
        s.execute("layer.select", json!({"layers":[ids[2],right],"active":ids[2]})).unwrap();
        h.run_steps(3);
        let comp = rect(&h, "viewer.comp");
        click(&mut h, comp.min + vec2(comp.width() * 320. / 640., comp.height() * 180. / 360.), alt);
        let expected = if case == "own Lock" { ids[2] } else { right };
        assert_eq!(h.state().session.active_layer().unwrap().0, expected, "{case}");
    }
}

#[test]
fn collapse_hides_children_and_visible_keys_and_reopens_without_losing_them() {
    let (mut h, ids) = harness();
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "timeline.reveal.opacity", json!({})).unwrap();
    let group = h.state_mut().session.execute("layer.group", json!({"name":"Actors"})).unwrap()["group"].as_u64().unwrap();
    h.run_steps(3);
    let child = rect(&h, &format!("timeline.layer.{}.row", ids[1]));
    assert!(!h.state().auto.query("timeline.key.").is_empty());
    let before = h.state().session.active_comp().unwrap().layers.clone();
    let tw = rect(&h, &format!("timeline.folder.{group}.twirl")).center();
    click(&mut h, tw, Modifiers::NONE);
    assert!(h.state().session.active_comp().unwrap().layer_groups[0].collapsed);
    assert!(h.state().auto.find(&format!("timeline.layer.{}.row", ids[1])).is_none());
    assert!(h.state().auto.query("timeline.key.").is_empty());
    assert_eq!(h.state().session.active_comp().unwrap().layers, before);
    let tw = rect(&h, &format!("timeline.folder.{group}.twirl")).center();
    click(&mut h, tw, Modifiers::NONE);
    assert!(!h.state().session.active_comp().unwrap().layer_groups[0].collapsed);
    assert_eq!(rect(&h, &format!("timeline.layer.{}.row", ids[1])).min.y, child.min.y);
}

#[test]
fn group_name_can_be_edited_after_a_double_click() {
    let (mut h, _) = harness();
    let group = h.state_mut().session.execute("layer.group", json!({"name":"Actors"})).unwrap()["group"].as_u64().unwrap();
    h.run_steps(3);
    let at = rect(&h, &format!("timeline.folder.{group}.row")).center();
    click(&mut h, at, Modifiers::NONE);
    click(&mut h, at, Modifiers::NONE);
    assert!(h.state().auto.find(&format!("timeline.folder.{group}.nameEdit")).is_some());
    h.event(Event::Text("Renamed".into()));
    h.step();
    h.event(Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::Key { key: egui::Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert_eq!(h.state().session.active_comp().unwrap().layer_groups[0].name, "Renamed");
}

fn stack(h: &Harness<'_, EffectcraftApp>) -> Vec<u64> {
    h.state().session.active_comp().unwrap().layers.iter().map(|l| l.id.0).collect()
}

#[test]
fn dragging_layer_into_collapsed_header_and_back_out_is_one_undo_each() {
    let (mut h, ids) = harness();
    let group = h.state_mut().session.execute("layer.group", json!({})).unwrap()["group"].as_u64().unwrap();
    h.state_mut().session.execute("layer.groupUpdate", json!({"group":group,"collapsed":true})).unwrap();
    h.run_steps(3);
    let before = h.state().session.project.to_file_json().unwrap();
    let undo = h.state().session.history.undo.len();
    let from = rect(&h, &format!("timeline.layer.{}.row", ids[0])).center();
    let to = rect(&h, &format!("timeline.folder.{group}.row")).center();
    drag(&mut h, from, to, Modifiers::NONE);
    assert_eq!(stack(&h), [ids[1], ids[2], ids[0]]);
    let g = &h.state().session.active_comp().unwrap().layer_groups[0];
    assert!(g.collapsed);
    assert_eq!(g.members.len(), 3);
    assert!(h.state().auto.find(&format!("timeline.layer.{}.row", ids[0])).is_none());
    assert_eq!(h.state().session.history.undo.len(), undo + 1);
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
    h.state_mut().session.execute("edit.redo", json!({})).unwrap();
    h.state_mut().session.execute("layer.groupUpdate", json!({"group":group,"collapsed":false})).unwrap();
    h.state_mut().session.execute("layer.select", json!({"layers":[ids[0]]})).unwrap();
    h.run_steps(3);
    let from = rect(&h, &format!("timeline.layer.{}.row", ids[0])).center();
    let header = rect(&h, &format!("timeline.folder.{group}.row"));
    let undo = h.state().session.history.undo.len();
    drag(&mut h, from, pos2(header.center().x, header.min.y + 1.), Modifiers::NONE);
    assert_eq!(stack(&h), [ids[0], ids[1], ids[2]]);
    assert_eq!(h.state().session.active_comp().unwrap().layer_groups[0].members.len(), 2);
    assert_eq!(h.state().session.history.undo.len(), undo + 1);
}

#[test]
fn dragging_group_header_moves_hidden_members_and_preserves_fold_state() {
    let (mut h, ids) = harness();
    let group = h.state_mut().session.execute("layer.group", json!({})).unwrap()["group"].as_u64().unwrap();
    let other = h.state_mut().session.execute("layer.group", json!({"layers":[ids[0]]})).unwrap()["group"].as_u64().unwrap();
    for id in [group, other] {
        h.state_mut().session.execute("layer.groupUpdate", json!({"group":id,"collapsed":true})).unwrap();
    }
    h.run_steps(3);
    let before = h.state().session.project.to_file_json().unwrap();
    let undo = h.state().session.history.undo.len();
    let from = rect(&h, &format!("timeline.folder.{group}.row")).center();
    let target = rect(&h, &format!("timeline.folder.{other}.row"));
    drag(&mut h, from, pos2(target.center().x, target.min.y + 1.), Modifiers::NONE);
    assert_eq!(stack(&h), [ids[1], ids[2], ids[0]]);
    let c = h.state().session.active_comp().unwrap();
    assert_eq!(c.layer_groups.len(), 2);
    assert!(c.layer_groups.iter().all(|g| g.collapsed));
    assert_eq!(h.state().session.history.undo.len(), undo + 1);
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
    h.run_steps(3);
    let from = rect(&h, &format!("timeline.folder.{other}.row")).center();
    let target = rect(&h, &format!("timeline.folder.{group}.row"));
    drag(&mut h, from, pos2(target.center().x, target.max.y - 1.), Modifiers::NONE);
    assert_eq!(stack(&h), [ids[1], ids[2], ids[0]]);
}

#[test]
fn multi_layer_drop_joins_folder_and_child_drop_uses_indented_position() {
    let (mut h, ids) = harness();
    let group = h.state_mut().session.execute("layer.group", json!({"layers":[ids[0]]})).unwrap()["group"].as_u64().unwrap();
    h.run_steps(3);
    let undo = h.state().session.history.undo.len();
    let from = rect(&h, &format!("timeline.layer.{}.row", ids[1])).center();
    let to = rect(&h, &format!("timeline.folder.{group}.row")).center();
    drag(&mut h, from, to, Modifiers::NONE);
    assert_eq!(stack(&h), ids);
    assert_eq!(h.state().session.active_comp().unwrap().layer_groups[0].members.len(), 3);
    assert_eq!(h.state().session.history.undo.len(), undo + 1);
    h.state_mut().session.execute("layer.select", json!({"layers":[ids[2]]})).unwrap();
    h.run_steps(3);
    let from = rect(&h, &format!("timeline.layer.{}.row", ids[2])).center();
    let target = rect(&h, &format!("timeline.layer.{}.row", ids[1]));
    drag(&mut h, from, pos2(target.max.x - 6., target.min.y + 1.), Modifiers::NONE);
    assert_eq!(stack(&h), [ids[0], ids[2], ids[1]]);
    assert_eq!(h.state().session.active_comp().unwrap().layer_groups[0].members.len(), 3);
    assert!(h.state().session.active_comp().unwrap().layer_groups_are_valid());
}

#[test]
fn release_outside_outline_and_drag_of_locked_folder_leave_document_unchanged() {
    let (mut h, ids) = harness();
    let group = h.state_mut().session.execute("layer.group", json!({})).unwrap()["group"].as_u64().unwrap();
    h.run_steps(3);
    let before = h.state().session.project.to_file_json().unwrap();
    let from = rect(&h, &format!("timeline.layer.{}.row", ids[0])).center();
    let outside = rect(&h, "viewer.comp").center();
    drag(&mut h, from, outside, Modifiers::NONE);
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
    h.state_mut().session.execute("layer.setSwitch", json!({"layers":[ids[1]],"switch":"lock","value":true})).unwrap();
    h.run_steps(3);
    let before = h.state().session.project.to_file_json().unwrap();
    let undo = h.state().session.history.undo.len();
    let from = rect(&h, &format!("timeline.folder.{group}.row")).center();
    let target = rect(&h, &format!("timeline.layer.{}.row", ids[0]));
    drag(&mut h, from, pos2(target.center().x, target.min.y + 1.), Modifiers::NONE);
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
    assert_eq!(h.state().session.history.undo.len(), undo);
}

#[test]
fn held_drag_shows_target_feedback_and_escape_cancels_it() {
    let (mut h, ids) = harness();
    let group = h.state_mut().session.execute("layer.group", json!({})).unwrap()["group"].as_u64().unwrap();
    h.run_steps(3);
    let before = h.state().session.project.to_file_json().unwrap();
    let undo = h.state().session.history.undo.len();
    let from = rect(&h, &format!("timeline.layer.{}.row", ids[0])).center();
    let target = rect(&h, &format!("timeline.folder.{group}.row"));
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerMoved(target.center()));
    h.run_steps(3);
    assert!(h.state().auto.find("timeline.drop.into").is_some());
    let edge = pos2(target.center().x, target.min.y + 1.);
    h.event(Event::PointerMoved(edge));
    h.run_steps(3);
    assert!(h.state().auto.find("timeline.drop.before").is_some());
    h.event(Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::Key { key: egui::Key::Escape, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerButton { pos: edge, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
    assert_eq!(h.state().session.history.undo.len(), undo);
    assert!(h.state().auto.find("timeline.drop.before").is_none());
}

#[test]
fn held_stack_drag_scrolls_at_panel_edge_without_editing_until_release() {
    let (mut h, _) = harness();
    for i in 0..35 {
        h.state_mut().session.execute("layer.newSolid", json!({"name":format!("Extra {i}")})).unwrap();
    }
    h.run_steps(3);
    let id = h.state().session.active_comp().unwrap().layers[0].id.0;
    let from = rect(&h, &format!("timeline.layer.{id}.row")).center();
    let outline = rect(&h, "timeline.layerMarquee");
    // Past the bottom of the layer names: inside them the pointer only picks the drop boundary.
    let edge = pos2(from.x, outline.max.y + 6.);
    let before = h.state().session.project.to_file_json().unwrap();
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerMoved(edge));
    h.run_steps(30);
    assert!(h.state().ui.timeline.scroll_y > 60.);
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
    let outside = rect(&h, "viewer.comp").center();
    h.event(Event::PointerMoved(outside));
    h.step();
    h.event(Event::PointerButton { pos: outside, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
}
#[test]
fn clicking_a_fully_locked_folder_preserves_selection() {
    for inherited in [false, true] {
        for modifiers in [Modifiers::NONE, Modifiers::SHIFT, Modifiers::COMMAND] {
            let (mut h, ids) = harness();
            let group = h.state_mut().session.execute("layer.group", json!({"layers":[ids[1],ids[2]]})).unwrap()["group"].as_u64().unwrap();
            if inherited {
                h.state_mut().session.execute("layer.groupUpdate", json!({"group":group,"locked":true})).unwrap();
            } else {
                h.state_mut().session.execute("layer.setSwitch", json!({"layers":[ids[1],ids[2]],"switch":"lock","value":true})).unwrap();
            }
            h.state_mut().session.execute("layer.select", json!({"layers":[ids[0]]})).unwrap();
            let before = h.state().session.project.to_file_json().unwrap();
            let undo = h.state().session.history.undo.len();
            h.run_steps(3);
            let p = rect(&h, &format!("timeline.folder.{group}.row")).right_center() - vec2(10., 0.);
            click(&mut h, p, modifiers);
            assert_eq!(
                h.state().session.state.selected_layers.iter().map(|l| l.0).collect::<Vec<_>>(),
                vec![ids[0]],
                "inherited={inherited}, modifiers={modifiers:?}"
            );
            assert_eq!(h.state().session.active_layer().unwrap().0, ids[0]);
            assert_eq!(h.state().session.history.undo.len(), undo);
            assert_eq!(h.state().session.project.to_file_json().unwrap(), before);
        }
    }
}
