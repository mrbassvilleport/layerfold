use effectcraft_engine::{
    Session,
    project::{LayerGroup, LayerId},
};
use serde_json::json;

fn nested() -> (Session, Vec<u64>, u64) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name":"Inherited locks","width":64,"height":64,"duration":6,"frameRate":24})).unwrap();
    for name in ["Outside", "Own lock", "Inherited lock"] {
        s.execute("layer.newSolid", json!({"name":name,"color":"#406080","width":16,"height":16})).unwrap();
    }
    let ids: Vec<u64> = s.active_comp().unwrap().layers.iter().map(|l| l.id.0).collect();
    for id in &ids {
        for (time, value) in [(1, 10), (2, 70)] {
            s.execute("prop.addKey", json!({"layer":id,"path":"transform/opacity","time":time,"value":value})).unwrap();
        }
    }
    s.execute("layer.select", json!({"layers":ids})).unwrap();
    s.execute("keys.selectAll", json!({})).unwrap();
    s.execute("layer.setSwitch", json!({"layer":ids[1],"switch":"lock","value":true})).unwrap();
    let cid = s.active_comp_id().unwrap();
    let root = s.project.next_id;
    s.edit("Nested fixture", None, |p, _| {
        p.next_id += 2;
        let c = p.comp_mut(cid).unwrap();
        c.layer_groups = vec![
            LayerGroup { id: root, name: "Root".into(), members: vec![LayerId(ids[0]), LayerId(ids[1])], ..Default::default() },
            LayerGroup { id: root + 1, name: "Inner".into(), parent: Some(root), members: vec![LayerId(ids[0])], ..Default::default() },
        ];
        Ok(())
    })
    .unwrap();
    s.execute("layer.groupUpdate", json!({"group":root,"locked":true})).unwrap();
    (s, ids, root)
}

#[test]
fn ancestor_lock_blocks_explicit_property_and_timing_commands_without_undo() {
    let (_, ids, _) = nested();
    for (command, params) in [
        ("prop.set", json!({"layer":ids[0],"path":"transform/opacity","value":40})),
        ("prop.addKey", json!({"layer":ids[0],"path":"transform/opacity","time":3,"value":40})),
        ("layer.rename", json!({"layer":ids[0],"name":"Changed"})),
        ("layer.expressions", json!({"layers":[ids[0]],"enabled":false})),
        ("layer.autoOrient", json!({"layers":[ids[0]],"mode":"alongPath"})),
        ("edit.duplicate", json!({"layers":[ids[0]]})),
        ("layer.timing", json!({"layers":[ids[0]],"delta":1})),
        ("layer.timeStretch", json!({"layers":[ids[0]],"percent":200})),
        ("layer.setParent", json!({"layers":[ids[0]],"parent":ids[2]})),
        ("edit.clear", json!({"layers":[ids[0]]})),
        ("layer.arrange", json!({"layers":[ids[0]],"to":"back"})),
    ] {
        let (mut s, _, _) = nested();
        let before = s.project.to_file_json().unwrap();
        let undo = s.history.undo.len();
        assert!(s.execute(command, params).is_err(), "{command}");
        assert_eq!(s.project.to_file_json().unwrap(), before, "{command}");
        assert_eq!(s.history.undo.len(), undo, "{command}");
    }
}

#[test]
fn keys_selected_before_lock_stay_unchanged_in_every_key_edit_path() {
    for (command, params) in [
        ("keys.move", json!({"delta":1})),
        ("keys.delete", json!({})),
        ("keys.easyEase", json!({})),
        ("keys.timeReverse", json!({})),
        ("keys.transform", json!({"timeOffset":1,"valueOffset":20})),
    ] {
        let (mut s, ids, _) = nested();
        let locked: Vec<_> = ids[..2].iter().map(|id| s.active_comp().unwrap().layer(LayerId(*id)).unwrap().clone()).collect();
        let outside = s.active_comp().unwrap().layer(LayerId(ids[2])).unwrap().clone();
        s.execute(command, params).unwrap();
        for layer in &locked {
            assert_eq!(s.active_comp().unwrap().layer(layer.id), Some(layer), "{command}");
        }
        assert_ne!(s.active_comp().unwrap().layer(outside.id), Some(&outside), "{command} must still edit unlocked keys");
    }
}

#[test]
fn mixed_timing_and_selection_skip_descendants_and_unlock_restores_raw_child_lock() {
    let (mut s, ids, root) = nested();
    let locked: Vec<_> = ids[..2].iter().map(|id| s.active_comp().unwrap().layer(LayerId(*id)).unwrap().clone()).collect();
    s.execute("layer.timing", json!({"layers":ids,"delta":1})).unwrap();
    for layer in &locked {
        assert_eq!(s.active_comp().unwrap().layer(layer.id), Some(layer));
    }
    assert_eq!(s.active_comp().unwrap().layer(LayerId(ids[2])).unwrap().start_time.seconds(), 1.0);
    s.execute("edit.selectAll", json!({})).unwrap();
    assert_eq!(s.state.selected_layers, vec![LayerId(ids[2])]);
    s.execute("layer.groupUpdate", json!({"group":root,"locked":false})).unwrap();
    let c = s.active_comp().unwrap();
    assert!(!c.layer(LayerId(ids[0])).unwrap().switches.locked);
    assert!(!c.layer_effectively_locked(LayerId(ids[0])));
    assert!(c.layer(LayerId(ids[1])).unwrap().switches.locked);
    assert!(c.layer_effectively_locked(LayerId(ids[1])));
    s.execute("layer.timing", json!({"layers":[ids[0]],"delta":1})).unwrap();
    assert_eq!(s.active_comp().unwrap().layer(LayerId(ids[0])).unwrap().start_time.seconds(), 1.0);
    s.execute("edit.selectAll", json!({})).unwrap();
    assert_eq!(s.state.selected_layers, vec![LayerId(ids[0]), LayerId(ids[2])]);
}
