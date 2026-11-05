//! M13.6 panels and tools through the commands: Progress (jobs), Media Browser, Metadata,
//! Lumetri Scopes, the Footage panel's Overlay / Ripple Insert edits, Content-Aware Fill, Scene
//! Edit Detection, Auto-trace and Align Video to Data.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use effectcraft_color::Label;
use effectcraft_project::{Footage, FootageKind, ItemId, ItemKind, LayerId, LayerSource, build};
use effectcraft_raster::Image;
use effectcraft_raster::inpaint::psnr;
use effectcraft_render::FootageSource;
use effectcraft_time::{FrameRate, Tick};
use serde_json::{Value, json};

use crate::Session;

const FPS: u32 = 25;

/// Footage computed per frame.
struct Gen {
    make: Box<dyn Fn(u32) -> Image + Send + Sync>,
    cache: Mutex<HashMap<u32, Arc<Image>>>,
}

impl FootageSource for Gen {
    fn frame(&self, _: ItemId, _: &Footage, t: Tick) -> Option<Arc<Image>> {
        let f = (t.seconds() * FPS as f64).round().max(0.0) as u32;
        let mut c = self.cache.lock().unwrap();
        Some(c.entry(f).or_insert_with(|| Arc::new((self.make)(f))).clone())
    }
}

/// A comp `w × h`, `secs` long, with one footage layer drawn by `make` (returns session, comp,
/// layer, footage item).
fn setup(w: u32, h: u32, secs: f64, make: impl Fn(u32) -> Image + Send + Sync + 'static) -> (Session, ItemId, LayerId, ItemId) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Panels", "width": w, "height": h, "frameRate": FPS, "duration": secs})).unwrap();
    let cid = s.active_comp_id().unwrap();
    let (lid, item) = s
        .edit("clip", None, |p, _| {
            let footage = Footage {
                path: "synthetic.mov".into(),
                kind: FootageKind::Video,
                width: w,
                height: h,
                frame_rate: FrameRate::new(FPS as i64, 1),
                duration: Tick::from_seconds_f64(secs),
                has_video: true,
                codec: "SYN".into(),
                ..Default::default()
            };
            let item = p.add_item("clip", Label::Aqua, None, ItemKind::Footage(footage));
            let comp = p.comp(cid).unwrap().clone();
            let l = build::layer(p, &comp, "Clip", LayerSource::Footage { item }, (w, h), None);
            let id = l.id;
            p.comp_mut(cid).unwrap().layers.insert(0, l);
            Ok((id, item))
        })
        .unwrap();
    s.footage = Arc::new(Gen { make: Box::new(make), cache: Mutex::default() });
    s.execute("layer.select", json!({"layers": [lid.0]})).unwrap();
    (s, cid, lid, item)
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("ec-panels-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---------------------------------------------------------------- Progress

#[test]
fn progress_lists_and_cancels_jobs() {
    let mut s = Session::default();
    let r = s
        .spawn_task("test", "Endless job", false, |ctl| {
            let mut k = 0;
            while ctl.progress(k % 100, 100) {
                std::thread::sleep(std::time::Duration::from_millis(2));
                k += 1;
            }
            Err(crate::render_queue::CANCELLED.into())
        })
        .unwrap();
    let id = r["job"].as_str().unwrap().to_string();
    let listed = s.execute("jobs.list", json!({})).unwrap();
    let running = listed["running"].as_array().unwrap();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0]["id"], json!(id));
    assert_eq!(running[0]["label"], json!("Endless job"));
    assert_eq!(running[0]["cancellable"], json!(true));
    assert_eq!(s.execute("jobs.cancel", json!({"job": id})).unwrap()["cancelled"], json!(true));
    s.execute("jobs.wait", json!({})).unwrap();
    assert!(s.jobs().is_empty());
    assert_eq!(s.job_log.last().unwrap().status, "cancelled");
    // A finished job applies its edit (one undo step) and reports its result.
    let v = s
        .spawn_task("test", "Quick job", true, |ctl| {
            ctl.progress(1, 1);
            let apply: crate::jobs::Apply = Box::new(|s: &mut Session| {
                s.edit("Quick", None, |p, _| {
                    p.settings.comment = "done".into();
                    Ok(())
                })?;
                Ok(json!(42))
            });
            Ok(apply)
        })
        .unwrap();
    assert_eq!(v, json!(42));
    assert_eq!(s.project.settings.comment, "done");
    assert_eq!(s.job_log.last().unwrap().status, "done");
    assert!(s.undo());
    assert_eq!(s.project.settings.comment, "");
}

// ---------------------------------------------------------------- Media Browser & Metadata

#[test]
fn media_browser_and_metadata() {
    let dir = tmp("browser");
    std::fs::create_dir_all(dir.join("Footage")).unwrap();
    std::fs::write(dir.join("shot.mov"), b"0123").unwrap();
    std::fs::write(dir.join("data.csv"), "time,v\n0,1\n").unwrap();
    std::fs::write(dir.join("readme.txt"), "x").unwrap();
    let d = dir.to_string_lossy().to_string();
    let mut s = Session::default();
    let r = s.execute("mediaBrowser.go", json!({"path": d})).unwrap();
    let names: Vec<&str> = r["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Footage", "data.csv", "readme.txt", "shot.mov"]);
    assert_eq!(s.state.media_browser.folder.as_deref(), Some(d.as_str()));
    let r = s.execute("mediaBrowser.list", json!({"importableOnly": true})).unwrap();
    assert_eq!(r["entries"].as_array().unwrap().len(), 3);
    s.execute("mediaBrowser.addFavorite", json!({})).unwrap();
    assert_eq!(s.state.media_browser.favorites, vec![d.clone()]);
    s.execute("mediaBrowser.go", json!({"path": dir.join("Footage").to_string_lossy()})).unwrap();
    s.execute("mediaBrowser.go", json!({"path": ".."})).unwrap();
    assert_eq!(s.state.media_browser.folder.as_deref(), Some(d.as_str()));
    s.execute("mediaBrowser.removeFavorite", json!({"path": d})).unwrap();
    assert!(s.state.media_browser.favorites.is_empty());
    // Import a data file from the browser; its metadata reads back.
    let r = s.execute("mediaBrowser.import", json!({"paths": [dir.join("data.csv").to_string_lossy()]})).unwrap();
    let id = r["items"][0].as_u64().unwrap();
    let m = s.execute("item.metadata", json!({"item": id})).unwrap();
    assert_eq!(m["item"]["type"], json!("Data"));
    assert_eq!(m["item"]["codec"], json!("CSV"));
    assert_eq!(m["item"]["dataRows"], json!(1));
    assert!(m["item"]["modified"].as_str().unwrap().ends_with('Z'));
    assert_eq!(m["project"]["footage"], json!(1));
    // Editable comments: item (undoable) and project.
    s.execute("project.setComment", json!({"items": [id], "comment": "telemetry"})).unwrap();
    s.execute("project.setProjectComment", json!({"comment": "client cut"})).unwrap();
    let m = s.execute("item.metadata", json!({"item": id})).unwrap();
    assert_eq!(m["item"]["comment"], json!("telemetry"));
    assert_eq!(m["project"]["comment"], json!("client cut"));
    s.undo();
    assert_eq!(s.project.settings.comment, "");
    let fi = s.execute("mediaBrowser.fileInfo", json!({"path": dir.join("shot.mov").to_string_lossy()})).unwrap();
    assert_eq!(fi["size"], json!(4));
    assert_eq!(fi["kind"], json!("video"));
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---------------------------------------------------------------- Lumetri Scopes

#[test]
fn scopes_read_the_comp_frame() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"width": 64, "height": 48, "frameRate": 25, "duration": 1})).unwrap();
    s.execute("layer.newSolid", json!({"color": "#ff0000"})).unwrap();
    let v = s.execute("scopes.analyze", json!({"scope": "vectorscopeYuv", "size": 101})).unwrap();
    let st = effectcraft_raster::scopes::ColorStandard::Rec709;
    let [_, cb, cr] = st.ycbcr([1.0, 0.0, 0.0]);
    assert_eq!(v["peaks"][0], json!([((cb + 0.5) * 100.0).round() as u32, ((0.5 - cr) * 100.0).round() as u32]));
    let h = s.execute("scopes.analyze", json!({"scope": "histogram", "standard": "rec601"})).unwrap();
    let bars = &h["histogram"];
    assert_eq!(bars[0][255], json!(64 * 48));
    assert_eq!(bars[1][0], json!(64 * 48));
    assert!((h["luma"]["mean"].as_f64().unwrap() - 0.299).abs() < 1e-3);
    assert!(s.execute("scopes.analyze", json!({"scope": "bogus"})).is_err());
}

// ---------------------------------------------------------------- Footage panel

#[test]
fn footage_panel_overlay_keeps_full_length_past_the_comp_end() {
    let (mut s, cid, _clip, item) = setup(32, 24, 8.0, |_| Image::filled(32, 24, [0.5, 0.5, 0.5, 1.0]));
    s.execute("footage.open", json!({"item": item.0})).unwrap();
    let r = s.execute("footage.overlayEdit", json!({"item": item.0, "in": 0.0, "out": 4.0, "at": 6.0})).unwrap();
    let ov = LayerId(r["layer"].as_u64().unwrap());
    let l = s.project.comp(cid).unwrap().layer(ov).unwrap();
    assert_eq!((l.in_point.seconds(), l.out_point.seconds()), (6.0, 10.0));
}

#[test]
fn footage_panel_overlay_and_ripple_insert() {
    let (mut s, cid, clip, item) = setup(32, 24, 8.0, |_| Image::filled(32, 24, [0.5, 0.5, 0.5, 1.0]));
    s.execute("footage.open", json!({"item": item.0})).unwrap();
    assert!(
        s.drain_events().iter().any(|e| matches!(e, crate::Event::Frontend { command, params } if command == "window.panel" && params["panel"] == "footage"))
    );
    s.execute("footage.setTime", json!({"time": 1.0})).unwrap();
    s.execute("footage.setIn", json!({})).unwrap();
    s.execute("footage.setOut", json!({"frame": 49})).unwrap();
    let info = s.execute("footage.info", json!({})).unwrap();
    assert_eq!(info["in"], json!(1.0));
    assert_eq!(info["out"], json!(2.0));
    // Overlay Edit at 0.6 s: a 1 s layer on top showing source 1–2 s.
    s.set_time(Tick::from_seconds_f64(0.6));
    let r = s.execute("footage.overlayEdit", json!({})).unwrap();
    let ov = LayerId(r["layer"].as_u64().unwrap());
    let comp = s.project.comp(cid).unwrap();
    assert_eq!(comp.layers[0].id, ov);
    let l = comp.layer(ov).unwrap();
    assert_eq!((l.in_point.seconds(), l.out_point.seconds(), l.start_time.seconds()), (0.6, 1.6, -0.4));
    assert_eq!(comp.layer(clip).unwrap().out_point.seconds(), 8.0, "overlay leaves other layers alone");
    // Ripple Insert at 3 s: layers crossing 3 s are split, the rest moves back by 1 s.
    s.set_time(Tick::from_seconds_f64(3.0));
    let r = s.execute("footage.rippleInsertEdit", json!({})).unwrap();
    let ins = LayerId(r["layer"].as_u64().unwrap());
    let comp = s.project.comp(cid).unwrap();
    assert_eq!(comp.layers.len(), 4, "overlay, insert, clip split in two");
    let l = comp.layer(ins).unwrap();
    assert_eq!((l.in_point.seconds(), l.out_point.seconds()), (3.0, 4.0));
    let head = comp.layer(clip).unwrap();
    assert_eq!((head.in_point.seconds(), head.out_point.seconds()), (0.0, 3.0));
    let tail = comp.layers.iter().find(|x| x.id != clip && x.id != ins && x.id != ov).unwrap();
    assert_eq!((tail.in_point.seconds(), tail.out_point.seconds(), tail.start_time.seconds()), (4.0, 9.0, 1.0));
    // Source time continuity: the tail shows source 3 s at comp 4 s.
    assert_eq!(tail.layer_time(Tick::from_seconds_f64(4.0)).seconds(), 3.0);
    // The overlay layer (in before 3 s, out before 3 s) didn't move.
    assert_eq!(comp.layer(ov).unwrap().in_point.seconds(), 0.6);
    // One undo step each.
    assert!(s.undo());
    assert_eq!(s.project.comp(cid).unwrap().layers.len(), 2);
    assert!(s.execute("footage.setOut", json!({"time": 0.5})).is_ok());
    assert_eq!(s.state.footage_panel.as_ref().unwrap().in_point, None, "an Out before the In clears the In");
}

// ---------------------------------------------------------------- Content-Aware Fill

fn bg(x: f32, y: f32) -> [f32; 4] {
    let s = 0.5 + 0.3 * (x * std::f32::consts::TAU / 10.0).sin();
    let n = effectcraft_raster::hash_noise(((x.floor() as i64).rem_euclid(20) / 4) as u32, ((y as u32) % 20) / 4, 3);
    [s, 0.3 + 0.4 * n, 0.6 - 0.3 * s * n, 1.0]
}

/// Panning textured background with a red "object" walking through the masked region.
fn caf_frame(f: u32, with_object: bool) -> Image {
    let (w, h) = (96u32, 64u32);
    let mut img = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut p = bg(x as f32 + 2.0 * f as f32, y as f32);
            let ox = 40 + (f % 4);
            if with_object && (ox..ox + 10).contains(&x) && (26..36).contains(&y) {
                p = [0.9, 0.1, 0.1, 1.0];
            }
            img.set(x, y, p);
        }
    }
    img
}

#[test]
fn content_aware_fill_removes_an_object() {
    let (mut s, cid, clip, _) = setup(96, 64, 0.48, |f| caf_frame(f, true));
    // Cut the object out with a Subtract mask; fill the work area (12 frames).
    s.execute("layer.addMask", json!({"rect": [36.0, 22.0, 20.0, 18.0], "mode": "subtract"})).unwrap();
    let out = tmp("caf");
    s.execute("contentFill.set", json!({"method": "object", "range": "workArea", "alphaExpansion": 1})).unwrap();
    let r = s.execute("contentFill.generate", json!({"outputDir": out.to_string_lossy(), "wait": true})).unwrap();
    let fill = LayerId(r["layer"].as_u64().unwrap());
    let files: Vec<String> = r["files"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert_eq!(files.len(), 12);
    // The Fill layer sits right above the source with the source's timing.
    let comp = s.project.comp(cid).unwrap();
    let i = comp.layers.iter().position(|l| l.id == fill).unwrap();
    assert_eq!(comp.layers[i + 1].id, clip);
    let fl = comp.layer(fill).unwrap();
    assert_eq!((fl.in_point, fl.out_point.seconds()), (Tick::ZERO, 0.48));
    let LayerSource::Footage { item } = fl.source else { panic!("footage") };
    let Some(ItemKind::Footage(f)) = s.project.item(item).map(|i| &i.kind) else { panic!() };
    assert_eq!((f.kind, f.sequence.len()), (FootageKind::Sequence, 12));
    assert!(s.project.item(item).unwrap().name.starts_with("Fill 1"));
    // The filled frames match the clean background inside the mask, and stay steady over time.
    let region: Vec<bool> = (0..96 * 64).map(|i| (36..56).contains(&(i % 96)) && (22..40).contains(&(i / 96))).collect();
    let mut prev: Option<Image> = None;
    for (k, path) in files.iter().enumerate() {
        let png = image::open(path).unwrap().to_rgba8();
        let img = Image::from_rgba8(96, 64, png.as_raw());
        let truth = caf_frame(k as u32, false);
        let db = psnr(&img, &truth, Some(&region));
        assert!(db > 25.0, "frame {k}: {db:.1} dB");
        // Outside the hole the frame is the source itself.
        let outside: Vec<bool> = region.iter().map(|r| !r).collect();
        assert!(psnr(&img, &caf_frame(k as u32, true), Some(&outside)) > 40.0);
        if let Some(p) = &prev {
            // Temporal consistency: the change in the hole follows the 2 px/frame pan (compare
            // against the previous fill shifted).
            let mut shifted = p.clone();
            for y in 0..64 {
                for x in 0..94 {
                    shifted.data[y * 96 + x] = p.data[y * 96 + x + 2];
                }
            }
            let inner: Vec<bool> = (0..96 * 64).map(|i| (38..54).contains(&(i % 96)) && (24..38).contains(&(i / 96))).collect();
            assert!(psnr(&img, &shifted, Some(&inner)) > 24.0, "frame {k} jumps");
        }
        prev = Some(img);
    }
    // One undo step removes the fill layer.
    assert!(s.undo());
    assert!(s.project.comp(cid).unwrap().layer(fill).is_none());
    std::fs::remove_dir_all(&out).unwrap();
    // Without a hole there is nothing to fill.
    let (mut s2, _, _, _) = setup(32, 24, 0.2, |_| Image::filled(32, 24, [0.5, 0.5, 0.5, 1.0]));
    assert!(s2.execute("contentFill.generate", json!({"wait": true, "outputDir": tmp("caf2").to_string_lossy()})).is_err());
}

#[test]
fn content_aware_fill_runs_in_the_background() {
    let (mut s, _, _, _) = setup(48, 32, 0.2, |f| {
        let mut img = Image::new(48, 32);
        for y in 0..32 {
            for x in 0..48 {
                img.set(x, y, bg(x as f32 + f as f32, y as f32));
            }
        }
        img
    });
    s.execute("layer.addMask", json!({"rect": [20.0, 10.0, 8.0, 8.0], "mode": "subtract"})).unwrap();
    let out = tmp("cafbg");
    let r = s.execute("contentFill.generate", json!({"method": "edgeBlend", "outputDir": out.to_string_lossy()})).unwrap();
    assert!(r["job"].as_str().unwrap().starts_with("task:"));
    assert_eq!(s.jobs().len(), 1);
    assert_eq!(s.jobs()[0].kind, "contentFill");
    s.wait_jobs();
    let rec = s.job_log.last().unwrap();
    assert_eq!(rec.status, "done", "{}", rec.message);
    assert_eq!(rec.result["frames"], json!(5));
    std::fs::remove_dir_all(&out).unwrap();
}

/// The browser: Content-Aware Fill goes to a job worker (its files written through the
/// worker's services), and the page adds the fill layer when the plan comes back.
#[test]
fn content_aware_fill_runs_in_a_job_worker() {
    #[derive(Default)]
    struct Manual(std::sync::Mutex<Vec<(crate::offload::WorkerRequest, Arc<crate::offload::Inbox>)>>);
    impl crate::offload::Offload for Manual {
        fn start(&self, req: crate::offload::WorkerRequest, inbox: Arc<crate::offload::Inbox>) -> std::result::Result<(), String> {
            self.0.lock().unwrap().push((req, inbox));
            Ok(())
        }
        fn cancel(&self, _: u64) {}
    }
    let (mut s, cid, clip, _) = setup(48, 32, 0.2, |f| {
        let mut img = Image::new(48, 32);
        for y in 0..32 {
            for x in 0..48 {
                img.set(x, y, bg(x as f32 + f as f32, y as f32));
            }
        }
        img
    });
    let off = Arc::new(Manual::default());
    s.offload = Some(off.clone());
    s.execute("layer.addMask", json!({"rect": [20.0, 10.0, 8.0, 8.0], "mode": "subtract"})).unwrap();
    let out = tmp("cafworker");
    let r = s.execute("contentFill.generate", json!({"method": "edgeBlend", "outputDir": out.to_string_lossy()})).unwrap();
    assert_eq!((r["job"].as_str(), r["worker"].as_bool()), (Some("contentFill"), Some(true)), "{r}");
    assert!(s.jobs().iter().any(|j| j.id == "contentFill"));
    let (req, inbox) = off.0.lock().unwrap().pop().unwrap();
    let back: crate::offload::WorkerRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
    // The worker: the same footage, the default (file system) services.
    let mut w = Session { footage: s.footage.clone(), ..Default::default() };
    let post: crate::offload::Post = std::rc::Rc::new(move |r| inbox.push(r));
    crate::offload::run_request(&mut w, back, &post);
    s.poll_offload();
    let comp = s.project.comp(cid).unwrap().clone();
    let i = comp.layers.iter().position(|l| l.id == clip).unwrap();
    assert!(i > 0, "a fill layer above the source");
    let fill = &comp.layers[i - 1];
    assert!(fill.name.starts_with("Fill 1"), "{}", fill.name);
    let LayerSource::Footage { item } = fill.source else { panic!("footage") };
    let Some(ItemKind::Footage(f)) = s.project.item(item).map(|i| &i.kind) else { panic!() };
    assert_eq!(f.sequence.len(), 5);
    assert!(f.sequence.iter().all(|p| std::path::Path::new(p).exists()));
    assert!(s.jobs().is_empty());
    // One undo step.
    assert!(s.undo());
    assert_eq!(s.project.comp(cid).unwrap().layers.len(), comp.layers.len() - 1);
    std::fs::remove_dir_all(&out).unwrap();
}

// ---------------------------------------------------------------- Scene Edit Detection

fn shot_frame(f: u32) -> Image {
    // Shots: frames 0–9, 10–17, 18–24.
    let (kind, k) = if f < 10 {
        (0, f)
    } else if f < 18 {
        (1, f - 10)
    } else {
        (2, f - 18)
    };
    let mut img = Image::new(80, 48);
    for y in 0..48 {
        for x in 0..80 {
            let fx = x as f32 + 2.0 * k as f32;
            let v = match kind {
                0 => 0.5 + 0.4 * (fx * 0.15).sin() * (y as f32 * 0.11).cos(),
                1 => {
                    if ((fx / 10.0).floor() as i32 + y as i32 / 10) % 2 == 0 {
                        0.85
                    } else {
                        0.15
                    }
                }
                _ => effectcraft_raster::hash_noise((fx.floor() as u32) / 4, y / 4, 5),
            };
            let c = match kind {
                0 => [v, v * 0.6, 0.2],
                1 => [0.1, v * 0.8, v],
                _ => [v * 0.9, v * 0.9, 0.5 + v * 0.4],
            };
            img.set(x, y, [c[0], c[1], c[2], 1.0]);
        }
    }
    img
}

/// Commands that add layers next to their source or change it refuse a source in a locked
/// folder, as `edit.split` does: no change and no undo step.
#[test]
fn folder_lock_blocks_create_trace_and_scene_split() {
    let (mut s, _, clip, _) = setup(80, 48, 1.0, shot_frame);
    let text = s.execute("layer.newText", json!({"text": "Hi", "size": 20})).unwrap()["layer"].as_u64().unwrap();
    let solid = s.execute("layer.newSolid", json!({"color": "#ffffff", "width": 20, "height": 20})).unwrap()["layer"].as_u64().unwrap();
    let group = s.execute("layer.group", json!({"layers": [clip.0, text, solid]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group": group, "locked": true})).unwrap();
    for (command, params) in [
        ("layer.create", json!({"op": "shapesFromText", "layers": [text]})),
        ("layer.autoTrace", json!({"layer": solid})),
        ("layer.sceneEditDetection", json!({"layer": clip.0, "mode": "split", "wait": true})),
    ] {
        let before = s.project.to_file_json().unwrap();
        let undo = s.history.undo.len();
        assert!(s.execute(command, params).is_err(), "{command}");
        assert_eq!(s.project.to_file_json().unwrap(), before, "{command}");
        assert_eq!(s.history.undo.len(), undo, "{command}");
    }
}

#[test]
fn folder_lock_blocks_analyses_mattes_inserts_and_deletes() {
    let (mut s, cid, clip, item) = setup(80, 48, 1.0, shot_frame);
    let id = |r: Value| LayerId(r["layer"].as_u64().unwrap());
    s.execute("layer.select", json!({"layers": [clip.0]})).unwrap();
    let cam = id(s.execute("layer.newCamera", json!({})).unwrap());
    let text = id(s.execute("layer.newText", json!({"text": "Hi", "size": 20})).unwrap());
    let outside = id(s.execute("layer.newSolid", json!({"color": "#ffffff", "width": 20, "height": 20})).unwrap());
    let order: Vec<LayerId> = s.project.comp(cid).unwrap().layers.iter().map(|l| l.id).collect();
    assert_eq!(order, vec![outside, text, cam, clip]);
    s.execute("layer.select", json!({"layers": [clip.0]})).unwrap();
    s.execute("track.new", json!({"kind": "transform"})).unwrap();
    s.execute("effect.apply", json!({"effect": "Warp Stabilizer"})).unwrap();
    s.execute("effect.apply", json!({"effect": "Roto Brush & Refine Edge"})).unwrap();
    s.execute("layer.addMask", json!({"layer": clip.0})).unwrap();
    s.execute("layer.setParent", json!({"layers": [text.0], "parent": outside.0})).unwrap();
    // A Type-tool layer left empty, still being edited when its folder is locked.
    s.execute("text.edit", json!({"layer": text.0, "created": true})).unwrap();
    s.execute("text.setSelection", json!({"select": "all"})).unwrap();
    s.execute("text.delete", json!({})).unwrap();
    let group = s.execute("layer.group", json!({"layers": [text.0, cam.0, clip.0]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group": group, "locked": true})).unwrap();
    let out = tmp("caf-locked");
    for (command, params) in [
        ("track.analyze", json!({"layer": clip.0, "wait": true})),
        ("warp.analyze", json!({"layer": clip.0, "wait": true})),
        ("track.mask", json!({"layer": clip.0, "wait": true})),
        ("roto.propagate", json!({"layer": clip.0, "wait": true})),
        ("roto.freeze", json!({"layer": clip.0, "wait": true})),
        ("contentFill.generate", json!({"layer": clip.0, "wait": true, "outputDir": out.to_string_lossy()})),
        ("camera.stereoRig", json!({})),
        ("footage.rippleInsertEdit", json!({"item": item.0, "at": 0.5})),
        ("layer.setTrackMatte", json!({"layer": outside.0, "matte": text.0})),
        ("layer.trackMatte", json!({"layers": [outside.0], "op": "below"})),
        ("edit.clear", json!({"layers": [outside.0]})),
        ("project.delete", json!({"items": [item.0]})),
    ] {
        let before = s.project.to_file_json().unwrap();
        let undo = s.history.undo.len();
        let err = s.execute(command, params).expect_err(command).to_string();
        assert!(err.to_lowercase().contains("lock"), "{command}: {err}");
        assert_eq!(s.project.to_file_json().unwrap(), before, "{command}");
        assert_eq!(s.history.undo.len(), undo, "{command}");
    }
    let undo = s.history.undo.len();
    s.execute("text.endEdit", json!({})).unwrap();
    assert!(s.project.comp(cid).unwrap().layer(text).is_some(), "the empty text layer stays in its locked folder");
    assert_eq!(s.history.undo.len(), undo);
}

#[test]
fn precompose_stays_inside_one_folder_level() {
    let (mut s, cid, clip, _) = setup(80, 48, 1.0, shot_frame);
    let id = |r: Value| r["layer"].as_u64().unwrap();
    s.execute("layer.select", json!({"layers": [clip.0]})).unwrap();
    let a = id(s.execute("layer.newSolid", json!({"color": "#ffffff"})).unwrap());
    let b = id(s.execute("layer.newSolid", json!({"color": "#ff0000"})).unwrap());
    let outside = id(s.execute("layer.newSolid", json!({"color": "#00ff00"})).unwrap());
    let group = s.execute("layer.group", json!({"layers": [b, a, clip.0]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group": group, "video": false, "audio": false})).unwrap();
    // Part inside the folder, part outside: refused.
    let before = s.project.to_file_json().unwrap();
    let err = s.execute("layer.precompose", json!({"layers": [outside, b], "name": "Mixed"})).unwrap_err().to_string();
    assert!(err.contains("layer groups"), "{err}");
    assert_eq!(s.project.to_file_json().unwrap(), before);
    // All inside one folder: the precomp layer takes their place in it, still hidden.
    let r = s.execute("layer.precompose", json!({"layers": [b, a], "name": "Pre"})).unwrap();
    let pre = LayerId(r["layer"].as_u64().unwrap());
    let outer = s.project.comp(cid).unwrap();
    assert!(outer.layer_groups_are_valid());
    assert_eq!(outer.layer_group(pre).map(|g| g.id), Some(group));
    assert_eq!(outer.layer_group(pre).unwrap().members, vec![pre, clip]);
    assert!(!outer.layer_effectively_visible(pre) && !outer.layer_effectively_audio(pre));
    let inner = s.project.comp(ItemId(r["comp"].as_u64().unwrap())).unwrap();
    assert!(inner.layer_groups.is_empty(), "no folder copies inside the precomp");
    assert!(inner.layers.iter().all(|l| l.switches.video), "the layers' own switches are untouched");
    // All outside folders: fine, and the precomp layer stays outside.
    let r = s.execute("layer.precompose", json!({"layers": [outside], "name": "Out"})).unwrap();
    let out = LayerId(r["layer"].as_u64().unwrap());
    assert!(s.project.comp(cid).unwrap().layer_group(out).is_none());
}

#[test]
fn folder_lock_protects_dependants_from_precompose_lift_and_clear() {
    let (mut s, _, clip, _) = setup(80, 48, 1.0, shot_frame);
    let id = |r: Value| r["layer"].as_u64().unwrap();
    s.execute("layer.select", json!({"layers": [clip.0]})).unwrap();
    let rig = id(s.execute("layer.newNull", json!({})).unwrap());
    let reference = id(s.execute("layer.newSolid", json!({"color": "#ffffff", "name": "Ref"})).unwrap());
    let logo = id(s.execute("layer.newSolid", json!({"color": "#ff0000", "name": "Logo"})).unwrap());
    let follower = id(s.execute("layer.newSolid", json!({"color": "#00ff00", "name": "Follower"})).unwrap());
    s.execute("layer.setParent", json!({"layers": [logo], "parent": rig})).unwrap();
    s.execute_checked(
        "prop.setExpression",
        json!({"layer": follower, "path": "transform/position", "expression": "thisComp.layer(\"Ref\").transform.position"}),
    )
    .unwrap();
    let group = s.execute("layer.group", json!({"layers": [follower, logo]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group": group, "locked": true})).unwrap();
    for (command, params) in [
        ("layer.precompose", json!({"layers": [rig], "name": "Pre"})),
        ("edit.liftWorkArea", json!({"layers": [rig]})),
        ("edit.extractWorkArea", json!({"layers": [rig]})),
        ("edit.clear", json!({"layers": [reference]})),
    ] {
        let before = s.project.to_file_json().unwrap();
        let undo = s.history.undo.len();
        let err = s.execute(command, params).expect_err(command).to_string();
        assert!(err.to_lowercase().contains("lock"), "{command}: {err}");
        assert_eq!(s.project.to_file_json().unwrap(), before, "{command}");
        assert_eq!(s.history.undo.len(), undo, "{command}");
    }
}

#[test]
fn scene_detection_keeps_a_clip_whose_folder_was_locked_meanwhile() {
    let (mut s, cid, clip, _) = setup(80, 48, 1.0, shot_frame);
    let group = s.execute("layer.group", json!({"layers": [clip.0]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.sceneEditDetection", json!({"layer": clip.0, "mode": "split"})).unwrap();
    s.execute("layer.groupUpdate", json!({"group": group, "locked": true})).unwrap();
    let before = s.project.to_file_json().unwrap();
    s.wait_jobs();
    assert_eq!(s.project.comp(cid).unwrap().layers.len(), 1);
    assert_eq!(s.project.to_file_json().unwrap(), before, "a locked folder keeps its clip whole");
}

#[test]
fn expression_layer_references_are_read_without_evaluating() {
    use crate::commands::{ExprComp as C, ExprLayer as L, ExprLayerRef as R, expr_layer_refs};
    let r = |comp, layer| R { comp, layer };
    let name = |n: &str| L::Name(n.into());
    assert_eq!(expr_layer_refs("thisComp.layer(\"Ref\").position"), vec![r(C::This, name("Ref"))]);
    assert_eq!(expr_layer_refs("thisComp.layer (\"Ref\").position"), vec![r(C::This, name("Ref"))], "space before the call");
    assert_eq!(expr_layer_refs("thisComp.layer(\"Say \\\"hi\\\"\")"), vec![r(C::This, name("Say \"hi\""))], "escapes");
    assert_eq!(expr_layer_refs("thisComp.layer(\"R\" + \"ef\")"), vec![r(C::This, L::Unknown)], "a computed name");
    assert_eq!(expr_layer_refs("comp('A').layer('B b')"), vec![r(C::Named("A".into()), L::Comp), r(C::Named("A".into()), name("B b"))]);
    assert_eq!(expr_layer_refs("var n = 1; comp(n).layer(2)"), vec![r(C::Unknown, L::Comp), r(C::Unknown, L::Unknown), r(C::Unknown, L::Index(2))]);
    assert_eq!(expr_layer_refs("var c = thisComp; c.layer(2)"), vec![r(C::This, L::Unknown), r(C::Unknown, L::Index(2))]);
    assert_eq!(expr_layer_refs("thisComp.layer( 3 ).position + thisComp.layer(index - 1).position"), vec![r(C::This, L::Index(3)), r(C::This, L::Unknown)]);
    assert_eq!(expr_layer_refs("var myLayer = thisLayer, layers = 2; myLayer.name + layers + value"), vec![]);
    assert_eq!(expr_layer_refs("thisComp.layer/*c*/(/*d*/\"Ref\" // e\n)"), vec![r(C::This, name("Ref"))], "comments");
    assert_eq!(expr_layer_refs("thisComp.layer(\"\\u0052ef\")"), vec![r(C::This, L::Unknown), r(C::Unknown, L::Unknown)], "an escaped name is unknown");
    let src = || C::Named("Source".into());
    assert_eq!(expr_layer_refs("comp(\"Source\")./*c*/layer(\"Ref\")"), vec![r(src(), L::Comp), r(src(), name("Ref"))], "a comment after the dot");
    assert_eq!(expr_layer_refs("comp(\"Source\")[\"layer\"](\"Ref\")"), vec![r(src(), L::Comp), r(src(), L::Unknown)], "bracket access");
    assert_eq!(expr_layer_refs("comp(\"Source\").numLayers"), vec![r(src(), L::Comp), r(src(), L::Count)]);
    assert_eq!(expr_layer_refs("var k = 0; thisComp.numLayers - thisComp[k]"), vec![r(C::This, L::Count), r(C::This, L::Unknown)]);
    assert_eq!(expr_layer_refs("var f = thisComp.layer; f(\"Ref\")"), vec![r(C::This, L::Unknown)], "an aliased method");
    assert_eq!(expr_layer_refs("/* thisComp.layer(\"Ref\") */ value // thisComp.layer(1)\n'thisComp.layer(2)'"), vec![], "comments and strings");
    assert_eq!(expr_layer_refs("[value[0], value[1]]"), vec![], "literal indexing");
    assert_eq!(expr_layer_refs("var x = [], i = 0; x[i]"), vec![r(C::Unknown, L::Unknown)], "a computed key may name any route");
}

#[test]
fn expression_layer_references_cover_regexes_aliases_and_computed_keys() {
    use crate::commands::{ExprComp as C, ExprLayer as L, ExprLayerRef as R, expr_layer_refs};
    let r = |comp, layer| R { comp, layer };
    let name = |n: &str| L::Name(n.into());
    let src = || C::Named("Source".into());
    // The sixth audit: regex literals, aliases, computed keys, the active camera.
    assert_eq!(expr_layer_refs("/[//]/.test(\"x\"); thisComp.layer(\"Ref\").position[0]"), vec![r(C::This, name("Ref"))], "a regex");
    assert_eq!(
        expr_layer_refs("thisComp.layer(1).position[0] / 2 / thisComp.layer(2).index"),
        vec![r(C::This, L::Index(1)), r(C::This, L::Index(2))],
        "division"
    );
    assert!(expr_layer_refs("var c = thisComp, k = \"layer\"; c[k](\"Ref\")").contains(&r(C::This, L::Unknown)), "an aliased comp");
    assert_eq!(expr_layer_refs("thisComp[\"lay\" + \"er\"](\"Ref\")"), vec![r(C::This, L::Unknown)], "a computed key");
    assert_eq!(expr_layer_refs("var f = comp; f(\"Source\").width"), vec![r(C::Unknown, L::Unknown)], "an aliased comp()");
    assert_eq!(
        expr_layer_refs("var s = comp(\"Source\"), k = 0; s[k]"),
        vec![r(src(), L::Comp), r(src(), L::Unknown), r(C::Unknown, L::Unknown)],
        "an aliased comp value"
    );
    assert_eq!(expr_layer_refs("thisComp.activeCamera.position"), vec![r(C::This, L::Unknown)]);
    assert_eq!(expr_layer_refs("var k = 0; thisLayer.source[k]"), vec![r(C::Unknown, L::Unknown)], "a precomp source");
    assert_eq!(expr_layer_refs("thisComp.width + thisLayer.source.width + value[\"x\"]"), vec![], "plain members");
}

#[test]
fn expression_layer_references_treat_hidden_routes_as_unknown() {
    use crate::commands::{ExprComp as C, ExprLayer as L, ExprLayerRef as R, expr_layer_refs};
    let r = |comp, layer| R { comp, layer };
    let name = |n: &str| L::Name(n.into());
    let any = |e: &str| expr_layer_refs(e).contains(&r(C::Unknown, L::Unknown));
    // The seventh audit: postfix increments, the global object, constructors.
    assert_eq!(expr_layer_refs("var x = 100; x++ / thisComp.layer(\"Ref\").index / 2"), vec![r(C::This, name("Ref"))], "x++ / y divides");
    assert!(any("var c = \"thisComp\", m = \"layer\"; globalThis[c][m](\"Ref\").index"), "the global object");
    assert!(any("\"\".constructor.constructor(\"return thisComp\")()"), "a constructor");
    assert!(any("/thisComp/.test(x)"), "a regex naming a route");
    // The eighth review: prelude internals, `yield /re/`, escaped names.
    assert!(any("new Layer(__S.c, __h(__LAYER, __S.c, 1)).transform.position"), "prelude internals");
    let refs = expr_layer_refs("function* g(){ yield /\"/; }\nthisComp.layer(1).transform.position;\n// \"");
    assert!(refs.contains(&r(C::This, L::Index(1))), "a regex after yield: {refs:?}");
    assert!(any("thisComp.\\u006cayer(1)"), "an escaped name");
    // The ninth review: an allowlist of free names; declarations shadow them.
    assert!(any("new Prop(3, 7, \"transform/position\").value"), "a prelude class");
    assert!(any("var {containingComp: C} = thisLayer, {layer: L} = C; L.call(C, 2).transform.position"), "destructuring");
    assert!(any("var C = thisComp.valueOf(); with (C) var L = layer; L.call(C, 2).transform.position"), "with");
    assert!(any("Object.getOwnPropertyDescriptor(Object.getPrototypeOf(thisLayer), \"containingComp\").get.call(thisLayer)"), "reflection");
    assert!(any("thisProject.item(1)"), "an unknown free name");
    assert_eq!(expr_layer_refs("wiggle(2, 30) + Math.sin(time) * effect(\"Slider\")(\"Slider\") + {a: 1, b: 2}.a"), vec![], "safe names");
    let own = "function f(a, b) { return a + b; } f(1, 2) + ((x) => x * 2)(3); for (var j = 0; j < 2; j++) {} try {} catch (e) {}";
    assert_eq!(expr_layer_refs(own), vec![], "declared names");
    // The tenth review: a declaration never hides a real global.
    assert!(any("var Object; Object.getPrototypeOf(thisLayer)"), "var Object");
    assert!(any("function f(thisProject) { return 1; } thisProject.numItems"), "a parameter named like a global");
}

/// Every expression runs in one JavaScript realm: one that may redefine the shared globals
/// (`effect = …`) makes what any locked expression reads unknown.
#[test]
fn an_expression_redefining_globals_protects_every_locked_expression() {
    let (mut s, cid, _, _) = setup(80, 48, 1.0, shot_frame);
    let id = |r: Value| r["layer"].as_u64().unwrap();
    let solid = |s: &mut Session, n: &str| id(s.execute("layer.newSolid", json!({"comp": cid.0, "color": "#ffffff", "name": n})).unwrap());
    let x = solid(&mut s, "X");
    let y = solid(&mut s, "Y");
    let u = solid(&mut s, "U");
    let l = solid(&mut s, "L");
    let set = |s: &mut Session, layer: u64, e: &str| {
        s.execute_checked("prop.setExpression", json!({"comp": cid.0, "layer": layer, "path": "transform/opacity", "expression": e})).unwrap();
    };
    set(&mut s, l, "effect(\"Slider\")(\"Slider\")");
    let group = s.execute("layer.group", json!({"comp": cid.0, "layers": [l]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"comp": cid.0, "group": group, "locked": true})).unwrap();
    // Nothing redefines `effect`: the locked layer reads only itself.
    s.execute("edit.clear", json!({"comp": cid.0, "layers": [y]})).unwrap();
    for e in ["effect = function () { return 0; }; value", "Math.sin = function () { return 0; }; value"] {
        set(&mut s, u, e);
        let err = s.execute("edit.clear", json!({"comp": cid.0, "layers": [x]})).unwrap_err().to_string();
        assert!(err.contains("locked"), "{e}: {err}");
    }
}

#[test]
fn removing_what_a_locked_layer_reads_through_others_is_refused() {
    let (mut s, cid, _, _) = setup(80, 48, 1.0, shot_frame);
    let id = |r: Value| r["layer"].as_u64().unwrap();
    let null = |s: &mut Session, n: &str| id(s.execute("layer.newNull", json!({"comp": cid.0, "name": n})).unwrap());
    let solid = |s: &mut Session, n: &str| id(s.execute("layer.newSolid", json!({"comp": cid.0, "color": "#ffffff", "name": n})).unwrap());
    // L is matted by M, M is parented to P.
    let p = null(&mut s, "P");
    let m = solid(&mut s, "M");
    let l = solid(&mut s, "L");
    s.execute("layer.setParent", json!({"comp": cid.0, "layers": [m], "parent": p})).unwrap();
    s.execute("layer.setTrackMatte", json!({"comp": cid.0, "layer": l, "matte": m, "kind": "alpha"})).unwrap();
    // F reads E's position, E is parented to Q.
    let q = null(&mut s, "Q");
    let e = solid(&mut s, "E");
    let f = solid(&mut s, "F");
    s.execute("layer.setParent", json!({"comp": cid.0, "layers": [e], "parent": q})).unwrap();
    s.execute_checked(
        "prop.setExpression",
        json!({"comp": cid.0, "layer": f, "path": "transform/position", "expression": "thisComp.layer(\"E\").toWorld([0, 0])"}),
    )
    .unwrap();
    let group = s.execute("layer.group", json!({"comp": cid.0, "layers": [l, f]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"comp": cid.0, "group": group, "locked": true})).unwrap();
    let before = s.project.to_file_json().unwrap();
    for gone in [p, q] {
        let err = s.execute("edit.clear", json!({"comp": cid.0, "layers": [gone]})).unwrap_err().to_string();
        assert!(err.contains("locked"), "{err}");
    }
    assert_eq!(s.project.to_file_json().unwrap(), before);
}

#[test]
fn reduce_project_keeps_a_comp_a_locked_layer_reads() {
    let (mut s, cid, _, _) = setup(80, 48, 1.0, shot_frame);
    s.execute("comp.new", json!({"name": "Source", "width": 80, "height": 48, "duration": 1})).unwrap();
    let source = s.active_comp_id().unwrap();
    s.execute("layer.newSolid", json!({"comp": source.0, "color": "#ffffff", "name": "Ref"})).unwrap();
    let follower = s.execute("layer.newSolid", json!({"comp": cid.0, "color": "#00ff00", "name": "Follower"})).unwrap()["layer"].as_u64().unwrap();
    let expr = "comp(\"Source\").layer(\"Ref\").transform.position";
    s.execute_checked("prop.setExpression", json!({"comp": cid.0, "layer": follower, "path": "transform/position", "expression": expr})).unwrap();
    let group = s.execute("layer.group", json!({"comp": cid.0, "layers": [follower]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"comp": cid.0, "group": group, "locked": true})).unwrap();
    s.state.project_selection = vec![cid];
    let err = s.execute("file.reduceProject", json!({})).unwrap_err().to_string();
    assert!(err.contains("locked"), "{err}");
    assert!(s.project.comp(source).is_some());
}

#[test]
fn a_layer_in_a_hidden_folder_is_not_tracked() {
    let (mut s, _, clip, _) = setup(80, 48, 1.0, shot_frame);
    let group = s.execute("layer.group", json!({"layers": [clip.0]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group": group, "video": false})).unwrap();
    s.execute("layer.select", json!({"layers": [clip.0]})).unwrap();
    let err = s.execute("track.new", json!({"layer": clip.0, "kind": "transform"})).unwrap_err().to_string();
    assert!(err.contains("no pixels"), "{err}");
}

#[test]
fn removing_a_layer_a_locked_layer_counts_is_refused() {
    let (mut s, cid, _, _) = setup(80, 48, 1.0, shot_frame);
    s.execute("comp.new", json!({"name": "Source", "width": 80, "height": 48, "duration": 1})).unwrap();
    let source = s.active_comp_id().unwrap();
    let any = s.execute("layer.newSolid", json!({"comp": source.0, "color": "#ffffff", "name": "Any"})).unwrap()["layer"].as_u64().unwrap();
    s.execute("comp.new", json!({"name": "Empty", "width": 80, "height": 48, "duration": 1})).unwrap();
    let empty = s.active_comp_id().unwrap();
    let follower = s.execute("layer.newSolid", json!({"comp": cid.0, "color": "#00ff00", "name": "Follower"})).unwrap()["layer"].as_u64().unwrap();
    s.execute_checked(
        "prop.setExpression",
        json!({"comp": cid.0, "layer": follower, "path": "transform/rotation", "expression": "comp(\"Source\").numLayers"}),
    )
    .unwrap();
    // An empty comp the locked layer reads can't be deleted either.
    s.execute_checked("prop.setExpression", json!({"comp": cid.0, "layer": follower, "path": "transform/opacity", "expression": "comp(\"Empty\").width"}))
        .unwrap();
    let group = s.execute("layer.group", json!({"comp": cid.0, "layers": [follower]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"comp": cid.0, "group": group, "locked": true})).unwrap();
    let err = s.execute("edit.clear", json!({"comp": source.0, "layers": [any]})).unwrap_err().to_string();
    assert!(err.contains("locked"), "{err}");
    let err = s.execute("project.delete", json!({"items": [empty.0]})).unwrap_err().to_string();
    assert!(err.contains("locked"), "{err}");
}

#[test]
fn removing_a_grandparent_of_a_locked_layer_is_refused() {
    let (mut s, cid, _, _) = setup(80, 48, 1.0, shot_frame);
    let id = |r: Value| r["layer"].as_u64().unwrap();
    let g = id(s.execute("layer.newNull", json!({"comp": cid.0, "name": "G"})).unwrap());
    let p = id(s.execute("layer.newNull", json!({"comp": cid.0, "name": "P"})).unwrap());
    let l = id(s.execute("layer.newSolid", json!({"comp": cid.0, "color": "#ffffff", "name": "L"})).unwrap());
    s.execute("layer.setParent", json!({"comp": cid.0, "layers": [p], "parent": g})).unwrap();
    s.execute("layer.setParent", json!({"comp": cid.0, "layers": [l], "parent": p})).unwrap();
    let group = s.execute("layer.group", json!({"comp": cid.0, "layers": [l]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"comp": cid.0, "group": group, "locked": true})).unwrap();
    let before = s.project.to_file_json().unwrap();
    let err = s.execute("edit.clear", json!({"comp": cid.0, "layers": [g]})).unwrap_err().to_string();
    assert!(err.contains("locked"), "{err}");
    assert_eq!(s.project.to_file_json().unwrap(), before);
}

#[test]
fn deleting_a_comp_a_locked_layer_reads_is_refused() {
    let (mut s, cid, _, _) = setup(80, 48, 1.0, shot_frame);
    s.execute("comp.new", json!({"name": "Source", "width": 80, "height": 48, "duration": 1})).unwrap();
    let source = s.active_comp_id().unwrap();
    s.execute("layer.newSolid", json!({"comp": source.0, "color": "#ffffff", "name": "Ref"})).unwrap();
    let follower = s.execute("layer.newSolid", json!({"comp": cid.0, "color": "#00ff00", "name": "Follower"})).unwrap()["layer"].as_u64().unwrap();
    let expr = "comp(\"Source\").layer(\"Ref\").transform.position";
    s.execute_checked("prop.setExpression", json!({"comp": cid.0, "layer": follower, "path": "transform/position", "expression": expr})).unwrap();
    let group = s.execute("layer.group", json!({"comp": cid.0, "layers": [follower]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"comp": cid.0, "group": group, "locked": true})).unwrap();
    let before = s.project.to_file_json().unwrap();
    let err = s.execute("project.delete", json!({"items": [source.0]})).unwrap_err().to_string();
    assert!(err.contains("locked"), "{err}");
    assert_eq!(s.project.to_file_json().unwrap(), before);
}

#[test]
fn split_and_precompose_is_one_step_even_with_one_undo_level() {
    let (mut s, cid, clip, _) = setup(80, 48, 1.0, shot_frame);
    s.prefs.general.undo_levels = 1;
    let before = s.project.to_file_json().unwrap();
    s.execute("layer.sceneEditDetection", json!({"layer": clip.0, "mode": "splitPrecompose", "wait": true})).unwrap();
    assert_eq!(s.project.comp(cid).unwrap().layers.len(), 3);
    assert!(s.undo());
    assert_eq!(s.project.to_file_json().unwrap(), before, "one undo restores the clip");
}

#[test]
fn expressions_in_other_comps_protect_the_layers_they_read() {
    let (mut s, cid, clip, _) = setup(80, 48, 1.0, shot_frame);
    let id = |r: Value| r["layer"].as_u64().unwrap();
    s.execute("layer.select", json!({"layers": [clip.0]})).unwrap();
    let reference = id(s.execute("layer.newSolid", json!({"color": "#ffffff", "name": "Ref"})).unwrap());
    let main = s.project.item(cid).unwrap().name.clone();
    s.execute("comp.new", json!({"name": "Other", "width": 80, "height": 48, "duration": 1})).unwrap();
    let other = s.active_comp_id().unwrap();
    let follower = id(s.execute("layer.newSolid", json!({"color": "#00ff00", "name": "Follower"})).unwrap());
    let expr = format!("comp(\"{main}\").layer (\"Ref\").transform.position");
    s.execute_checked("prop.setExpression", json!({"comp": other.0, "layer": follower, "path": "transform/position", "expression": expr})).unwrap();
    let group = s.execute("layer.group", json!({"comp": other.0, "layers": [follower]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"comp": other.0, "group": group, "locked": true})).unwrap();
    let err = s.execute("edit.clear", json!({"comp": cid.0, "layers": [reference]})).unwrap_err().to_string();
    assert!(err.contains("locked"), "{err}");
    // A layer of this comp named like the referenced one in the other comp is not protected.
    s.execute("layer.newSolid", json!({"comp": other.0, "color": "#ff0000", "name": "Ref"})).unwrap();
    let local = s.project.comp(other).unwrap().layers[0].id;
    s.execute("edit.clear", json!({"comp": other.0, "layers": [local.0]})).unwrap();
}

#[test]
fn layers_made_from_a_source_join_its_folder() {
    let (mut s, cid, clip, _) = setup(80, 48, 1.0, shot_frame);
    s.execute("layer.select", json!({"layers": [clip.0]})).unwrap();
    let text = s.execute("layer.newText", json!({"text": "Hi", "size": 20})).unwrap()["layer"].as_u64().unwrap();
    let group = s.execute("layer.group", json!({"layers": [text]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group": group, "video": false})).unwrap();
    let r = s.execute("layer.create", json!({"op": "shapesFromText", "layers": [text]})).unwrap();
    let c = s.project.comp(cid).unwrap();
    for v in r["layers"].as_array().unwrap() {
        let l = LayerId(v.as_u64().unwrap());
        assert_eq!(c.layer_group(l).map(|g| g.id), Some(group));
        assert!(!c.layer_effectively_visible(l));
    }
}

#[test]
fn scene_split_precompose_refuses_a_source_a_locked_folder_uses() {
    let (mut s, cid, clip, _) = setup(80, 48, 1.0, shot_frame);
    s.execute("layer.select", json!({"layers": [clip.0]})).unwrap();
    let child = s.execute("layer.newSolid", json!({"color": "#ffffff", "width": 20, "height": 20})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.setParent", json!({"layers": [child], "parent": clip.0})).unwrap();
    let group = s.execute("layer.group", json!({"layers": [child]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group": group, "locked": true})).unwrap();
    let before = s.project.to_file_json().unwrap();
    let undo = s.history.undo.len();
    assert!(s.execute("layer.sceneEditDetection", json!({"layer": clip.0, "mode": "splitPrecompose", "wait": true})).is_err());
    assert_eq!(s.project.to_file_json().unwrap(), before);
    assert_eq!(s.history.undo.len(), undo);
    assert_eq!(s.project.comp(cid).unwrap().layers.len(), 2);
}

#[test]
fn an_indexed_expression_protects_the_layers_it_can_shift() {
    let (mut s, _, clip, _) = setup(80, 48, 1.0, shot_frame);
    let id = |r: Value| r["layer"].as_u64().unwrap();
    s.execute("layer.select", json!({"layers": [clip.0]})).unwrap();
    let reference = id(s.execute("layer.newSolid", json!({"color": "#ffffff", "name": "Ref"})).unwrap());
    let follower = id(s.execute("layer.newSolid", json!({"color": "#00ff00", "name": "Follower"})).unwrap());
    // Follower is layer 1, Ref layer 2, Clip layer 3.
    s.execute_checked("prop.setExpression", json!({"layer": follower, "path": "transform/position", "expression": "thisComp.layer(2).transform.position"}))
        .unwrap();
    let group = s.execute("layer.group", json!({"layers": [follower]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group": group, "locked": true})).unwrap();
    let err = s.execute("edit.clear", json!({"layers": [reference]})).unwrap_err().to_string();
    assert!(err.contains("locked"), "{err}");
    // Layer 3 is below the reference: removing it shifts nothing the expression reads.
    s.execute("edit.clear", json!({"layers": [clip.0]})).unwrap();
}

#[test]
fn split_pieces_stay_in_their_folder() {
    let (mut s, cid, clip, item) = setup(80, 48, 1.0, shot_frame);
    let group = s.execute("layer.group", json!({"layers": [clip.0]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group": group, "video": false, "audio": false})).unwrap();
    let in_folder = |s: &Session, skip: &[LayerId]| {
        let c = s.project.comp(cid).unwrap();
        assert!(c.layer_groups_are_valid());
        for l in c.layers.iter().filter(|l| !skip.contains(&l.id)) {
            assert_eq!(c.layer_group(l.id).map(|g| g.id), Some(group), "{}", l.name);
            assert!(!c.layer_effectively_visible(l.id) && !c.layer_effectively_audio(l.id), "{}", l.name);
        }
        c.layers.len()
    };
    s.execute("layer.sceneEditDetection", json!({"layer": clip.0, "mode": "split", "wait": true})).unwrap();
    assert_eq!(in_folder(&s, &[]), 3, "Scene Edit Detection pieces");
    s.undo();
    let r = s.execute("footage.rippleInsertEdit", json!({"item": item.0, "at": 0.5})).unwrap();
    assert_eq!(in_folder(&s, &[LayerId(r["layer"].as_u64().unwrap())]), 3, "the ripple's split tail");
    s.undo();
    s.execute("comp.workArea", json!({"start": 0.3, "end": 0.6})).unwrap();
    s.execute("edit.liftWorkArea", json!({"layers": [clip.0]})).unwrap();
    assert_eq!(in_folder(&s, &[]), 2, "Lift's tail");
}

#[test]
fn ending_text_edit_keeps_the_parent_of_a_locked_layer() {
    let (mut s, cid, clip, _) = setup(80, 48, 1.0, shot_frame);
    let id = |r: Value| LayerId(r["layer"].as_u64().unwrap());
    s.execute("layer.select", json!({"layers": [clip.0]})).unwrap();
    let text = id(s.execute("layer.newText", json!({"text": "Hi", "size": 20})).unwrap());
    let child = id(s.execute("layer.newSolid", json!({"color": "#ffffff", "width": 20, "height": 20})).unwrap());
    s.execute("layer.setParent", json!({"layers": [child.0], "parent": text.0})).unwrap();
    s.execute("text.edit", json!({"layer": text.0, "created": true})).unwrap();
    s.execute("text.setSelection", json!({"select": "all"})).unwrap();
    s.execute("text.delete", json!({})).unwrap();
    let group = s.execute("layer.group", json!({"layers": [child.0]})).unwrap()["group"].as_u64().unwrap();
    s.execute("layer.groupUpdate", json!({"group": group, "locked": true})).unwrap();
    s.execute("text.endEdit", json!({})).unwrap();
    let c = s.project.comp(cid).unwrap();
    assert!(c.layer(text).is_some(), "the locked child's parent stays");
    assert_eq!(c.layer(child).unwrap().parent, Some(text));
}

#[test]
fn scene_edit_detection_finds_cuts() {
    let (mut s, cid, clip, _) = setup(80, 48, 1.0, shot_frame);
    // Markers.
    let r = s.execute("layer.sceneEditDetection", json!({"mode": "markers", "wait": true})).unwrap();
    assert_eq!(r["cuts"], json!([0.4, 0.72]));
    let l = s.project.comp(cid).unwrap().layer(clip).unwrap();
    assert_eq!(l.markers.iter().map(|m| m.time.seconds()).collect::<Vec<_>>(), vec![0.4, 0.72]);
    s.undo();
    // Split Layers.
    let r = s.execute("layer.sceneEditDetection", json!({"mode": "split", "wait": true})).unwrap();
    let ids: Vec<u64> = r["layers"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 3);
    let comp = s.project.comp(cid).unwrap();
    let spans: Vec<(f64, f64)> = ids.iter().map(|i| comp.layer(LayerId(*i)).unwrap()).map(|l| (l.in_point.seconds(), l.out_point.seconds())).collect();
    assert_eq!(spans, vec![(0.0, 0.4), (0.4, 0.72), (0.72, 1.0)]);
    s.undo();
    assert_eq!(s.project.comp(cid).unwrap().layers.len(), 1);
    // Split and Precompose: three precomp layers, one undo step.
    let r = s.execute("layer.sceneEditDetection", json!({"mode": "splitPrecompose", "wait": true})).unwrap();
    let comp = s.project.comp(cid).unwrap();
    assert_eq!(comp.layers.len(), 3);
    for v in r["layers"].as_array().unwrap() {
        let l = comp.layer(LayerId(v.as_u64().unwrap())).unwrap();
        assert!(matches!(l.source, LayerSource::Comp { .. }));
    }
    assert_eq!(s.history.undo.last().unwrap().0, "Scene Edit Detection");
    s.undo();
    assert_eq!(s.project.comp(cid).unwrap().layers.len(), 1);
}

// ---------------------------------------------------------------- Auto-trace

fn disc_frame(f: u32) -> Image {
    let mut img = Image::new(80, 60);
    let (cx, cy) = (30.0 + 2.0 * f as f64, 30.0);
    for y in 0..60 {
        for x in 0..80 {
            let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
            // Anti-aliased disc of radius 15 plus a 12 × 12 square, on transparency.
            let a = (15.5 - d).clamp(0.0, 1.0) as f32;
            let sq = (60..72).contains(&x) && (8..20).contains(&y);
            let a = if sq { 1.0 } else { a };
            img.set(x, y, [a, a, a, a]);
        }
    }
    img
}

fn iou_with(path_set: &[effectcraft_keyframe::ShapePath], truth: &Image) -> f64 {
    let paths: Vec<_> = path_set.iter().map(effectcraft_path::to_kurbo).collect();
    let cov = effectcraft_path::fill_coverage(&paths, &effectcraft_geom::Mat3::IDENTITY, truth.width, truth.height, effectcraft_path::FillRule::EvenOdd);
    let (mut i, mut u) = (0.0, 0.0);
    for (c, p) in cov.data.iter().zip(&truth.data) {
        let (a, b) = (*c >= 0.5, p[3] >= 0.5);
        i += (a && b) as u8 as f64;
        u += (a || b) as u8 as f64;
    }
    i / u
}

fn mask_paths(s: &Session, cid: ItemId, lid: LayerId, lt: Tick) -> Vec<effectcraft_keyframe::ShapePath> {
    let l = s.project.comp(cid).unwrap().layer(lid).unwrap();
    l.masks()
        .unwrap()
        .groups()
        .filter(|g| g.get("opacity").unwrap().value_at(lt).as_f64() > 0.0)
        .map(|g| match g.get("path").unwrap().value_at(lt) {
            effectcraft_project::Value::Path(p) => p,
            _ => panic!("path"),
        })
        .collect()
}

#[test]
fn auto_trace_matches_shapes() {
    let (mut s, cid, clip, _) = setup(80, 60, 0.4, disc_frame);
    let r = s.execute("layer.autoTrace", json!({"layer": clip.0, "channel": "alpha", "tolerance": 0.5, "cornerRoundness": 30})).unwrap();
    assert_eq!(r["masks"], json!(2));
    let iou = iou_with(&mask_paths(&s, cid, clip, Tick::ZERO), &disc_frame(0));
    assert!(iou > 0.95, "IoU {iou}");
    s.undo();
    // Work area onto a new layer: one key per frame following the moving disc.
    let r = s.execute("layer.autoTrace", json!({"layer": clip.0, "timeSpan": "workArea", "applyToNewLayer": true, "minimumArea": 20})).unwrap();
    let nl = LayerId(r["layer"].as_u64().unwrap());
    assert_ne!(nl, clip);
    assert_eq!(r["frames"], json!(10));
    for f in [0u32, 5, 9] {
        let t = FrameRate::new(FPS as i64, 1).tick_of(f as i64);
        let iou = iou_with(&mask_paths(&s, cid, nl, t), &disc_frame(f));
        assert!(iou > 0.93, "frame {f}: IoU {iou}");
    }
    // Luminance with Invert traces the background instead (one outline with holes).
    s.undo();
    let r = s.execute("layer.autoTrace", json!({"layer": clip.0, "channel": "luminance", "invert": true, "threshold": 50})).unwrap();
    assert!(r["masks"].as_u64().unwrap() >= 1);
}

// ---------------------------------------------------------------- Align Video to Data

#[test]
fn align_video_to_data() {
    let (mut s, cid, clip, _) = setup(16, 16, 20.0, |_| Image::filled(16, 16, [0.2, 0.2, 0.2, 1.0]));
    let add_data = |s: &mut Session, name: &str, ext: &str, text: &str| -> u64 {
        s.edit("data", None, |p, _| {
            let f =
                Footage { path: format!("{name}.{ext}"), kind: FootageKind::Data, codec: ext.to_uppercase(), data: Some(text.into()), ..Default::default() };
            Ok(p.add_item(name, Label::Sandstone, None, ItemKind::Footage(f)).0)
        })
        .unwrap()
    };
    let gps = add_data(&mut s, "gps", "csv", "lat,lon,timestamp\n1,2,2026-05-01T10:00:00Z\n1,2,2026-05-01T10:00:01Z\n");
    // The video started 5 s after the first sample: it moves 5 s later.
    let r = s.execute("layer.alignVideoToData", json!({"data": gps, "videoStart": "2026-05-01T10:00:05Z"})).unwrap();
    assert_eq!(r["startTime"], json!(5.0));
    let l = s.project.comp(cid).unwrap().layer(clip).unwrap();
    assert_eq!((l.start_time.seconds(), l.in_point.seconds()), (5.0, 5.0));
    s.undo();
    // Data placed at 2 s, timestamps as times of day, video start as timecode.
    let log = add_data(&mut s, "log", "json", r#"{"samples": [{"t": "09:59:58.0", "v": 1}, {"t": "09:59:59.0", "v": 2}]}"#);
    let r = s.execute("layer.alignVideoToData", json!({"data": "log", "videoStart": "10:00:00:00", "dataStart": 2.0})).unwrap();
    assert_eq!(r["key"], json!("t"));
    assert_eq!(r["startTime"], json!(4.0));
    // A video that started before the data moves earlier (negative start).
    let r = s.execute("layer.alignVideoToData", json!({"data": log, "videoStart": "2026-05-01T09:59:57Z"})).unwrap();
    assert_eq!(r["startTime"], json!(-1.0));
    // Errors: unknown key, no data.
    assert!(s.execute("layer.alignVideoToData", json!({"data": gps, "key": "nope", "videoStart": 0})).is_err());
    let _: Value = Value::Null;
}

#[test]
fn panels_commands_are_wired() {
    // The Window menu opens real panels now, and the former stubs are enabled.
    let menus = crate::menus::entries();
    for panel in ["lumetriScopes", "footage", "mediaBrowser", "metadata", "progress", "contentAwareFill"] {
        assert!(menus.iter().any(|(_, e)| e.command == "window.panel" && e.params["panel"] == panel), "{panel}");
    }
    assert!(crate::find_command("window.unavailablePanel").is_none());
    let (s, _, _, _) = setup(8, 8, 1.0, |_| Image::filled(8, 8, [0.0; 4]));
    for id in ["layer.autoTrace", "layer.sceneEditDetection", "layer.alignVideoToData", "layer.newContentAwareFill", "contentFill.generate"] {
        assert!(s.is_enabled(id), "{id}");
    }
}

/// A browser like the web app's: a virtual tree whose files are read on demand.
#[derive(Default)]
struct VirtualBrowser {
    tree: Mutex<crate::media_browser::VirtualTree>,
    fetched: Mutex<Vec<String>>,
    actions: Mutex<Vec<String>>,
}

impl crate::media_browser::Browser for VirtualBrowser {
    fn home(&self) -> String {
        "/Browser Storage".into()
    }
    fn places(&self) -> Vec<(String, String)> {
        vec![("Browser Storage".into(), self.home()), ("Shoot".into(), "/Folders/Shoot".into())]
    }
    fn list(&self, dir: &str, only: bool) -> Result<Vec<crate::media_browser::Entry>, String> {
        self.tree.lock().unwrap().list(dir, only)
    }
    fn fetch(&self, paths: &[String], _: &Value) -> bool {
        // Opened folders' files arrive later.
        let pending: Vec<String> = paths.iter().filter(|p| p.starts_with("/Folders/")).cloned().collect();
        self.fetched.lock().unwrap().extend(pending.iter().cloned());
        pending.is_empty()
    }
    fn actions(&self) -> Vec<(String, String)> {
        vec![("openFolder".into(), "Open Folder…".into())]
    }
    fn action(&self, id: &str) -> Result<Value, String> {
        self.actions.lock().unwrap().push(id.into());
        Ok(json!({"pending": true}))
    }
}

#[test]
fn media_browser_over_a_virtual_tree() {
    let b = Arc::new(VirtualBrowser::default());
    {
        let mut t = b.tree.lock().unwrap();
        t.add_dir("/Browser Storage");
        t.add_file("/Folders/Shoot/a.csv", "/Folders/Shoot/a.csv", 12, Some(1));
    }
    let mut s = Session { browser: Some(b.clone()), ..Default::default() };
    let r = s.execute("mediaBrowser.list", json!({})).unwrap();
    assert_eq!(r["path"], "/Browser Storage");
    assert_eq!(r["places"][1]["path"], "/Folders/Shoot");
    assert_eq!(r["actions"][0]["id"], "openFolder");
    let r = s.execute("mediaBrowser.go", json!({"path": "/Folders/Shoot"})).unwrap();
    assert_eq!(r["entries"][0]["name"], "a.csv");
    assert_eq!(r["parent"], "/Folders");
    assert!(s.execute("mediaBrowser.go", json!({"path": "/Nope"})).is_err());
    // Importing a file that is not read yet: pending (the host imports it once it arrives).
    let r = s.execute("mediaBrowser.import", json!({"paths": ["/Folders/Shoot/a.csv"]})).unwrap();
    assert_eq!(r["pending"], true);
    assert_eq!(*b.fetched.lock().unwrap(), ["/Folders/Shoot/a.csv"]);
    assert!(s.project.items.values().all(|i| !matches!(i.kind, ItemKind::Footage(_))));
    let r = s.execute("mediaBrowser.action", json!({"action": "openFolder"})).unwrap();
    assert_eq!(r["pending"], true);
    assert_eq!(*b.actions.lock().unwrap(), ["openFolder"]);
    assert!(s.execute("mediaBrowser.action", json!({})).is_err());
}

/// Footage the source can't decode is reported with its reason by `project.summary` and
/// `footage.info`, so an agent can tell the user why the picture is black.
#[test]
fn footage_that_cannot_be_decoded_reports_why() {
    struct Broken;
    impl FootageSource for Broken {
        fn frame(&self, _: ItemId, _: &Footage, _: Tick) -> Option<Arc<Image>> {
            None
        }
        fn problem(&self, f: &Footage) -> Option<String> {
            Some(format!("{} isn't supported by the built-in decoder", f.codec))
        }
    }
    let mut s = Session { footage: Arc::new(Broken), ..Default::default() };
    s.execute("comp.new", json!({"name": "C", "width": 64, "height": 36, "duration": 2})).unwrap();
    let f = Footage {
        kind: FootageKind::Video,
        path: "a.mp4".into(),
        codec: "H.264 (High 4:2:2)".into(),
        width: 64,
        height: 36,
        has_video: true,
        ..Default::default()
    };
    let item = std::sync::Arc::make_mut(&mut s.project).add_item("Clip", Default::default(), None, ItemKind::Footage(f));
    let sum = s.execute("project.summary", json!({})).unwrap();
    let it = sum["items"].as_array().unwrap().iter().find(|i| i["id"] == json!(item.0)).unwrap();
    assert_eq!(it["problem"], json!("H.264 (High 4:2:2) isn't supported by the built-in decoder"));
    assert!(sum["items"].as_array().unwrap().iter().filter(|i| i["id"] != json!(item.0)).all(|i| i.get("problem").is_none()), "only footage reports one");
    s.execute("footage.open", json!({"item": item.0})).unwrap();
    assert_eq!(s.execute("footage.info", json!({})).unwrap()["problem"], it["problem"]);
}

/// A proxy in use that can't be decoded leaves its item without a picture although the item's
/// own file is fine: `project.summary` and `footage.info` give the proxy's reason, said to be the
/// proxy's, for footage and compositions alike, and nothing once Use Proxy is off.
#[test]
fn a_proxy_in_use_that_cannot_be_decoded_reports_why() {
    struct BrokenProxies;
    impl FootageSource for BrokenProxies {
        fn frame(&self, _: ItemId, _: &Footage, _: Tick) -> Option<Arc<Image>> {
            None
        }
        fn problem(&self, f: &Footage) -> Option<String> {
            f.path.ends_with(".avi").then(|| format!("{} isn't supported by the built-in decoder", f.codec))
        }
    }
    let mut s = Session { footage: Arc::new(BrokenProxies), ..Default::default() };
    let comp = s.execute("comp.new", json!({"name": "C", "width": 64, "height": 36, "duration": 2})).unwrap()["comp"].as_u64().unwrap();
    let still = Footage { kind: FootageKind::Still, path: "a.png".into(), width: 64, height: 36, has_video: true, ..Default::default() };
    let item = std::sync::Arc::make_mut(&mut s.project).add_item("Plate", Default::default(), None, ItemKind::Footage(still));
    let proxy = Footage {
        kind: FootageKind::Video,
        path: "plate.avi".into(),
        codec: "MPEG-4 Part 2".into(),
        width: 32,
        height: 18,
        has_video: true,
        ..Default::default()
    };
    let want = json!("Proxy: MPEG-4 Part 2 isn't supported by the built-in decoder");
    let problem = |s: &mut Session, id: u64| {
        let sum = s.execute("project.summary", json!({})).unwrap();
        sum["items"].as_array().unwrap().iter().find(|i| i["id"] == json!(id)).unwrap().get("problem").cloned()
    };
    for id in [item.0, comp] {
        let p = std::sync::Arc::make_mut(&mut s.project);
        p.items.get_mut(&ItemId(id)).unwrap().proxy = Some(Box::new(effectcraft_project::Proxy { footage: proxy.clone(), enabled: true }));
        assert_eq!(problem(&mut s, id), Some(want.clone()), "item {id}");
        std::sync::Arc::make_mut(&mut s.project).items.get_mut(&ItemId(id)).unwrap().proxy.as_mut().unwrap().enabled = false;
        assert_eq!(problem(&mut s, id), None, "item {id}: its own file shows, and is fine");
    }
    std::sync::Arc::make_mut(&mut s.project).items.get_mut(&item).unwrap().proxy.as_mut().unwrap().enabled = true;
    s.execute("footage.open", json!({"item": item.0})).unwrap();
    assert_eq!(s.execute("footage.info", json!({})).unwrap()["problem"], want);
}
