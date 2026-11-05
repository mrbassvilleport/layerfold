use effectcraft_keyframe::{Keyframe, ShapePath, Value};
use effectcraft_time::{FrameRate, Tick};

use crate::build::{self, Ids};
use crate::{Comp, ItemKind, LayerSource, MaskMode, Project, Solid};

fn project_with_layer() -> (Project, crate::ItemId) {
    let mut p = Project::default();
    let comp = Comp::new(1920, 1080, FrameRate::FPS_29_97, Tick::from_seconds_f64(10.0));
    let cid = p.add_item("Main", effectcraft_color::Label::Sandstone, None, ItemKind::Comp(comp.clone().into()));
    let sid = p.add_item(
        "Red Solid",
        effectcraft_color::Label::Red,
        None,
        ItemKind::Solid(Solid { color: [1.0, 0.0, 0.0], width: 1920, height: 1080, pixel_aspect: 1.0 }),
    );
    let layer = build::layer(&mut p, &comp, "Red Solid 1", LayerSource::Solid { item: sid }, (1920, 1080), None);
    p.comp_mut(cid).unwrap().layers.push(layer);
    (p, cid)
}

#[test]
fn layer_folders_roundtrip_ignore_pixels_and_repair_without_reordering() {
    let (mut p, cid) = project_with_layer();
    let id = p.comp(cid).unwrap().layers[0].id;
    let plain = p.to_file_json().unwrap();
    assert!(!plain.contains("layer_groups"), "old projects need no new field");
    assert!(Project::from_json(&plain).unwrap().comp(cid).unwrap().layer_groups.is_empty());
    let before = p.comp(cid).unwrap().clone();
    p.comp_mut(cid).unwrap().layer_groups.push(crate::LayerGroup { id: 5000, name: "Folder".into(), collapsed: true, members: vec![id], ..Default::default() });
    let grouped = p.comp(cid).unwrap();
    assert!(grouped.same_pixels(&before));
    assert_eq!(grouped.pixel_form(), before.pixel_form());
    let text = p.to_file_json().unwrap();
    let reopened = Project::from_json(&text).unwrap();
    assert_eq!(reopened.comp(cid).unwrap().layer_groups, grouped.layer_groups);
    assert!(reopened.next_id > 5000);
    let malformed = p.comp_mut(cid).unwrap();
    malformed.layer_groups[0].name = "  ".into();
    malformed.layer_groups[0].members.extend([id, crate::LayerId(u64::MAX)]);
    malformed.layer_groups.push(malformed.layer_groups[0].clone());
    let (repaired, notes) = Project::from_json_repaired(&p.to_file_json().unwrap()).unwrap();
    assert!(notes.iter().any(|s| s.contains("layer groups")));
    let c = repaired.comp(cid).unwrap();
    assert!(c.layer_groups_are_valid());
    assert_eq!(c.layer_groups.len(), 1);
    assert_eq!(c.layer_groups[0].name, "Group");
    assert_eq!(c.layers, before.layers);
}

#[test]
fn invalid_folder_ids_cannot_exhaust_the_project_allocator() {
    for id in [u64::MAX, u64::MAX - 1, crate::MAX_GROUP_ID] {
        let (mut p, cid) = project_with_layer();
        let member = p.comp(cid).unwrap().layers[0].id;
        let next = p.next_id;
        p.comp_mut(cid).unwrap().layer_groups.push(crate::LayerGroup {
            id,
            name: "Broken".into(),
            collapsed: false,
            members: vec![member],
            ..Default::default()
        });
        let (mut loaded, notes) = Project::from_json_repaired(&p.to_file_json().unwrap()).unwrap();
        assert!(loaded.comp(cid).unwrap().layer_groups.is_empty());
        assert!(!notes.is_empty());
        assert_eq!(loaded.alloc(), next);
        assert_eq!(loaded.next_id, next + 1);
    }
}

#[test]
fn crossing_folder_memberships_repair_into_disjoint_contiguous_spans() {
    let (mut p, cid) = project_with_layer();
    let c = p.comp_mut(cid).unwrap();
    let template = c.layers[0].clone();
    c.layers = (100..105)
        .map(|id| {
            let mut l = template.clone();
            l.id = crate::LayerId(id);
            l
        })
        .collect();
    c.layer_groups = vec![
        crate::LayerGroup { id: 500, name: "First".into(), collapsed: false, members: vec![crate::LayerId(100), crate::LayerId(104)], ..Default::default() },
        crate::LayerGroup { id: 501, name: "Second".into(), collapsed: true, members: vec![crate::LayerId(101), crate::LayerId(103)], ..Default::default() },
    ];
    let before = c.layers.clone();
    assert!(c.normalize_layer_groups());
    assert!(c.layer_groups_are_valid());
    assert_eq!(c.layers, before);
    assert_eq!(c.layer_groups[0].members, vec![crate::LayerId(100)]);
    assert_eq!(c.layer_groups[1].members, vec![crate::LayerId(101), crate::LayerId(102), crate::LayerId(103)]);
    assert!(!c.normalize_layer_groups());
}

/// A folder's Video changes only its members' frames, so the cache keeps the rest (#1641); when
/// it also turns the only Solo on or off, every frame changes.
#[test]
fn folder_video_limits_the_changed_span_and_gates_solo() {
    let (mut p, cid) = project_with_layer();
    let comp = p.comp(cid).unwrap().clone();
    let LayerSource::Solid { item } = comp.layers[0].source else { panic!("solid") };
    let mut short = build::layer(&mut p, &comp, "Short", LayerSource::Solid { item }, (1920, 1080), None);
    short.in_point = Tick::from_seconds_f64(2.0);
    short.out_point = Tick::from_seconds_f64(4.0);
    let c = p.comp_mut(cid).unwrap();
    c.layers.insert(0, short);
    let id = c.layers[0].id;
    c.layer_groups = vec![
        crate::LayerGroup { id: 6000, name: "Outer".into(), members: vec![id], ..Default::default() },
        crate::LayerGroup { id: 6001, name: "Inner".into(), members: vec![id], parent: Some(6000), ..Default::default() },
    ];
    let shown = c.clone();
    for g in [0, 1] {
        let mut hidden = shown.clone();
        hidden.layer_groups[g].video = false;
        let (s, e) = hidden.changed_span(&shown).expect("a span, not the whole comp");
        assert!(s <= Tick::from_seconds_f64(2.0) && s > Tick::from_seconds_f64(1.9), "group {g}: starts at the member's in point");
        assert!(e >= Tick::from_seconds_f64(4.0) && e < Tick::from_seconds_f64(4.1), "group {g}: ends at its out point");
    }
    let mut soloed = shown.clone();
    soloed.layers[0].switches.solo = true;
    let mut hidden = soloed.clone();
    hidden.layer_groups[0].video = false;
    // Like a soloed layer with its own Video off, a Solo in a hidden folder still counts.
    assert!(soloed.any_solo() && hidden.any_solo(), "a Solo in a hidden folder counts");
    assert!(hidden.changed_span(&soloed).is_some(), "only the soloed layer's span changes");
}

#[test]
fn nested_layer_groups_roundtrip_gates_and_pixel_keys() {
    let (mut p, cid) = project_with_layer();
    let c = p.comp_mut(cid).unwrap();
    let id = c.layers[0].id;
    let switches = c.layers[0].switches.clone();
    c.layer_groups = vec![
        crate::LayerGroup { id: 5000, name: "Parent".into(), members: vec![id], ..Default::default() },
        crate::LayerGroup { id: 5001, name: "Child".into(), members: vec![id], parent: Some(5000), ..Default::default() },
    ];
    assert!(c.layer_groups_are_valid());
    assert_eq!(c.layer_group(id).unwrap().id, 5001);
    assert_eq!(c.layer_group_path(id).iter().map(|g| g.id).collect::<Vec<_>>(), [5000, 5001]);
    assert!(c.group_contains_group(5000, 5001));
    assert!(!c.group_contains_group(5000, 5000));
    assert_eq!(c.group_descendants(5000), [5000, 5001]);
    let enabled = c.clone();
    c.layer_groups[0].video = false;
    c.layer_groups[0].audio = false;
    c.layer_groups[0].locked = true;
    assert!(!c.layer_effectively_visible(id));
    assert!(!c.layer_effectively_audio(id));
    assert!(c.layer_effectively_locked(id));
    assert_eq!(c.layers[0].switches, switches);
    assert!(!c.same_pixels(&enabled));
    assert_ne!(c.pixel_form(), enabled.pixel_form());
    let muted = c.clone();
    c.layer_groups[0].name = "Renamed".into();
    c.layer_groups[0].collapsed = true;
    c.layer_groups[0].audio = true;
    c.layer_groups[0].locked = false;
    assert!(c.same_pixels(&muted));
    assert_eq!(c.pixel_form(), muted.pixel_form());
    let reopened = Project::from_json(&p.to_file_json().unwrap()).unwrap();
    assert_eq!(reopened.comp(cid).unwrap().layer_groups, p.comp(cid).unwrap().layer_groups);
    let old: crate::LayerGroup = serde_json::from_str(&format!(r#"{{"id":50,"name":"Old","members":[{}]}}"#, id.0)).unwrap();
    assert!(old.video && old.audio && !old.locked && old.parent.is_none());
}

#[test]
fn nested_layer_group_repair_bounds_cycles_depth_and_restores_parent_spans() {
    let (mut p, cid) = project_with_layer();
    let c = p.comp_mut(cid).unwrap();
    let id = c.layers[0].id;
    let mut extra = c.layers[0].clone();
    extra.id = crate::LayerId(id.0 + 1);
    c.layers.push(extra);
    let layers = c.layers.clone();
    c.layer_groups = (1..=40)
        .map(|n| crate::LayerGroup { id: 5000 + n, name: format!("Group {n}"), members: vec![id], parent: (n > 1).then_some(4999 + n), ..Default::default() })
        .collect();
    c.layer_groups[0].parent = Some(5002);
    c.layer_groups[2].parent = Some(u64::MAX);
    c.layer_groups[1].members.clear();
    assert!(!c.layer_groups_are_valid());
    assert!(c.normalize_layer_groups());
    assert!(c.layer_groups_are_valid());
    assert_eq!(c.layers, layers);
    assert!(c.layer_group_path(id).len() <= crate::MAX_GROUP_DEPTH);
    assert!(!c.normalize_layer_groups());
    c.layer_groups = (1..=33)
        .map(|n| crate::LayerGroup { id: 6000 + n, name: format!("Depth {n}"), members: vec![id], parent: (n > 1).then_some(5999 + n), ..Default::default() })
        .collect();
    let last = c.layer_groups.pop().unwrap();
    assert!(c.layer_groups_are_valid(), "32 folder levels are supported");
    assert_eq!(c.layer_group_path(id).len(), crate::MAX_GROUP_DEPTH);
    c.layer_groups.push(last);
    assert!(!c.layer_groups_are_valid(), "33 folder levels must be repaired");
    assert!(c.normalize_layer_groups());
    assert!(c.layer_groups_are_valid());
    assert_eq!(c.layers, layers);
}

#[test]
fn nested_layer_groups_expand_inserted_members_and_remap_parents() {
    let (mut p, cid) = project_with_layer();
    let c = p.comp_mut(cid).unwrap();
    let template = c.layers[0].clone();
    c.layers = (100..103)
        .map(|id| {
            let mut l = template.clone();
            l.id = crate::LayerId(id);
            l
        })
        .collect();
    c.layer_groups = vec![
        crate::LayerGroup { id: 5000, name: "Parent".into(), members: vec![crate::LayerId(100)], ..Default::default() },
        crate::LayerGroup { id: 5001, name: "Child".into(), parent: Some(5000), members: vec![crate::LayerId(100), crate::LayerId(102)], ..Default::default() },
    ];
    assert!(c.normalize_layer_groups());
    assert!(c.layer_groups_are_valid());
    for g in &c.layer_groups {
        assert_eq!(g.members, [crate::LayerId(100), crate::LayerId(101), crate::LayerId(102)]);
    }
    crate::essential::offset_ids(&mut p, 10000);
    let c = p.comp(crate::ItemId(cid.0 + 10000)).unwrap();
    assert!(c.layer_groups_are_valid());
    assert_eq!(c.layer_groups[1].parent, Some(15000));
    assert_eq!(c.layer_group(crate::LayerId(10100)).unwrap().id, 15001);
}

#[test]
fn paths_resolve() {
    let (p, cid) = project_with_layer();
    let l = &p.comp(cid).unwrap().layers[0];
    assert_eq!(l.props.prop("transform/position").unwrap().value, Value::Vec3([960.0, 540.0, 0.0]));
    assert_eq!(l.props.prop("Transform/Opacity").unwrap().value, Value::Scalar(100.0));
    assert_eq!(l.props.prop("transform/#7").unwrap().match_id, "rotation");
    let uid = l.props.prop("transform/scale").unwrap().uid;
    let path = l.props.path_of(uid).unwrap();
    assert_eq!(l.props.prop(&path).unwrap().match_id, "scale");
    assert_eq!(l.props.name_path_of(uid).unwrap(), "Transform/Scale");
}

#[test]
fn masks_and_occurrence_paths() {
    let (mut p, cid) = project_with_layer();
    let mut next = p.next_id;
    let m1 = build::mask(&mut Ids(&mut next), "Mask 1", ShapePath::rect([0.0, 0.0], 10.0, 10.0), MaskMode::Add, [255, 0, 0]);
    let m2 = build::mask(&mut Ids(&mut next), "Mask 2", ShapePath::ellipse([0.0, 0.0], 10.0, 10.0), MaskMode::Subtract, [0, 255, 0]);
    p.next_id = next;
    let l = &mut p.comp_mut(cid).unwrap().layers[0];
    let masks = l.props.sub_mut("masks").unwrap();
    masks.children.push(m1.into());
    masks.children.push(m2.into());
    assert_eq!(l.props.group("masks/mask#2").unwrap().name, "Mask 2");
    assert_eq!(l.props.group("masks/Mask 2").unwrap().name, "Mask 2");
    assert!(l.props.prop("masks/mask#2/feather").is_some());
}

#[test]
fn animate_and_roundtrip_json() {
    let (mut p, cid) = project_with_layer();
    {
        let l = &mut p.comp_mut(cid).unwrap().layers[0];
        let op = l.props.prop_mut("transform/opacity").unwrap();
        op.set_animated(true, Tick::ZERO);
        op.set_value_at(Tick::from_seconds_f64(1.0), Value::Scalar(0.0));
        assert_eq!(op.keys.len(), 2);
        assert_eq!(op.value_at(Tick::from_seconds_f64(0.5)), Value::Scalar(50.0));
    }
    let json = p.to_json();
    let q = Project::from_json(&json).unwrap();
    assert_eq!(p, q);
    let _ = Keyframe::new(Tick::ZERO, Value::Scalar(0.0));
}

#[test]
fn comp_resolution_is_saved_and_old_projects_load_with_auto() {
    use crate::Resolution;
    let (mut p, cid) = project_with_layer();
    // Auto (the default) isn't written, so older versions read the file as before.
    assert!(!p.to_file_json().unwrap().contains("\"resolution\""));
    p.comp_mut(cid).unwrap().resolution = Resolution::Custom(5);
    let q = Project::from_json(&p.to_file_json().unwrap()).unwrap();
    assert_eq!(q.comp(cid).unwrap().resolution, Resolution::Custom(5));
    p.comp_mut(cid).unwrap().resolution = Resolution::Half;
    let text = p.to_file_json().unwrap();
    assert_eq!(Project::from_json(&text).unwrap().comp(cid).unwrap().resolution, Resolution::Half);
    // A project saved before comps had a resolution opens with Auto.
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                m.remove("resolution");
                m.values_mut().for_each(strip);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut old: serde_json::Value = serde_json::from_str(&text).unwrap();
    strip(&mut old);
    assert_eq!(Project::from_json(&old.to_string()).unwrap().comp(cid).unwrap().resolution, Resolution::Auto);
    // It's not part of the comp's pixels.
    let (a, b) = (p.comp(cid).unwrap(), q.comp(cid).unwrap());
    assert!(a != b && a.same_pixels(b));
    assert_eq!(format!("{:?}", a.pixel_form()), format!("{:?}", b.pixel_form()));
}

#[test]
fn resolution_factors_and_scales() {
    use crate::Resolution;
    assert_eq!(Resolution::from_factor(1), Resolution::Full);
    assert_eq!(Resolution::from_factor(4), Resolution::Quarter);
    assert_eq!(Resolution::from_factor(0), Resolution::Full);
    assert_eq!(Resolution::from_factor(7), Resolution::Custom(7));
    assert_eq!(Resolution::from_factor(1000), Resolution::Custom(40));
    assert_eq!(Resolution::Custom(5).factor(), Some(5));
    assert_eq!(Resolution::Auto.factor(), None);
    // A hostile factor from a file renders within the Custom range.
    assert_eq!(Resolution::Custom(0).scale(1.0, 1.0), 1.0);
    assert_eq!(Resolution::Custom(255).scale(1.0, 1.0), 1.0 / 40.0);
    assert_eq!(Resolution::Third.scale(0.1, 1.0), 1.0 / 3.0);
}

#[test]
fn unique_names_and_cycles() {
    let (p, cid) = project_with_layer();
    let c = p.comp(cid).unwrap();
    assert_eq!(c.unique_layer_name("Red Solid 1"), "Red Solid 2");
    assert_eq!(c.unique_layer_name("Shape Layer 1"), "Shape Layer 1");
    assert!(p.comp_contains(cid, cid));
}

#[test]
fn camera_and_light_layers_build() {
    let (mut p, cid) = project_with_layer();
    let comp = p.comp(cid).unwrap().clone();
    let cam = build::layer(&mut p, &comp, "Camera 1", LayerSource::Camera, (0, 0), None);
    assert!(cam.props.prop("cameraOptions/zoom").is_some());
    assert!(cam.props.prop("transform/poi").is_some());
    let light = build::layer(&mut p, &comp, "Light 1", LayerSource::Light { kind: crate::LightKind::Spot }, (0, 0), None);
    assert!(light.props.prop("lightOptions/coneAngle").is_some());
}

#[test]
fn same_content_sees_allocated_ids_and_comp_edits() {
    let (p, cid) = project_with_layer();
    let mut q = p.clone();
    assert!(q.same_content(&p));
    q.alloc();
    assert!(!q.same_content(&p), "an allocated id stays taken");
    let mut q = p.clone();
    // A comp copied on write but left equal is the same; a changed one is not.
    let _ = q.comp_mut(cid).unwrap();
    assert!(q.same_content(&p));
    q.comp_mut(cid).unwrap().layers[0].name = "Renamed".into();
    assert!(!q.same_content(&p));
    let mut r = p.clone();
    r.item_mut(cid).unwrap().name = "Other".into();
    assert!(!r.same_content(&p));
    let mut r = p.clone();
    r.settings.bit_depth = crate::BitDepth::Bpc16;
    assert!(!r.same_content(&p));
}

#[test]
fn project_files_name_their_version_and_always_read_back() {
    let (mut p, cid) = project_with_layer();
    let text = p.to_file_json().unwrap();
    assert_eq!(crate::saved_by(&text).as_deref(), Some(crate::APP_VERSION));
    assert!(text.trim_start().starts_with("{\n  \"savedBy\""), "the version comes first");
    assert_eq!(Project::from_json(&text).unwrap(), p);
    assert_eq!(crate::saved_by(&p.to_json()), None, "plain JSON (worker hand-offs) has none");
    assert_eq!(crate::saved_by(r#"{"savedBy": "999.0.0", "schema": 3}"#).as_deref(), Some("999.0.0"), "a newer schema still names its version");
    assert_eq!(crate::saved_by(r#"[2, {}, {}, 1]"#), None, "the array form has none");
    // JSON can't hold NaN: saving refuses instead of writing a file that can't be opened.
    p.comp_mut(cid).unwrap().layers[0].props.prop_mut("transform/opacity").unwrap().value = Value::Scalar(f64::NAN);
    let e = p.to_file_json().unwrap_err().to_string();
    assert!(e.contains("can't be saved"), "{e}");
    // Infinity too, in either profile: the check doesn't rely on reading the file back.
    p.comp_mut(cid).unwrap().layers[0].props.prop_mut("transform/opacity").unwrap().value = Value::Scalar(f64::INFINITY);
    let e = p.to_file_json().unwrap_err().to_string();
    assert!(e.contains("can't be saved"), "{e}");
    assert!(crate::is_newer_version("0.2.0", "0.1.9") && crate::is_newer_version("1.0.0-rc.1", "0.9.9"));
    assert!(!crate::is_newer_version("0.1.1", "0.1.1") && !crate::is_newer_version("0.1.0", "0.1.1"));
}

#[test]
fn project_files_refuse_newer_schemas() {
    let p = Project { schema: crate::SCHEMA_VERSION + 1, ..Project::default() };
    let expected = format!(
        "the project can't be saved: it holds a value a project file can't store (project schema {} is newer than this version supports ({}))",
        p.schema,
        crate::SCHEMA_VERSION
    );
    assert_eq!(p.to_file_json().unwrap_err().to_string(), expected);
    assert_eq!(p.to_file_json_with_view(&crate::SavedView::default(), false).unwrap_err().to_string(), expected);
}

#[test]
fn saved_files_reopen_in_pretty_and_compact_form() {
    let (p, _cid) = project_with_layer();
    assert_eq!(Project::from_json(&p.to_file_json().unwrap()).unwrap(), p, "the pretty project file");
    assert_eq!(Project::from_json(&p.to_json_compact()).unwrap(), p, "compact JSON");
}

/// A project file read back and saved again is the same text, indented and compact (Save Compact
/// Project Files), with its saved view.
#[test]
fn project_files_save_again_byte_for_byte() {
    let (p, cid) = project_with_layer();
    let view = crate::SavedView { active_comp: Some(cid), open_comps: vec![cid], comp_times: [(cid, effectcraft_time::Tick(4_237_833_600))].into() };
    for compact in [false, true] {
        let text = p.to_file_json_with_view(&view, compact).unwrap();
        let prefix = if compact { "{\"savedBy\":\"" } else { "{\n  \"savedBy\": \"" };
        assert!(text.starts_with(&format!("{prefix}{}\"", crate::APP_VERSION)), "compact {compact}: {text}");
        assert_eq!(text.contains('\n'), !compact);
        let again = Project::from_json(&text).unwrap().to_file_json_with_view(&crate::saved_view(&text), compact).unwrap();
        assert_eq!(again, text, "compact {compact}");
    }
}

#[test]
fn project_files_remember_the_open_comps() {
    let (p, cid) = project_with_layer();
    let view = crate::SavedView { active_comp: Some(cid), open_comps: vec![cid], comp_times: [(cid, effectcraft_time::Tick(4_237_833_600))].into() };
    let text = p.to_file_json_with_view(&view, false).unwrap();
    assert_eq!(crate::saved_view(&text), view);
    assert_eq!(Project::from_json(&text).unwrap(), p, "the view is not part of the project");
    // Files from before #748 have no view; a garbled one is ignored, not an error.
    assert_eq!(crate::saved_view(&p.to_file_json().unwrap()), crate::SavedView::default());
    let odd = text.replacen(&format!("\"activeComp\": {}", cid.0), "\"activeComp\": \"Main\"", 1);
    assert_eq!(crate::saved_view(&odd), crate::SavedView { active_comp: None, ..view.clone() });
    // Entries that aren't an id and a whole tick count are skipped; the rest are kept.
    let times = text.replacen("\"compTimes\": {", "\"compTimes\": {\"x\": 1, \"7\": \"a\", \"8\": 1.5, \"9\": -4,", 1);
    let expect = [(cid, effectcraft_time::Tick(4_237_833_600)), (crate::ItemId(9), effectcraft_time::Tick(-4))].into();
    assert_eq!(crate::saved_view(&times).comp_times, expect);
    let not_a_map = text.replacen("\"compTimes\": {", "\"compTimes\": [], \"unused\": {", 1);
    assert_eq!(crate::saved_view(&not_a_map), crate::SavedView { comp_times: Default::default(), ..view });
}

/// The Project panel's "used N times": layers in every comp that use the item directly, a precomp
/// counted by the comps it is nested in, and an unused item used by nothing.
#[test]
fn item_users_count_layers_in_every_comp() {
    let (mut p, main) = project_with_layer();
    let sid = p.find_by_name("Red Solid").unwrap().id;
    let comp = p.comp(main).unwrap().clone();
    let pre = p.add_item("A Precomp", effectcraft_color::Label::Sandstone, None, ItemKind::Comp(comp.clone().into()));
    let unused = p.add_item("Unused", effectcraft_color::Label::Red, None, ItemKind::Solid(Solid { color: [0.0; 3], width: 8, height: 8, pixel_aspect: 1.0 }));
    // Two more layers of the solid in the precomp (it starts with one, cloned from Main), and the
    // precomp nested in Main.
    for _ in 0..2 {
        let l = build::layer(&mut p, &comp, "Red Solid", LayerSource::Solid { item: sid }, (1920, 1080), None);
        p.comp_mut(pre).unwrap().layers.push(l);
    }
    let nest = build::layer(&mut p, &comp, "A Precomp", LayerSource::Comp { item: pre }, (1920, 1080), None);
    p.comp_mut(main).unwrap().layers.push(nest);
    // Ordered by comp name: "A Precomp" before "Main".
    assert_eq!(p.item_users(sid), vec![(pre, 3), (main, 1)]);
    assert_eq!(p.item_users(pre), vec![(main, 1)]);
    assert!(p.item_users(unused).is_empty());
    assert!(p.item_users(main).is_empty());
}

#[test]
fn group_spans_merge_overlaps_and_keep_gaps() {
    use crate::{LayerGroup, LayerId};
    let (mut p, cid) = project_with_layer();
    let template = p.comp(cid).unwrap().layers[0].clone();
    let s = Tick::from_seconds_f64;
    let comp = p.comp_mut(cid).unwrap();
    comp.layers.clear();
    // A, B overlap; C touches B; in the pause E is zero length and F inverted (damaged file), so
    // neither may add a segment; D sits after the pause and forms the inner group.
    for (id, a, b) in [(1, 0.0, 2.0), (2, 1.5, 3.0), (3, 3.0, 3.5), (5, 4.0, 4.0), (6, 4.3, 3.8), (4, 4.5, 6.0)] {
        let mut l = template.clone();
        l.id = LayerId(id);
        l.in_point = s(a);
        l.out_point = s(b);
        comp.layers.push(l);
    }
    let ids = |v: &[u64]| v.iter().map(|i| LayerId(*i)).collect::<Vec<_>>();
    comp.layer_groups = vec![
        LayerGroup { id: 100, name: "G".into(), members: ids(&[1, 2, 3, 5, 6, 4]), ..Default::default() },
        LayerGroup { id: 101, name: "H".into(), members: ids(&[4]), parent: Some(100), ..Default::default() },
    ];
    let comp = p.comp(cid).unwrap();
    assert_eq!(comp.layer_group_spans(100, &[]), vec![(s(0.0), s(3.5)), (s(4.5), s(6.0))]);
    assert_eq!(comp.layer_group_spans(101, &[]), vec![(s(4.5), s(6.0))]);
    assert_eq!(comp.layer_group_spans(100, &[LayerId(4)]), vec![(s(0.0), s(3.5))]);
    assert!(comp.layer_group_spans(999, &[]).is_empty());
    let mut damaged = comp.clone();
    damaged.layer_groups[0].members.push(LayerId(77));
    assert_eq!(damaged.layer_group_spans(100, &[]), comp.layer_group_spans(100, &[]));
}

/// A project of `n` empty comps; `links` adds a precomp layer of comp `b` inside comp `a`
/// (indexes into the returned ids).
fn comp_graph(n: usize, links: &[(usize, usize)]) -> (Project, Vec<crate::ItemId>) {
    let mut p = Project::default();
    let ids: Vec<crate::ItemId> = (0..n)
        .map(|i| {
            p.add_item(
                &format!("C{i}"),
                effectcraft_color::Label::None,
                None,
                ItemKind::Comp(Comp::new(8, 8, FrameRate::FPS_25, Tick::from_seconds_f64(1.0)).into()),
            )
        })
        .collect();
    for &(a, b) in links {
        let outer = p.comp(ids[a]).unwrap().clone();
        let l = build::layer(&mut p, &outer, "pre", LayerSource::Comp { item: ids[b] }, (8, 8), None);
        let ItemKind::Comp(c) = &mut p.item_mut(ids[a]).unwrap().kind else { panic!("comp") };
        std::sync::Arc::make_mut(c).layers.push(l);
    }
    (p, ids)
}

#[test]
fn comp_contains_ends_on_cycles_long_chains_and_shared_precomps() {
    // A ⇄ B cycle, C unrelated: the walk used to recurse until the stack ran out (#1237).
    let (p, c) = comp_graph(3, &[(0, 1), (1, 0)]);
    assert!(!p.comp_contains(c[0], c[2]));
    assert!(p.comp_contains(c[0], c[1]) && p.comp_contains(c[1], c[0]));
    // A comp nesting itself.
    let (p, c) = comp_graph(2, &[(0, 0)]);
    assert!(!p.comp_contains(c[0], c[1]));
    assert!(p.comp_contains(c[0], c[0]));
    // A target reached through a cycle.
    let (p, c) = comp_graph(3, &[(0, 1), (1, 0), (1, 2)]);
    assert!(p.comp_contains(c[0], c[2]));
    assert!(!p.comp_contains(c[2], c[0]));
    // 40 levels of diamonds: 2^40 paths, each comp visited once.
    let mut links = vec![];
    for i in 0..40 {
        let (top, l, r, bottom) = (i * 3, i * 3 + 1, i * 3 + 2, i * 3 + 3);
        links.extend([(top, l), (top, r), (l, bottom), (r, bottom)]);
    }
    let (p, c) = comp_graph(121, &links);
    let t0 = std::time::Instant::now();
    assert!(p.comp_contains(c[0], c[120]));
    assert!(!p.comp_contains(c[120], c[0]));
    assert!(t0.elapsed() < std::time::Duration::from_secs(1), "{:?}", t0.elapsed());
    // A chain 20 000 comps deep (a deep recursion's stack frames).
    let n = 20_000;
    let (p, c) = comp_graph(n, &(0..n - 1).map(|i| (i, i + 1)).collect::<Vec<_>>());
    assert!(p.comp_contains(c[0], c[n - 1]));
    assert!(!p.comp_contains(c[n - 1], c[0]));
}

/// Interpret Footage ▸ Cineon Settings: the 10 Bit Black and White Points become the converted
/// points, Current Gamma 1 gives linear light, Highlight Rolloff 0 keeps over-range values and
/// a higher one ramps them off; the settings apply only to DPX and Cineon files, and projects
/// saved without them read the defaults.
#[test]
fn cineon_settings_convert_log_code_values() {
    use crate::{CineonSettings, CineonUnits, Footage};
    let code = |v: f32| v / 1023.0;
    let c = CineonSettings { log_conversion: true, rolloff: 0.0, ..Default::default() };
    assert!(c.convert(code(95.0)).abs() < 1e-6, "black point");
    assert!((c.convert(code(685.0)) - 1.0).abs() < 1e-5, "white point");
    // Code 400: (10^(-0.95) - 10^(-1.9667)) / (1 - 10^(-1.9667)) = 0.10251 linear, ^(1/1.7).
    assert!((c.convert(code(400.0)) - 0.26190).abs() < 1e-4, "{}", c.convert(code(400.0)));
    let linear = CineonSettings { gamma: 1.0, ..c };
    assert!((linear.convert(code(400.0)) - 0.10251).abs() < 1e-4, "{}", linear.convert(code(400.0)));
    assert!(c.convert(code(1023.0)) > 1.5, "over range kept: {}", c.convert(code(1023.0)));
    // The default rolloff (20) ramps the white point and above off, up to 1.
    let rolled = CineonSettings { rolloff: 20.0, ..c };
    let (w, mid, top) = (rolled.convert(code(685.0)), rolled.convert(code(720.0)), rolled.convert(code(1023.0)));
    assert!(w > 0.8 && w < mid && mid < top && top <= 1.0, "{w} {mid} {top}");
    let points = CineonSettings { converted_black: 64.0, converted_white: 940.0, ..c };
    assert!((points.convert(code(95.0)) - 64.0 / 1023.0).abs() < 1e-5);
    assert!((points.convert(code(685.0)) - 940.0 / 1023.0).abs() < 1e-5);
    // Units.
    assert!((CineonUnits::EightBit.from_ten_bit(1023.0) - 255.0).abs() < 1e-9);
    assert!((CineonUnits::Float.to_ten_bit(0.5) - 511.5).abs() < 1e-9);
    assert_eq!(CineonUnits::parse("8-bit"), Some(CineonUnits::EightBit));
    assert!(CineonUnits::ALL.iter().all(|u| CineonUnits::parse(u.id()) == Some(*u)));
    // DPX and Cineon files only, and only with Logarithmic Conversion on.
    let on = CineonSettings { log_conversion: true, ..Default::default() };
    let dpx = Footage { path: "/m/plate.0001.DPX".into(), cineon: on, ..Default::default() };
    assert!(dpx.cineon_applies());
    assert!(Footage { path: "/m/scan.cin".into(), cineon: on, ..Default::default() }.cineon_applies());
    assert!(!Footage { path: "/m/still.png".into(), cineon: on, ..Default::default() }.cineon_applies());
    assert!(!Footage { path: "/m/plate.dpx".into(), ..Default::default() }.cineon_applies());
    // Saved only when changed; read back, and absent in older projects.
    let plain = serde_json::to_value(Footage { path: "/m/a.dpx".into(), ..Default::default() }).unwrap();
    assert!(plain.get("cineon").is_none(), "{plain}");
    let saved = serde_json::to_string(&dpx).unwrap();
    assert_eq!(serde_json::from_str::<Footage>(&saved).unwrap().cineon, on);
    assert_eq!(serde_json::from_value::<Footage>(plain).unwrap().cineon, CineonSettings::default());
}

/// A Wiggle Paths 100 000 groups deep gets Spatial Phase on a 2 MiB stack.
#[test]
fn upgrading_deeply_nested_shape_groups_fits_a_small_stack() {
    let n = 100_000;
    let mut next = 1;
    let mut contents = build::shape_simple_op(&mut Ids(&mut next), "wiggle").unwrap();
    contents.children.retain(|c| c.match_id() != "spatialPhase");
    for _ in 0..n {
        let uid = Ids(&mut next).alloc();
        contents = crate::PropGroup::new(uid, "group", "Group 1").with(contents);
    }
    let h = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let added = build::upgrade_shape_contents(&mut Ids(&mut next), &mut contents);
            (added, contents)
        })
        .unwrap();
    let (added, mut g) = h.join().unwrap();
    assert!(added);
    // One level at a time: dropping the whole tree would recurse as deep as it is nested.
    let mut depth = 0;
    while g.match_id == "group" {
        let Some(crate::Node::Group(child)) = g.children.pop() else { panic!("a nested group") };
        g = child;
        depth += 1;
    }
    assert_eq!(depth, n);
    let ids: Vec<&str> = g.children.iter().map(|c| c.match_id()).collect();
    assert_eq!(ids, ["size", "detail", "points", "speed", "correlation", "phase", "spatialPhase", "seed"]);
}
