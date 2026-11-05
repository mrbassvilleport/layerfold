use effectcraft_engine::{Session, project::LayerId};
use serde_json::json;

fn nested() -> (Session, Vec<u64>, u64, u64) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name":"Review","width":64,"height":36,"duration":6})).unwrap();
    for name in ["X", "B", "A"] {
        s.execute("layer.newSolid", json!({"name":name,"color":"#406080"})).unwrap();
    }
    let ids: Vec<_> = s.active_comp().unwrap().layers.iter().map(|l| l.id.0).collect();
    let inner = s.execute("layer.group", json!({"layers":[ids[0],ids[1]],"name":"Inner"})).unwrap()["group"].as_u64().unwrap();
    let outer = s.execute("layer.group", json!({"layers":[ids[0],ids[1]],"name":"Outer"})).unwrap()["group"].as_u64().unwrap();
    (s, ids, inner, outer)
}

#[test]
fn arrange_rejects_entry_into_locked_group_atomically() {
    for by_index in [false, true] {
        for ancestor in [false, true] {
            let (mut s, ids, inner, outer) = nested();
            let locked = if ancestor { outer } else { inner };
            s.execute("layer.groupUpdate", json!({"group":locked,"locked":true})).unwrap();
            let before = s.project.to_file_json().unwrap();
            let undo = s.history.undo.len();
            let params = if by_index { json!({"layers":[ids[2]],"index":2}) } else { json!({"layers":[ids[2]],"above":ids[1]}) };
            let result = s.execute("layer.arrange", params.clone());
            assert!(result.is_err(), "arrange must reject entry into a locked destination: {params}");
            assert!(result.unwrap_err().to_string().contains("locked"));
            assert_eq!(s.project.to_file_json().unwrap(), before);
            assert_eq!(s.history.undo.len(), undo);
            s.execute("layer.groupUpdate", json!({"group":locked,"locked":false})).unwrap();
            let before = s.project.to_file_json().unwrap();
            s.execute("layer.arrange", params).unwrap();
            assert!(s.active_comp().unwrap().layer_groups.iter().all(|g| g.members.contains(&LayerId(ids[2]))));
            let after = s.project.to_file_json().unwrap();
            s.execute("edit.undo", json!({})).unwrap();
            assert_eq!(s.project.to_file_json().unwrap(), before);
            s.execute("edit.redo", json!({})).unwrap();
            assert_eq!(s.project.to_file_json().unwrap(), after);
        }
    }
}
#[test]
fn duplicate_inherits_every_group_of_the_source_layer() {
    copies_inherit_groups("edit.duplicate");
}
#[test]
fn split_inherits_every_group_of_the_source_layer() {
    copies_inherit_groups("edit.splitLayer");
}

fn copies_inherit_groups(command: &str) {
    for above in [false, true] {
        for first in [false, true] {
            let (mut s, ids, _, _) = nested();
            let original = LayerId(ids[if first { 0 } else { 1 }]);
            s.prefs.general.create_split_layers_above = above;
            s.execute("prop.addKey", json!({"layer":original.0,"path":"transform/opacity","time":2,"value":70})).unwrap();
            s.execute("time.set", json!({"time":3})).unwrap();
            let groups: Vec<_> = s.active_comp().unwrap().layer_group_path(original).iter().map(|g| g.id).collect();
            let before = s.project.to_file_json().unwrap();
            let undo = s.history.undo.len();
            let created = s.execute(command, json!({"layers":[original.0]})).unwrap();
            let copy = LayerId(created[0].as_u64().unwrap());
            let c = s.active_comp().unwrap();
            let copy_groups: Vec<_> = c.layer_group_path(copy).iter().map(|g| g.id).collect();
            assert_eq!(copy_groups, groups, "{command}, above={above}, first={first}");
            assert!(c.layer_groups_are_valid());
            assert_eq!(
                c.layer(original).unwrap().props.prop("transform/opacity").unwrap().keys,
                c.layer(copy).unwrap().props.prop("transform/opacity").unwrap().keys
            );
            assert_eq!(s.history.undo.len(), undo + 1);
            let after = s.project.to_file_json().unwrap();
            let loaded = effectcraft_engine::project::Project::from_json(&after).unwrap();
            assert_eq!(loaded.comp(s.active_comp_id().unwrap()).unwrap().layer_groups, c.layer_groups);
            s.execute("edit.undo", json!({})).unwrap();
            assert_eq!(s.project.to_file_json().unwrap(), before);
            s.execute("edit.redo", json!({})).unwrap();
            assert_eq!(s.project.to_file_json().unwrap(), after);
        }
    }
}
#[test]
fn invalid_group_names_report_the_invoked_command_without_edits() {
    for command in ["layer.group", "layer.groupUpdate"] {
        for name in [" ".to_string(), "a".repeat(257)] {
            let (mut s, ids, inner, _) = nested();
            let before = s.project.to_file_json().unwrap();
            let undo = s.history.undo.len();
            let error = s.execute(command, json!({"group":inner,"layers":[ids[2]],"name":name})).unwrap_err();
            assert!(error.to_string().contains(command), "{command}: {error}");
            assert_eq!(s.project.to_file_json().unwrap(), before);
            assert_eq!(s.history.undo.len(), undo);
        }
    }
}

#[test]
fn new_layer_insertion_respects_locked_group_boundaries() {
    for ancestor in [false, true] {
        for command in ["indexed item", "footage item", "selected item", "layer.newNull", "layer.newSolid"] {
            let (mut s, ids, inner, outer) = nested();
            let main = s.active_comp_id().unwrap();
            let source = s.execute("comp.new", json!({"name":"Source","width":16,"height":16,"duration":2})).unwrap()["comp"].as_u64().unwrap();
            s.execute("comp.open", json!({"comp":main.0})).unwrap();
            let footage = s.execute("file.importPlaceholder", json!({"name":"Clip.png","width":16,"height":16})).unwrap()["item"].as_u64().unwrap();
            s.execute("layer.select", json!({"layers":[ids[1]]})).unwrap();
            let locked = if ancestor { outer } else { inner };
            s.execute("layer.groupUpdate", json!({"group":locked,"locked":true})).unwrap();
            let before = s.project.to_file_json().unwrap();
            let selection = s.state.selected_layers.clone();
            let active = s.active_layer();
            let undo = s.history.undo.len();
            let redo = s.history.redo.len();
            let result = match command {
                "indexed item" => s.execute("layer.addItem", json!({"item":source,"index":2})),
                "footage item" => s.execute("layer.addItem", json!({"item":footage,"index":2})),
                "selected item" => s.execute("layer.addItem", json!({"item":source})),
                _ => s.execute(command, json!({})),
            };
            assert!(result.is_err(), "{command}, ancestor={ancestor}: inserted inside locked group");
            assert!(result.unwrap_err().to_string().contains("locked"));
            assert_eq!(s.project.to_file_json().unwrap(), before);
            assert_eq!(s.state.selected_layers, selection);
            assert_eq!(s.active_layer(), active);
            assert_eq!((s.history.undo.len(), s.history.redo.len()), (undo, redo));
            for index in [1, 3] {
                let groups = s.active_comp().unwrap().layer_groups.clone();
                let copy = s.execute("layer.addItem", json!({"item":source,"index":index})).unwrap()["layer"].as_u64().unwrap();
                assert_eq!(s.active_comp().unwrap().layer_groups, groups);
                assert!(!s.active_comp().unwrap().layer_effectively_locked(LayerId(copy)));
                let after = s.project.to_file_json().unwrap();
                s.execute("edit.undo", json!({})).unwrap();
                assert_eq!(s.project.to_file_json().unwrap(), before);
                s.execute("edit.redo", json!({})).unwrap();
                assert_eq!(s.project.to_file_json().unwrap(), after);
                s.execute("edit.undo", json!({})).unwrap();
            }
            s.execute("layer.groupUpdate", json!({"group":locked,"locked":false})).unwrap();
            let copy = s.execute("layer.addItem", json!({"item":source,"index":2})).unwrap()["layer"].as_u64().unwrap();
            assert_eq!(s.active_comp().unwrap().layer_group_path(LayerId(copy)).len(), 2);
        }
    }
}

#[test]
fn browser_import_refuses_locked_destination_before_importing() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Probe(Arc<AtomicUsize>);
    impl effectcraft_engine::Importer for Probe {
        fn probe(&self, path: &str) -> Result<effectcraft_engine::project::Footage, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(effectcraft_engine::project::Footage { path: path.into(), width: 16, height: 16, has_video: true, ..Default::default() })
        }
    }
    let (mut s, _, inner, _) = nested();
    let probes = Arc::new(AtomicUsize::new(0));
    s.importer = Some(Arc::new(Probe(probes.clone())));
    s.execute("layer.groupUpdate", json!({"group":inner,"locked":true})).unwrap();
    let before = s.project.to_file_json().unwrap();
    let undo = s.history.undo.len();
    let params = json!({"paths":["locked-drop.png"],"addToComp":true,"index":2});
    let result = s.execute("mediaBrowser.import", params.clone());
    assert!(result.is_err(), "Media Browser must report a rejected destination before importing: {result:?}");
    assert!(result.unwrap_err().to_string().contains("locked"));
    assert_eq!(probes.load(Ordering::SeqCst), 0, "no file decoding for a rejected drop");
    assert_eq!(s.project.to_file_json().unwrap(), before);
    assert_eq!(s.history.undo.len(), undo);
    s.execute("layer.groupUpdate", json!({"group":inner,"locked":false})).unwrap();
    let result = s.execute("mediaBrowser.import", params).unwrap();
    assert!(result["errors"].as_array().unwrap().is_empty(), "{result:?}");
    assert_eq!(probes.load(Ordering::SeqCst), 1);
    let layer = s.active_layer().unwrap();
    assert_eq!(s.active_comp().unwrap().layer_group_path(layer).len(), 2);
}

#[test]
fn missing_group_ids_report_the_invoked_command_without_edits() {
    for command in ["layer.groupUpdate", "layer.groupAdd"] {
        for group in [None, Some(json!(null)), Some(json!("bad")), Some(json!(-1)), Some(json!({}))] {
            let (mut s, ids, _, _) = nested();
            let mut params = if command == "layer.groupAdd" { json!({"layers":[ids[2]]}) } else { json!({"name":"Renamed"}) };
            if let Some(group) = group {
                params["group"] = group;
            }
            let before = s.project.to_file_json().unwrap();
            let undo = s.history.undo.len();
            let error = s.execute(command, params).unwrap_err();
            assert!(error.to_string().contains(command), "{command}: {error}");
            assert_eq!(s.project.to_file_json().unwrap(), before);
            assert_eq!(s.history.undo.len(), undo);
        }
    }
}

#[test]
fn paste_respects_locked_group_boundaries_without_changing_clipboard_or_history() {
    for ancestor in [false, true] {
        let (mut s, ids, inner, outer) = nested();
        s.execute("edit.copy", json!({"layers":[ids[2]]})).unwrap();
        s.execute("layer.select", json!({"layers":[ids[1]]})).unwrap();
        let locked = if ancestor { outer } else { inner };
        s.execute("layer.groupUpdate", json!({"group":locked,"locked":true})).unwrap();
        let before = s.project.to_file_json().unwrap();
        let selection = s.state.selected_layers.clone();
        let active = s.active_layer();
        let clipboard = serde_json::to_value(&s.state.clipboard).unwrap();
        let history = (s.history.undo.len(), s.history.redo.len());
        let result = s.execute("edit.paste", json!({}));
        assert!(result.is_err(), "Paste entered a locked group, ancestor={ancestor}: {result:?}");
        assert!(result.unwrap_err().to_string().contains("locked"));
        assert_eq!(s.project.to_file_json().unwrap(), before);
        assert_eq!(s.state.selected_layers, selection);
        assert_eq!(s.active_layer(), active);
        assert_eq!(serde_json::to_value(&s.state.clipboard).unwrap(), clipboard);
        assert_eq!((s.history.undo.len(), s.history.redo.len()), history);

        // The gap before the whole locked group is still a valid paste destination.
        s.execute("edit.undo", json!({})).unwrap();
        s.execute("layer.select", json!({"layers":[ids[0]]})).unwrap();
        s.execute("layer.groupUpdate", json!({"group":locked,"locked":true})).unwrap();
        assert_eq!(s.state.selected_layers, [LayerId(ids[0])]);
        assert_eq!(s.project.to_file_json().unwrap(), before);
        let groups = s.active_comp().unwrap().layer_groups.clone();
        let created = s.execute("edit.paste", json!({})).unwrap();
        let copy = LayerId(created[0].as_u64().unwrap());
        assert!(s.active_comp().unwrap().layer_group_path(copy).is_empty());
        assert_eq!(s.active_comp().unwrap().layer_groups, groups);
        let after = s.project.to_file_json().unwrap();
        assert_eq!(s.history.undo.len(), history.0 + 1);
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(s.project.to_file_json().unwrap(), before);
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(s.project.to_file_json().unwrap(), after);
        s.execute("edit.undo", json!({})).unwrap();

        s.execute("layer.groupUpdate", json!({"group":locked,"locked":false})).unwrap();
        s.execute("layer.select", json!({"layers":[ids[1]]})).unwrap();
        let created = s.execute("edit.paste", json!({})).unwrap();
        let copy = LayerId(created[0].as_u64().unwrap());
        assert_eq!(s.active_comp().unwrap().layer_group_path(copy).len(), 2);
        assert!(s.active_comp().unwrap().layer_groups_are_valid());
        assert_eq!(serde_json::to_value(&s.state.clipboard).unwrap(), clipboard);
    }
}

#[test]
fn ancestor_lock_protects_both_edges_of_an_unlocked_child_group() {
    for index in [2, 3] {
        for command in ["layer.addItem", "layer.newSolid", "edit.paste"] {
            let mut s = Session::default();
            s.execute("comp.new", json!({"name":"Distinct boundaries"})).unwrap();
            let source = s.execute("file.importPlaceholder", json!({"name":"Source.png","width":16,"height":16})).unwrap()["item"].as_u64().unwrap();
            for name in ["X", "C", "B", "A"] {
                s.execute("layer.newSolid", json!({"name":name})).unwrap();
            }
            let ids: Vec<_> = s.active_comp().unwrap().layers.iter().map(|l| l.id).collect();
            s.execute("layer.select", json!({"layers":[ids[1].0]})).unwrap();
            let inner = s.execute("layer.group", json!({"layers":[ids[1].0],"name":"Inner"})).unwrap()["group"].as_u64().unwrap();
            s.execute("layer.select", json!({"layers":[ids[0].0]})).unwrap();
            let outer = s.execute("layer.group", json!({"layers":[ids[0].0,ids[1].0,ids[2].0],"name":"Outer"})).unwrap()["group"].as_u64().unwrap();
            let comp = s.active_comp().unwrap();
            assert_eq!(comp.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["A", "B", "C", "X"]);
            let child = comp.layer_groups.iter().find(|g| g.id == inner).unwrap();
            assert_eq!(child.members, [ids[1]]);
            assert_eq!(child.parent, Some(outer));
            assert!(!child.locked);
            assert_eq!(comp.layer_groups.iter().find(|g| g.id == outer).unwrap().members, ids[..3]);
            s.execute("edit.copy", json!({"layers":[ids[3].0]})).unwrap();
            s.execute("layer.select", json!({"layers":[ids[index - 1].0]})).unwrap();
            s.execute("layer.groupUpdate", json!({"group":outer,"locked":true})).unwrap();
            let before = s.project.to_file_json().unwrap();
            let selection = s.state.selected_layers.clone();
            let history = (s.history.undo.len(), s.history.redo.len());
            let params = if command == "layer.addItem" { json!({"item":source,"index":index}) } else { json!({}) };
            let result = s.execute(command, params.clone());
            assert!(result.is_err(), "{command} entered locked Outer at edge {index} of unlocked Inner");
            assert!(result.unwrap_err().to_string().contains("locked"));
            assert_eq!(s.project.to_file_json().unwrap(), before);
            assert_eq!(s.state.selected_layers, selection);
            assert_eq!((s.history.undo.len(), s.history.redo.len()), history);
            s.execute("layer.groupUpdate", json!({"group":outer,"locked":false})).unwrap();
            let unlocked = s.project.to_file_json().unwrap();
            s.execute(command, params).unwrap();
            let copy = s.state.selected_layers[0];
            assert_eq!(s.active_comp().unwrap().layer_group_path(copy).iter().map(|g| g.id).collect::<Vec<_>>(), [outer]);
            let after = s.project.to_file_json().unwrap();
            s.execute("edit.undo", json!({})).unwrap();
            assert_eq!(s.project.to_file_json().unwrap(), unlocked);
            s.execute("edit.redo", json!({})).unwrap();
            assert_eq!(s.project.to_file_json().unwrap(), after);
        }
    }
}
