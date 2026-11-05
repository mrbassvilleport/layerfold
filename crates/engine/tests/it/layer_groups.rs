use effectcraft_engine::{Session, project::LayerId};
use serde_json::{Value, json};

fn setup() -> (Session, Vec<u64>) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name":"Groups","width":64,"height":64,"duration":6})).unwrap();
    for name in ["E", "D", "C", "B", "A"] {
        s.execute("layer.newSolid", json!({"name":name,"color":"#406080","width":16,"height":16})).unwrap();
    }
    let ids = s.active_comp().unwrap().layers.iter().map(|l| l.id.0).collect();
    s.execute("edit.deselectAll", json!({})).unwrap();
    (s, ids)
}
fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, params).unwrap()
}
fn order(s: &Session) -> Vec<String> {
    s.active_comp().unwrap().layers.iter().map(|l| l.name.clone()).collect()
}

#[test]
fn gathers_at_active_preserves_timing_and_order_with_one_undo_and_roundtrip() {
    let (mut s, ids) = setup();
    run(&mut s, "layer.select", json!({"layers":[ids[2],ids[0],ids[4]]}));
    assert_eq!(s.active_layer(), Some(LayerId(ids[2])));
    run(&mut s, "layer.timing", json!({"layers":[ids[4]],"in":1,"out":4}));
    run(&mut s, "prop.addKey", json!({"layer":ids[0],"path":"transform/opacity","time":2,"value":70}));
    let before = s.project.to_file_json().unwrap();
    let layers = s.active_comp().unwrap().layers.clone();
    let undo = s.history.undo.len();
    let group = run(&mut s, "layer.group", json!({"name":"Characters"}))["group"].as_u64().unwrap();
    assert_eq!(order(&s), ["B", "A", "C", "E", "D"]);
    let c = s.active_comp().unwrap();
    assert!(c.layer_groups_are_valid());
    assert_eq!(c.layer_groups[0].members, vec![LayerId(ids[0]), LayerId(ids[2]), LayerId(ids[4])]);
    for l in &layers {
        assert_eq!(c.layer(l.id), Some(l));
    }
    assert_eq!(s.history.undo.len(), undo + 1);
    let after = s.project.to_file_json().unwrap();
    let loaded = effectcraft_engine::project::Project::from_json(&after).unwrap();
    assert_eq!(loaded.comp(s.active_comp_id().unwrap()).unwrap().layer_groups, c.layer_groups);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project.to_file_json().unwrap(), before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.project.to_file_json().unwrap(), after);
    run(&mut s, "layer.groupUpdate", json!({"group":group,"name":"New name","collapsed":true}));
    assert!(s.active_comp().unwrap().layer_groups[0].collapsed);
    let before_ungroup = order(&s);
    run(&mut s, "layer.ungroup", json!({"group":group}));
    assert!(s.active_comp().unwrap().layer_groups.is_empty());
    assert_eq!(order(&s), before_ungroup);
}

#[test]
fn active_selection_is_independent_of_document_and_undo() {
    let (mut s, ids) = setup();
    run(&mut s, "layer.select", json!({"layers":[ids[4]]}));
    run(&mut s, "layer.select", json!({"layers":[ids[0],ids[2]],"add":true}));
    assert_eq!(s.active_layer(), Some(LayerId(ids[4])));
    let before = s.project.to_file_json().unwrap();
    let selected = s.state.selected_layers.clone();
    let undo = s.history.undo.len();
    run(&mut s, "layer.setActive", json!({"layer":ids[2]}));
    assert_eq!(s.state.selected_layers, selected);
    assert_eq!(s.active_layer(), Some(LayerId(ids[2])));
    assert_eq!(s.history.undo.len(), undo);
    assert_eq!(s.project.to_file_json().unwrap(), before);
    assert!(s.execute("layer.setActive", json!({"layer":ids[1]})).is_err());
    run(&mut s, "layer.select", json!({"layers":[ids[2]],"toggle":true}));
    assert_eq!(s.active_layer(), Some(LayerId(ids[0])));
    run(&mut s, "layer.select", json!({"layers":[ids[4],ids[0]],"active":ids[0]}));
    assert_eq!(s.active_layer(), Some(LayerId(ids[0])));
}

#[test]
fn adding_an_existing_descendant_to_ancestor_preserves_subgroup_without_undo() {
    let (mut s, ids) = setup();
    let inner = run(&mut s, "layer.group", json!({"layers":[ids[0]]}))["group"].as_u64().unwrap();
    let outer = run(&mut s, "layer.group", json!({"layers":[ids[0],ids[1]]}))["group"].as_u64().unwrap();
    let before = s.project.to_file_json().unwrap();
    let undo = s.history.undo.len();
    run(&mut s, "layer.groupAdd", json!({"group":outer,"layers":[ids[0]]}));
    assert_eq!(s.project.to_file_json().unwrap(), before);
    assert_eq!(s.history.undo.len(), undo);
    assert_eq!(s.active_comp().unwrap().layer_group(LayerId(ids[0])).unwrap().id, inner);
    run(&mut s, "layer.groupAdd", json!({"group":outer,"layers":[ids[0],ids[4]]}));
    assert_eq!(s.active_comp().unwrap().layer_group(LayerId(ids[0])).unwrap().id, inner);
    assert_eq!(s.active_comp().unwrap().layer_group(LayerId(ids[4])).unwrap().id, outer);
    assert!(s.active_comp().unwrap().layer_groups_are_valid());
    assert_eq!(s.history.undo.len(), undo + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project.to_file_json().unwrap(), before);
}

#[test]
fn group_add_remove_delete_and_arrange_keep_valid_membership() {
    let (mut s, ids) = setup();
    run(&mut s, "layer.select", json!({"layers":[ids[1],ids[2]]}));
    let g = run(&mut s, "layer.group", json!({}))["group"].as_u64().unwrap();
    run(&mut s, "layer.groupAdd", json!({"group":g,"layers":[ids[4]]}));
    assert_eq!(order(&s), ["A", "B", "C", "E", "D"]);
    run(&mut s, "layer.groupRemove", json!({"layers":[ids[1],ids[2],ids[4]]}));
    assert!(s.active_comp().unwrap().layer_groups.is_empty());
    assert_eq!(order(&s), ["A", "B", "C", "E", "D"], "removing all members only removes the folder");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "layer.groupRemove", json!({"layers":[ids[1]]}));
    assert_eq!(order(&s), ["A", "C", "E", "B", "D"]);
    assert!(s.active_comp().unwrap().layer_groups_are_valid());
    run(&mut s, "layer.arrange", json!({"layers":[ids[2],ids[4]],"to":"backward"}));
    assert_eq!(order(&s), ["A", "B", "C", "E", "D"]);
    assert_eq!(s.active_comp().unwrap().layer_groups[0].members.len(), 2);
    run(&mut s, "edit.clear", json!({"layers":[ids[2]]}));
    assert!(s.active_comp().unwrap().layer_groups_are_valid());
    assert_eq!(s.active_comp().unwrap().layer_groups[0].members, vec![LayerId(ids[4])]);
}

#[test]
fn no_op_arrange_keeps_members_and_duplicate_comp_gets_new_group_ids() {
    let (mut s, ids) = setup();
    run(&mut s, "layer.select", json!({"layers":[ids[0],ids[1],ids[2]]}));
    let group = run(&mut s, "layer.group", json!({}))["group"].as_u64().unwrap();
    let before = s.project.to_file_json().unwrap();
    let undo = s.history.undo.len();
    run(&mut s, "layer.arrange", json!({"layers":[ids[0]],"to":"forward"}));
    assert_eq!(s.project.to_file_json().unwrap(), before);
    assert_eq!(s.history.undo.len(), undo);
    let cid = s.active_comp_id().unwrap();
    let duplicate = run(&mut s, "project.duplicate", json!({"items":[cid.0]}))["items"][0].as_u64().unwrap();
    let c = s.project.comp(effectcraft_engine::project::ItemId(duplicate)).unwrap();
    assert!(c.layer_groups_are_valid());
    assert_ne!(c.layer_groups[0].id, group);
    assert!(c.layer_groups[0].members.iter().all(|id| !ids.contains(&id.0)));
    run(&mut s, "edit.deselectAll", json!({}));
    run(&mut s, "layer.ungroup", json!({"group":group}));
    assert!(s.active_comp().unwrap().layer_groups.is_empty(), "explicit group needs no layer selection");
}

#[test]
fn reorder_within_folder_and_insert_an_ordinary_layer_at_the_requested_drop_position() {
    let (mut s, ids) = setup();
    run(&mut s, "layer.select", json!({"layers":[ids[0],ids[1],ids[2]]}));
    run(&mut s, "layer.group", json!({}));
    run(&mut s, "layer.arrange", json!({"layers":[ids[2]],"above":ids[1]}));
    assert_eq!(order(&s), ["A", "C", "B", "D", "E"]);
    assert!(s.active_comp().unwrap().layer_groups_are_valid());
    assert_eq!(s.active_comp().unwrap().layer_groups[0].members.iter().map(|id| id.0).collect::<Vec<_>>(), [ids[0], ids[2], ids[1]]);
    run(&mut s, "layer.arrange", json!({"layers":[ids[4]],"above":ids[1]}));
    assert_eq!(order(&s), ["A", "C", "E", "B", "D"]);
    assert_eq!(s.active_comp().unwrap().layer_groups[0].members.len(), 4);
    assert!(s.active_comp().unwrap().layer_groups_are_valid());
}

#[test]
fn group_operations_respect_locked_layers_left_in_selection() {
    let (mut s, ids) = setup();
    run(&mut s, "layer.select", json!({"layers":[ids[0],ids[2],ids[4]]}));
    run(&mut s, "layer.setSwitch", json!({"layers":[ids[2]],"switch":"lock","value":true}));
    let group = run(&mut s, "layer.group", json!({}))["group"].as_u64().unwrap();
    assert!(!s.active_comp().unwrap().layer_groups[0].members.contains(&LayerId(ids[2])));
    let before = s.project.to_file_json().unwrap();
    let undo = s.history.undo.len();
    assert!(s.execute("layer.groupAdd", json!({"group":group,"layers":[ids[2]]})).is_err());
    assert!(s.execute("layer.groupRemove", json!({"layers":[ids[2]]})).is_err());
    assert_eq!(s.project.to_file_json().unwrap(), before);
    assert_eq!(s.history.undo.len(), undo);
}

#[test]
fn drop_into_folder_and_out_at_boundary_is_atomic_even_without_order_change() {
    let (mut s, ids) = setup();
    let group = run(&mut s, "layer.group", json!({"layers":[ids[1],ids[2]],"name":"BC"}))["group"].as_u64().unwrap();
    let original_layers = s.active_comp().unwrap().layers.clone();
    let before = s.project.to_file_json().unwrap();
    let undo = s.history.undo.len();
    run(&mut s, "layer.drop", json!({"layers":[ids[4],ids[0]],"into":group}));
    assert_eq!(order(&s), ["B", "C", "A", "E", "D"]);
    assert_eq!(s.active_comp().unwrap().layer_groups[0].members.iter().map(|l| l.0).collect::<Vec<_>>(), [ids[1], ids[2], ids[0], ids[4]]);
    assert_eq!(s.history.undo.len(), undo + 1);
    for layer in original_layers {
        assert_eq!(s.active_comp().unwrap().layer(layer.id), Some(&layer));
    }
    let after = s.project.to_file_json().unwrap();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project.to_file_json().unwrap(), before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.project.to_file_json().unwrap(), after);
    run(&mut s, "layer.drop", json!({"layers":[ids[4]],"above":ids[3]}));
    assert_eq!(order(&s), ["B", "C", "A", "E", "D"], "boundary extraction can keep stack order");
    assert!(!s.active_comp().unwrap().layer_groups[0].members.contains(&LayerId(ids[4])));
    assert_eq!(s.history.undo.len(), undo + 2);
    assert!(s.active_comp().unwrap().layer_groups_are_valid());
}

#[test]
fn drop_between_folders_reorders_whole_folder_without_merging_or_expanding_it() {
    let (mut s, ids) = setup();
    let first = run(&mut s, "layer.group", json!({"layers":[ids[0],ids[1]]}))["group"].as_u64().unwrap();
    let second = run(&mut s, "layer.group", json!({"layers":[ids[2],ids[3]]}))["group"].as_u64().unwrap();
    run(&mut s, "layer.groupUpdate", json!({"group":first,"collapsed":true}));
    let undo = s.history.undo.len();
    run(&mut s, "layer.drop", json!({"group":first,"above":ids[4]}));
    assert_eq!(order(&s), ["C", "D", "A", "B", "E"]);
    assert_eq!(s.history.undo.len(), undo + 1);
    assert!(s.active_comp().unwrap().layer_groups.iter().find(|g| g.id == first).unwrap().collapsed);
    run(&mut s, "layer.drop", json!({"group":first,"above":ids[3]}));
    assert_eq!(order(&s), ["A", "B", "C", "D", "E"], "whole folder cannot split another folder");
    assert!(s.active_comp().unwrap().layer_groups_are_valid());
    let before = s.project.to_file_json().unwrap();
    let undo = s.history.undo.len();
    run(&mut s, "layer.drop", json!({"group":first,"above":ids[1]}));
    assert_eq!(s.history.undo.len(), undo);
    run(&mut s, "layer.drop", json!({"group":first,"into":second}));
    assert_eq!(s.active_comp().unwrap().layer_groups.iter().find(|g| g.id == first).unwrap().parent, Some(second));
    assert!(s.active_comp().unwrap().layer_groups_are_valid());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project.to_file_json().unwrap(), before);
}

#[test]
fn nested_create_move_and_ungroup_preserve_children_and_exact_undo() {
    let (mut s, ids) = setup();
    let outer = run(&mut s, "layer.group", json!({"layers":[ids[0],ids[1],ids[2],ids[3]],"name":"Outer"}))["group"].as_u64().unwrap();
    let inner = run(&mut s, "layer.group", json!({"layers":[ids[1],ids[2]],"name":"Inner"}))["group"].as_u64().unwrap();
    let comp = s.active_comp().unwrap();
    assert_eq!(comp.layer_group_path(LayerId(ids[1])).iter().map(|g| g.id).collect::<Vec<_>>(), [outer, inner]);
    assert_eq!(comp.layer_groups.iter().find(|g| g.id == outer).unwrap().members.len(), 4);
    run(&mut s, "layer.groupUpdate", json!({"group":inner,"video":false,"audio":false,"collapsed":true}));
    let before = s.project.to_file_json().unwrap();
    let undo = s.history.undo.len();
    run(&mut s, "layer.drop", json!({"group":inner}));
    assert_eq!(s.history.undo.len(), undo + 1);
    assert_eq!(order(&s), ["A", "D", "E", "B", "C"]);
    let comp = s.active_comp().unwrap();
    let child = comp.layer_groups.iter().find(|g| g.id == inner).unwrap();
    assert_eq!(child.parent, None);
    assert!(!child.video && !child.audio && child.collapsed);
    assert!(comp.layer_groups_are_valid());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project.to_file_json().unwrap(), before);
    run(&mut s, "layer.ungroup", json!({"group":outer}));
    let comp = s.active_comp().unwrap();
    assert!(comp.layer_groups.iter().all(|g| g.id != outer));
    assert_eq!(comp.layer_groups.iter().find(|g| g.id == inner).unwrap().parent, None);
    assert_eq!(order(&s), ["A", "B", "C", "D", "E"]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project.to_file_json().unwrap(), before);
}

#[test]
fn nested_drop_rejects_cycles_and_locked_destinations_without_an_edit() {
    let (mut s, ids) = setup();
    let outer = run(&mut s, "layer.group", json!({"layers":[ids[0],ids[1],ids[2]]}))["group"].as_u64().unwrap();
    let inner = run(&mut s, "layer.group", json!({"layers":[ids[1],ids[2]]}))["group"].as_u64().unwrap();
    let before = s.project.to_file_json().unwrap();
    let undo = s.history.undo.len();
    for target in [outer, inner] {
        assert!(s.execute("layer.drop", json!({"group":outer,"into":target})).is_err());
        assert_eq!(s.project.to_file_json().unwrap(), before);
        assert_eq!(s.history.undo.len(), undo);
    }
    run(&mut s, "layer.groupUpdate", json!({"group":outer,"locked":true}));
    let locked = s.project.to_file_json().unwrap();
    assert!(s.execute("layer.drop", json!({"layers":[ids[4]],"into":inner})).is_err());
    assert_eq!(s.project.to_file_json().unwrap(), locked);
    run(&mut s, "layer.groupUpdate", json!({"group":outer,"locked":false}));
    run(&mut s, "layer.drop", json!({"layers":[ids[4]],"into":inner}));
    assert_eq!(s.active_comp().unwrap().layer_group_path(LayerId(ids[4])).iter().map(|g| g.id).collect::<Vec<_>>(), [outer, inner]);
}

#[test]
fn nesting_preserves_raw_switches_and_duplicate_remaps_group_parents() {
    let (mut s, ids) = setup();
    run(&mut s, "layer.setSwitch", json!({"layers":[ids[0]],"switch":"video","value":false}));
    let original = s.active_comp().unwrap().layers.clone();
    let inner = run(&mut s, "layer.group", json!({"layers":[ids[0],ids[1]]}))["group"].as_u64().unwrap();
    let outer = run(&mut s, "layer.group", json!({"layers":[ids[0],ids[1],ids[2]]}))["group"].as_u64().unwrap();
    let before = s.project.to_file_json().unwrap();
    run(&mut s, "layer.groupUpdate", json!({"group":outer,"video":false,"audio":false,"locked":true}));
    for layer in &original {
        assert_eq!(s.active_comp().unwrap().layer(layer.id), Some(layer));
    }
    assert!(!s.active_comp().unwrap().layer_effectively_visible(LayerId(ids[1])));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.project.to_file_json().unwrap(), before);
    assert!(!s.active_comp().unwrap().layer_effectively_visible(LayerId(ids[0])));
    assert!(s.active_comp().unwrap().layer_effectively_visible(LayerId(ids[1])));
    let cid = s.active_comp_id().unwrap();
    let copy = run(&mut s, "project.duplicate", json!({"items":[cid.0]}))["items"][0].as_u64().unwrap();
    let c = s.project.comp(effectcraft_engine::project::ItemId(copy)).unwrap();
    let child = c.layer_groups.iter().find(|g| g.parent.is_some()).unwrap();
    assert_ne!(child.id, inner);
    assert_ne!(child.parent, Some(outer));
    assert!(c.layer_groups.iter().any(|g| Some(g.id) == child.parent));
    assert!(c.layer_groups_are_valid());
}

#[test]
fn mixed_arrange_cannot_silently_drop_unselected_members_or_group_gates() {
    let (mut s, ids) = setup();
    run(&mut s, "layer.group", json!({"layers":[ids[0],ids[1]],"name":"G"}));
    let h = run(&mut s, "layer.group", json!({"layers":[ids[2],ids[3],ids[4]],"name":"H"}))["group"].as_u64().unwrap();
    run(&mut s, "layer.groupUpdate", json!({"group":h,"video":false,"audio":false}));
    let before = s.project.to_file_json().unwrap();
    let undo = s.history.undo.len();
    assert!(s.execute("layer.arrange", json!({"layers":[ids[0],ids[1],ids[3]],"to":"backward"})).is_err());
    assert_eq!(s.project.to_file_json().unwrap(), before);
    assert_eq!(s.history.undo.len(), undo);
    assert!(!s.active_comp().unwrap().layer_effectively_visible(LayerId(ids[4])));
    assert!(!s.active_comp().unwrap().layer_effectively_audio(LayerId(ids[4])));
}

#[test]
fn nested_arrange_moves_each_leaf_once_and_keeps_tree_valid() {
    let (mut s, ids) = setup();
    run(&mut s, "layer.group", json!({"layers":[ids[1],ids[2]]}));
    let outer = run(&mut s, "layer.group", json!({"layers":[ids[0],ids[1],ids[2]]}))["group"].as_u64().unwrap();
    for how in ["back", "front", "backward", "forward"] {
        run(&mut s, "layer.arrange", json!({"layers":[ids[0],ids[1],ids[2]],"to":how}));
        let c = s.active_comp().unwrap();
        assert!(c.layer_groups_are_valid());
        let all: std::collections::HashSet<_> = c.layers.iter().map(|l| l.id).collect();
        assert_eq!(all.len(), 5);
        assert_eq!(c.layers.len(), 5);
        assert_eq!(c.layer_groups.iter().find(|g| g.id == outer).unwrap().members.len(), 3);
    }
}

#[test]
fn drop_transfers_members_at_exact_child_position_and_deletes_empty_source() {
    let (mut s, ids) = setup();
    let first = run(&mut s, "layer.group", json!({"layers":[ids[0],ids[1]]}))["group"].as_u64().unwrap();
    let second = run(&mut s, "layer.group", json!({"layers":[ids[2],ids[3]]}))["group"].as_u64().unwrap();
    run(&mut s, "layer.drop", json!({"layers":[ids[1]],"into":second,"above":ids[3]}));
    assert_eq!(order(&s), ["A", "C", "B", "D", "E"]);
    run(&mut s, "layer.drop", json!({"layers":[ids[0]],"into":second,"above":ids[2]}));
    assert_eq!(order(&s), ["A", "C", "B", "D", "E"]);
    assert!(s.active_comp().unwrap().layer_groups.iter().all(|g| g.id != first));
    assert_eq!(s.active_comp().unwrap().layer_groups[0].members.len(), 4);
    assert!(s.active_comp().unwrap().layer_groups_are_valid());
    let before = s.project.to_file_json().unwrap();
    assert!(s.execute("layer.drop", json!({"layers":[ids[4]],"into":second,"above":ids[4]})).is_err());
    assert!(s.execute("layer.drop", json!({"layers":[ids[4]],"above":999999})).is_err());
    assert!(s.execute("layer.drop", json!({"layers":[ids[4]],"into":999999})).is_err());
    assert_eq!(s.project.to_file_json().unwrap(), before);
}

#[test]
fn drop_respects_locks_and_never_splits_a_locked_folder() {
    let (mut s, ids) = setup();
    let group = run(&mut s, "layer.group", json!({"layers":[ids[1],ids[2]]}))["group"].as_u64().unwrap();
    run(&mut s, "layer.setSwitch", json!({"layers":[ids[2]],"switch":"lock","value":true}));
    let before = s.project.to_file_json().unwrap();
    let undo = s.history.undo.len();
    assert!(s.execute("layer.drop", json!({"group":group})).is_err());
    assert_eq!(s.project.to_file_json().unwrap(), before);
    assert_eq!(s.history.undo.len(), undo);
    run(&mut s, "layer.drop", json!({"layers":[ids[1],ids[2]]}));
    assert_eq!(order(&s), ["A", "C", "D", "E", "B"]);
    assert_eq!(s.active_comp().unwrap().layer_groups[0].members, [LayerId(ids[2])]);
}

#[test]
fn active_layer_follows_its_compositions_remembered_selection() {
    let (mut s, ids) = setup();
    let first = s.active_comp_id().unwrap();
    let second = run(&mut s, "comp.new", json!({"name":"Second","width":64,"height":64,"duration":6}))["comp"].as_u64().unwrap();
    let other = run(&mut s, "layer.newSolid", json!({"name":"Other"}))["layer"].as_u64().unwrap();
    run(&mut s, "comp.open", json!({"comp":first.0}));
    run(&mut s, "layer.select", json!({"layers":[ids[0],ids[2]],"active":ids[2]}));
    run(&mut s, "comp.open", json!({"comp":second}));
    run(&mut s, "layer.select", json!({"layers":[other]}));
    run(&mut s, "comp.open", json!({"comp":first.0}));
    assert_eq!(s.state.selected_layers, [LayerId(ids[0]), LayerId(ids[2])]);
    assert_eq!(s.active_layer(), Some(LayerId(ids[2])), "restore the explicit primary, not the topmost fallback");
    run(&mut s, "comp.open", json!({"comp":second}));
    assert_eq!(s.active_layer(), Some(LayerId(other)));
    run(&mut s, "edit.deselectAll", json!({}));
    assert_eq!(s.active_layer(), None);
    run(&mut s, "comp.open", json!({"comp":first.0}));
    assert_eq!(s.active_layer(), Some(LayerId(ids[2])));
    run(&mut s, "comp.open", json!({"comp":second}));
    assert!(s.state.selected_layers.is_empty());
    assert_eq!(s.active_layer(), None);
}

#[test]
fn importing_a_project_remaps_folder_ids_and_members_and_undoes_once() {
    let (mut source, ids) = setup();
    let group = run(&mut source, "layer.group", json!({"layers":[ids[1],ids[2]],"name":"BC"}))["group"].as_u64().unwrap();
    run(&mut source, "layer.groupUpdate", json!({"group":group,"collapsed":true}));
    let dir = std::env::temp_dir().join(format!("ec-import-layer-groups-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("grouped.ecproj");
    run(&mut source, "file.saveAs", json!({"path":path}));
    let (mut target, _) = setup();
    let existing: Vec<_> = target.active_comp().unwrap().layers.iter().map(|l| l.id).collect();
    let before = target.project.to_file_json().unwrap();
    let undo = target.history.undo.len();
    let imported = run(&mut target, "file.importProject", json!({"path":path}));
    let cid = effectcraft_engine::project::ItemId(imported["comps"][0]["id"].as_u64().unwrap());
    let comp = target.project.comp(cid).unwrap();
    assert!(comp.layer_groups_are_valid());
    let g = &comp.layer_groups[0];
    assert_ne!(g.id, group);
    assert_eq!(g.name, "BC");
    assert!(g.collapsed);
    assert!(g.members.iter().all(|id| !existing.contains(id)));
    assert_eq!(g.members.iter().map(|id| comp.layer(*id).unwrap().name.as_str()).collect::<Vec<_>>(), ["B", "C"]);
    assert!(target.project.next_id > g.id);
    assert_eq!(target.history.undo.len(), undo + 1);
    let after = target.project.to_file_json().unwrap();
    let loaded = effectcraft_engine::project::Project::from_json(&after).unwrap();
    assert_eq!(loaded.comp(cid).unwrap().layer_groups, comp.layer_groups);
    run(&mut target, "edit.undo", json!({}));
    assert_eq!(target.project.to_file_json().unwrap(), before);
    run(&mut target, "edit.redo", json!({}));
    assert_eq!(target.project.to_file_json().unwrap(), after);
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}
