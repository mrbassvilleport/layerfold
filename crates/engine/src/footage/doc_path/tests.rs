use std::sync::{Arc, Mutex};

use effectcraft_project::{Footage, ItemId, ItemKind};
use serde_json::json;

use crate::{Event, Services, Session};

/// File access that records every path it is asked about.
#[derive(Default)]
struct Recording(Mutex<Vec<String>>);

/// The address the tests store as "another computer" (TEST-NET-1, never routed).
const FAR_HOST: &str = "192.0.2.1";

impl Recording {
    fn note(&self, path: &str) {
        self.0.lock().unwrap().push(path.to_string());
    }
}

impl Services for Recording {
    fn read_file(&self, path: &str) -> std::io::Result<Vec<u8>> {
        self.note(path);
        // A regression would wait on the network: fail at once, the assertions report it.
        if path.contains(FAR_HOST) {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        std::fs::read(path)
    }
    fn write_file(&self, path: &str, data: &[u8]) -> std::io::Result<()> {
        std::fs::write(path, data)
    }
    fn exists(&self, path: &str) -> bool {
        self.note(path);
        !path.contains(FAR_HOST) && std::path::Path::new(path).exists()
    }
    fn stamp(&self, path: &str) -> Option<(u64, u128)> {
        self.note(path);
        if path.contains(FAR_HOST) {
            return None;
        }
        let meta = std::fs::metadata(path).ok()?;
        Some((meta.len(), meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos()))
    }
}

/// Probes every file as a still. Only the Unix part of the test below imports files.
#[cfg(unix)]
struct Stills;

#[cfg(unix)]
impl crate::Importer for Stills {
    fn probe(&self, path: &str) -> Result<Footage, String> {
        if !std::path::Path::new(path).is_file() {
            return Err(format!("{path}: no such file"));
        }
        Ok(Footage { path: path.into(), kind: effectcraft_project::FootageKind::Still, width: 64, height: 32, has_video: true, ..Default::default() })
    }
}

fn footage_id(s: &Session, name: &str) -> ItemId {
    s.project.items.values().find(|i| i.name == name).unwrap().id
}

fn footage<'a>(s: &'a Session, name: &str) -> &'a Footage {
    match &s.project.item(footage_id(s, name)).unwrap().kind {
        ItemKind::Footage(f) => f,
        _ => panic!("{name} is not footage"),
    }
}

/// A proxy path to another computer is refused like the item's own path: the render falls back
/// to the item, and nothing looks at the proxy file.
#[test]
fn proxy_on_another_computer_is_never_looked_at() {
    let root = std::env::temp_dir().join(format!("ec-doc-path-proxy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let far = r"\\192.0.2.1\share\proxy.png";
    let mut s = Session::default();
    let f = Footage { path: "clip.png".into(), width: 64, height: 48, has_video: true, ..Default::default() };
    let id = Arc::make_mut(&mut s.project).add_item("clip", crate::color::Label::None, None, ItemKind::Footage(f));
    let proxy = Footage { path: far.into(), width: 64, height: 48, has_video: true, ..Default::default() };
    Arc::make_mut(&mut s.project).items.get_mut(&id).unwrap().proxy = Some(Box::new(effectcraft_project::Proxy { footage: proxy, enabled: true }));
    let project = root.join("p.ecproj");
    s.execute("file.saveAs", json!({"path": project.to_string_lossy()})).unwrap();

    let rec = Arc::new(Recording::default());
    let mut o = Session { services: rec.clone(), ..Default::default() };
    o.execute("file.open", json!({"path": project.to_string_lossy()})).unwrap();
    let opened = o.project.items.values().find(|i| i.name == "clip").unwrap();
    assert!(opened.proxy.as_ref().unwrap().footage.missing, "the refused proxy is marked missing");
    assert!(o.stored_paths.follow(far).is_err(), "the path stays refused for later checks");
    o.execute("footage.check", json!({"wait": true})).unwrap();
    let seen = rec.0.lock().unwrap().clone();
    assert!(!seen.iter().any(|p| p.contains("192.0.2.1")), "{seen:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A session holding footage on another computer, as a template or a timeline file would store it.
fn with_far_footage(far: &str) -> Session {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 64, "height": 48, "frameRate": 25, "duration": 1})).unwrap();
    let f = Footage { path: far.into(), width: 64, height: 48, has_video: true, duration: effectcraft_time::Tick::from_seconds_f64(1.0), ..Default::default() };
    let id = Arc::make_mut(&mut s.project).add_item("far", crate::color::Label::None, None, ItemKind::Footage(f));
    s.execute("layer.addItem", json!({"item": id.0})).unwrap();
    s
}

#[test]
fn imported_template_is_held_to_the_same_rule() {
    let far = r"\\192.0.2.1\share\clip.png";
    let dir = std::env::temp_dir().join(format!("ec-doc-path-template-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = with_far_footage(far);
    let comp = source.active_comp_id().unwrap().0;
    let manifest = json!({"format": crate::commands::essential::TEMPLATE_FORMAT, "version": 1, "comp": comp, "name": "T"});
    let entries = vec![("manifest.json".to_string(), manifest.to_string().into_bytes()), ("project.ecproj".to_string(), source.project.to_json().into_bytes())];
    let template = dir.join("T.ectemplate");
    std::fs::write(&template, effectcraft_lottie::zip::store(&entries)).unwrap();

    for (id, params) in [
        ("essential.importTemplate", json!({"path": template.to_string_lossy(), "addToComp": false})),
        ("templates.create", json!({"path": template.to_string_lossy()})),
    ] {
        let rec = Arc::new(Recording::default());
        let mut s = Session { services: rec.clone(), ..Default::default() };
        s.execute(id, params).unwrap();
        assert!(footage(&s, "far").missing, "{id}");
        assert!(s.stored_paths.follow(far).is_err(), "{id}");
        s.execute("footage.check", json!({"wait": true})).unwrap();
        let seen = rec.0.lock().unwrap().clone();
        assert!(!seen.iter().any(|p| p.contains("192.0.2.1")), "{id}: {seen:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn imported_timeline_is_held_to_the_same_rule() {
    let far = r"\\192.0.2.1\share\clip.mov";
    let dir = std::env::temp_dir().join(format!("ec-doc-path-timeline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Written by hand: the exporter refuses footage it can't find.
    let timeline = dir.join("t.otio");
    let otio = json!({
        "OTIO_SCHEMA": "Timeline.1", "name": "T", "tracks": {"OTIO_SCHEMA": "Stack.1", "name": "tracks", "children": [{
            "OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [{
                "OTIO_SCHEMA": "Clip.2", "name": "clip",
                "source_range": {"OTIO_SCHEMA": "TimeRange.1",
                    "start_time": {"OTIO_SCHEMA": "RationalTime.1", "rate": 25.0, "value": 0.0},
                    "duration": {"OTIO_SCHEMA": "RationalTime.1", "rate": 25.0, "value": 25.0}},
                "media_references": {"DEFAULT_MEDIA": {"OTIO_SCHEMA": "ExternalReference.1", "target_url": far}},
                "active_media_reference_key": "DEFAULT_MEDIA"
            }]
        }]}
    });
    std::fs::write(&timeline, otio.to_string()).unwrap();
    let rec = Arc::new(Recording::default());
    let mut s = Session { services: rec.clone(), ..Default::default() };
    let r = s.execute("file.importTimeline", json!({"path": timeline.to_string_lossy()})).unwrap();
    assert_eq!(r["missing"].as_array().map(Vec::len), Some(1), "the clip must reach the probe: {r}");
    let stored = r["missing"][0].as_str().unwrap().to_string();
    assert!(s.stored_paths.follow(&stored).is_err(), "{stored}");
    s.execute("footage.check", json!({"wait": true})).unwrap();
    let seen = rec.0.lock().unwrap().clone();
    assert!(!seen.iter().any(|p| p.contains("192.0.2.1")), "{seen:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Layer ▸ Create reads a vector or Photoshop file itself: footage marked missing (as a path on
/// another computer is when the project opens) is refused without asking for the file.
#[test]
fn layers_made_from_footage_on_another_computer_never_read_it() {
    let rec = Arc::new(Recording::default());
    let mut s = Session { services: rec.clone(), ..Default::default() };
    s.execute("comp.new", json!({"name": "Main", "width": 64, "height": 48, "frameRate": 25, "duration": 1})).unwrap();
    let add = |s: &mut Session, f: Footage| {
        let id = Arc::make_mut(&mut s.project).add_item("far", crate::color::Label::None, None, ItemKind::Footage(f));
        s.execute("layer.addItem", json!({"item": id.0})).unwrap()["layer"].as_u64().unwrap()
    };
    let base = Footage { width: 64, height: 48, has_video: true, missing: true, ..Default::default() };
    let svg = add(&mut s, Footage { path: r"\\192.0.2.1\share\logo.svg".into(), ..base.clone() });
    let e = s.execute("layer.create", json!({"op": "shapesFromVector", "layers": [svg]})).unwrap_err().to_string();
    assert!(e.contains("missing"), "{e}");
    let layer = effectcraft_project::SourceLayer { index: 0, name: "Title".into(), ..Default::default() };
    let psd = add(&mut s, Footage { path: r"\\192.0.2.1\share\title.psd".into(), layer: Some(layer), ..base });
    let e = s.execute("layer.create", json!({"op": "editableText", "layers": [psd]})).unwrap_err().to_string();
    assert!(e.contains("missing"), "{e}");
    // Not marked missing but stored by a document that may not follow it: refused as well.
    let far = r"\\192.0.2.1\share\badge.svg";
    let doc = std::env::temp_dir().join("ec-doc-path-create").join("p.ecproj");
    s.stored_paths.add_path(far, Some(&doc.to_string_lossy()));
    assert!(s.stored_paths.follow(far).is_err(), "the fixture is a refused stored path");
    let svg = add(&mut s, Footage { path: far.into(), width: 64, height: 48, has_video: true, ..Default::default() });
    let e = s.execute("layer.create", json!({"op": "shapesFromVector", "layers": [svg]})).unwrap_err().to_string();
    assert!(e.contains("not opened"), "{e}");
    let seen = rec.0.lock().unwrap().clone();
    assert!(!seen.iter().any(|p| p.contains("192.0.2.1")), "{seen:?}");
}

/// Footage paths to other computers stored in a project are never looked at: not on open, by
/// the footage check, by the changed-on-disk scan, by Edit Original or by Reveal in Finder. A
/// relative path is taken from the project's folder, and a file the user imports in this session
/// is used as given until the project is saved and opened again.
#[test]
fn footage_on_other_computers_is_never_looked_at() {
    let root = std::env::temp_dir().join(format!("ec-doc-path-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("job");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(root.join("media")).unwrap();
    std::fs::write(dir.join("clip.png"), b"x").unwrap();
    std::fs::write(root.join("media").join("far.png"), b"x").unwrap();
    let mut stored = vec![("unc", r"\\192.0.2.1\share\x.png".to_string()), ("url", "file://192.0.2.1/s/x.png".to_string()), ("beside", "clip.png".to_string())];
    // Linux reads `//tmp/…/far.png` as the local file: a sink that skipped the check would find it.
    if cfg!(unix) {
        stored.push(("slashes", format!("/{}", root.join("media").join("far.png").display())));
    }
    let mut s = Session::default();
    for (name, path) in &stored {
        let f = Footage { path: path.clone(), width: 64, height: 48, has_video: true, ..Default::default() };
        Arc::make_mut(&mut s.project).add_item(name, crate::color::Label::None, None, ItemKind::Footage(f));
    }
    let project = dir.join("p.ecproj");
    s.execute("file.saveAs", json!({"path": project.to_string_lossy()})).unwrap();

    let rec = Arc::new(Recording::default());
    let mut o = Session { services: rec.clone(), ..Default::default() };
    o.execute("file.open", json!({"path": project.to_string_lossy()})).unwrap();
    for (name, _) in &stored {
        assert_eq!(footage(&o, name).missing, *name != "beside", "{name}");
    }
    assert_eq!(std::path::Path::new(&footage(&o, "beside").path), dir.join("clip.png"));
    let r = o.execute("footage.check", json!({"wait": true})).unwrap();
    assert_eq!(r["missing"].as_u64(), Some(stored.len() as u64 - 1), "{r}");
    o.execute("footage.reloadChanged", json!({})).unwrap();
    o.drain_events();
    for (name, _) in &stored {
        o.state.project_selection = vec![footage_id(&o, name)];
        let opened = o.execute("edit.editOriginal", json!({}));
        let revealed = o.execute("file.revealInFinder", json!({}));
        assert_eq!((opened.is_ok(), revealed.is_ok()), (*name == "beside", *name == "beside"), "{name}: {opened:?} {revealed:?}");
    }
    let urls: Vec<String> = o.drain_events().into_iter().filter_map(|e| if let Event::OpenUrl(u) = e { Some(u) } else { None }).collect();
    assert_eq!(urls.len(), 2, "{urls:?}");

    let seen = rec.0.lock().unwrap().clone();
    assert!(seen.iter().any(|p| std::path::Path::new(p) == dir.join("clip.png")), "{seen:?}");
    // Only files in the temporary folder (the project's folder, where moved footage is looked for).
    let local = root.to_string_lossy().into_owned();
    for p in &seen {
        assert!(p.starts_with(&local), "looked at {p}");
    }

    // Chosen in this session, the same file is followed as given (as a file on a NAS would be)…
    #[cfg(unix)]
    {
        let far = stored.iter().find(|(n, _)| *n == "slashes").unwrap().1.clone();
        o.importer = Some(Arc::new(Stills));
        o.execute("file.import", json!({"paths": [far.clone()]})).unwrap();
        let imported = |s: &Session| -> Vec<ItemId> {
            s.project.items.values().filter(|i| matches!(&i.kind, ItemKind::Footage(f) if f.path == far)).map(|i| i.id).collect()
        };
        let id = *imported(&o).iter().find(|id| matches!(&o.project.item(**id).unwrap().kind, ItemKind::Footage(f) if !f.missing)).unwrap();
        o.state.project_selection = vec![id];
        o.execute("edit.editOriginal", json!({})).unwrap();
        o.execute("file.revealInFinder", json!({})).unwrap();
        assert!(o.execute("footage.check", json!({"wait": true, "items": [id.0]})).unwrap()["missing"] == 0);
        // …and is a stored path again once the project is saved and opened.
        o.execute("file.save", json!({})).unwrap();
        let mut q = Session::default();
        q.execute("file.open", json!({"path": project.to_string_lossy()})).unwrap();
        let ids = imported(&q);
        assert_eq!(ids.len(), 2);
        for id in ids {
            assert!(matches!(&q.project.item(id).unwrap().kind, ItemKind::Footage(f) if f.missing));
            q.state.project_selection = vec![id];
            assert!(q.execute("edit.editOriginal", json!({})).is_err());
            assert!(q.execute("file.revealInFinder", json!({})).is_err());
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn stored_paths_resolve_against_the_project() {
    let doc = std::env::temp_dir().join("art").join("intro.ecproj");
    let doc = doc.to_str().unwrap();
    assert_eq!(super::resolve("clip.mov", Some(doc)), Ok(std::env::temp_dir().join("art").join("clip.mov")));
    assert_eq!(super::resolve("clip.mov", None), Err(super::NotFollowed::NoProjectFolder));
    assert_eq!(super::resolve("../logo.png", None), Err(super::NotFollowed::NoProjectFolder));
    // A drive path is not a local file on Unix, with or without a project: it is not a missing project.
    #[cfg(unix)]
    for stored in [r"C:\x.mov", "C:x.mov"] {
        assert_eq!(super::resolve(stored, None), Err(super::NotFollowed::Refused(super::Refused::Invalid)), "{stored}");
    }
    assert_eq!(super::resolve(r"\\192.0.2.1\share\x.mov", Some(doc)), Err(super::NotFollowed::Refused(super::Refused::Network)));
    assert_eq!(super::resolve("jar:clip.mov", Some(doc)), Err(super::NotFollowed::Refused(super::Refused::Url)));
    // Windows reads a colon in a name as an alternate data stream; elsewhere it is a plain character.
    if cfg!(windows) {
        assert_eq!(super::resolve("shots/take:1.mov", Some(doc)), Err(super::NotFollowed::Refused(super::Refused::Invalid)));
    } else {
        assert_eq!(super::resolve("shots/take:1.mov", Some(doc)), Ok(std::env::temp_dir().join("art").join("shots").join("take:1.mov")));
    }
    // A Windows drive path names no file on other platforms.
    #[cfg(unix)]
    assert_eq!(super::resolve(r"C:\art\clip.mov", Some(doc)), Err(super::NotFollowed::Refused(super::Refused::Invalid)));
    #[cfg(windows)]
    assert!(super::resolve(r"\\server\share\x.mov", Some(r"\\SERVER\share\job\intro.ecproj")).is_ok());
}

#[test]
fn reveal_deleted_file_in_existing_folder_offers_relink_guidance() {
    let dir = std::env::temp_dir().join(format!("ec-reveal-offline-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::default();
    let file = dir.join("deleted.png");
    let id = std::sync::Arc::make_mut(&mut s.project).add_item(
        "Deleted",
        Default::default(),
        None,
        ItemKind::Footage(Footage { path: file.to_string_lossy().into_owned(), ..Default::default() }),
    );
    let err = s.execute("file.revealInFinder", json!({"item": id.0})).unwrap_err().to_string();
    assert!(err.contains("deleted.png") && err.contains("Relink Missing Footage"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn relative_proxy_is_resolved_and_checked_next_to_the_project() {
    let dir = std::env::temp_dir().join(format!("ec-proxy-path-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("proxy.png"), b"x").unwrap();
    let mut s = Session::default();
    let id = Arc::make_mut(&mut s.project).add_item(
        "Plate",
        Default::default(),
        None,
        ItemKind::Footage(Footage { path: dir.join("proxy.png").to_string_lossy().into_owned(), width: 64, height: 32, ..Default::default() }),
    );
    Arc::make_mut(&mut s.project).item_mut(id).unwrap().proxy = Some(Box::new(effectcraft_project::Proxy {
        footage: Footage { path: "proxy.png".into(), width: 32, height: 16, ..Default::default() },
        enabled: true,
    }));
    let project = dir.join("project.ecproj");
    s.execute("file.saveAs", json!({"path": project})).unwrap();
    let mut reopened = Session::default();
    reopened.execute("file.open", json!({"path": project})).unwrap();
    let result = reopened.execute("footage.check", json!({"wait": true})).unwrap();
    assert_eq!(result, json!({"checked": 2, "missing": 0, "probed": 0}));
    assert_eq!(std::path::Path::new(&reopened.project.item(id).unwrap().proxy.as_ref().unwrap().footage.path), dir.join("proxy.png"));
    std::fs::remove_dir_all(dir).unwrap();
}
