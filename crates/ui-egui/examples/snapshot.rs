//! Headless UI snapshot: runs the real [`EffectcraftApp`] without a window (egui_kittest + wgpu)
//! and writes PNGs. Works regardless of window focus, Spaces or display sleep, so agents can
//! look at the UI at any time.
//!
//! ```text
//! cargo run -p effectcraft-ui-egui --example snapshot -- [--out ui.png] [--size 1680x1020]
//!     [--scale 2] [--settle 1.5] [--empty] [--software] [--script steps.jsonl | --step '<json>']...
//! ```
//!
//! The UI and the compositor run on a hardware GPU when there is one. `--software` keeps
//! egui_kittest's own choice, a software rasterizer (WARP on Windows), on which the
//! compositor's first frame does not finish.
//!
//! Each script step is a control-channel request (`{"method":"engine.execute","params":{...}}`,
//! see `docs/control-protocol.md`), run in order with the app settling in between. The extra
//! method `{"method":"snap","params":{"path":"x.png"}}` writes an intermediate snapshot.
//! Replies are printed to stdout as JSON lines.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use effectcraft_ui_egui::{ControlRequest, EffectcraftApp};
use egui_kittest::Harness;
use serde_json::{Value, json};

struct Args {
    out: String,
    size: (f32, f32),
    scale: f32,
    settle: f64,
    demo: bool,
    software: bool,
    steps: Vec<Value>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { out: "ui.png".into(), size: (1680.0, 1020.0), scale: 2.0, settle: 1.5, demo: true, software: false, steps: Vec::new() };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--out" => a.out = val()?,
            "--size" => {
                let v = val()?;
                let (w, h) = v.split_once('x').ok_or("--size WxH")?;
                a.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--scale" => a.scale = val()?.parse().map_err(|_| "bad --scale")?,
            "--settle" => a.settle = val()?.parse().map_err(|_| "bad --settle")?,
            "--empty" => a.demo = false,
            "--software" => a.software = true,
            "--step" => a.steps.push(serde_json::from_str(&val()?).map_err(|e| format!("--step: {e}"))?),
            "--script" => {
                let path = val()?;
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
                for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")) {
                    a.steps.push(serde_json::from_str(line).map_err(|e| format!("{path}: {e}: {line}"))?);
                }
            }
            "-h" | "--help" => {
                return Err(
                    "usage: snapshot [--out ui.png] [--size WxH] [--scale 2] [--settle secs] [--empty] [--software] [--script f.jsonl | --step json]...".into(),
                );
            }
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    Ok(a)
}

/// One frame, first queueing the app's synthetic input (kittest doesn't call
/// `raw_input_hook`); the harness runs one frame per queued event.
fn step_frame(h: &mut Harness<'_, EffectcraftApp>) {
    for e in h.state_mut().take_synthetic_input() {
        h.event(e);
    }
    h.step();
}

/// Step the app for `secs` of wall time so background frame renders land in the viewer.
fn settle(h: &mut Harness<'_, EffectcraftApp>, secs: f64) {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < end {
        step_frame(h);
        std::thread::sleep(Duration::from_millis(30));
    }
    step_frame(h);
}

fn snap(h: &mut Harness<'_, EffectcraftApp>, path: &str) -> Result<(), String> {
    let img = h.render()?;
    img.save(path).map_err(|e| format!("{path}: {e}"))?;
    println!("{}", json!({"snapshot": path, "width": img.width(), "height": img.height()}));
    Ok(())
}

/// egui_kittest prefers a CPU adapter so test images match across machines. On Windows that is
/// WARP, where the compositor's blend kernels never finish their first dispatch and filming
/// hangs. The viewer composites on the harness's own device, so picking it here is enough.
fn hardware_setup() -> eframe::egui_wgpu::WgpuSetup {
    let mut setup = egui_kittest::wgpu::default_wgpu_setup();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(c) = &mut setup {
        let fallback = c.native_adapter_selector.take();
        c.native_adapter_selector = Some(std::sync::Arc::new(move |adapters, surface| {
            use eframe::wgpu::DeviceType;
            let rank = |t: DeviceType| match t {
                DeviceType::DiscreteGpu => Some(0),
                DeviceType::IntegratedGpu => Some(1),
                DeviceType::VirtualGpu => Some(2),
                DeviceType::Other | DeviceType::Cpu => None,
            };
            let best = adapters.iter().filter_map(|a| rank(a.get_info().device_type).map(|r| (r, a))).min_by_key(|(r, _)| *r);
            match (best, &fallback) {
                (Some((_, a)), _) => Ok(a.clone()),
                (None, Some(f)) => f(adapters, surface),
                (None, None) => adapters.first().cloned().ok_or_else(|| "No adapter found".to_owned()),
            }
        }));
    }
    setup
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let mut session = effectcraft_host::session();
    if args.demo {
        let _ = session.execute("file.openDemoProject", json!({}));
    }
    let (tx, rx) = mpsc::channel::<ControlRequest>();
    let builder = Harness::builder().with_size(egui::vec2(args.size.0, args.size.1)).with_pixels_per_point(args.scale);
    let builder = if args.software { builder.wgpu() } else { builder.wgpu_setup(hardware_setup()) };
    let mut harness = builder.build_eframe(move |_cc| EffectcraftApp::new(session).with_control(rx));
    // A real window has no painted pointer either; the demos draw their own over these images.
    harness.set_render_cursor(false);
    settle(&mut harness, args.settle);
    for step in &args.steps {
        let method = step["method"].as_str().unwrap_or_default().to_string();
        let params = step.get("params").cloned().unwrap_or(json!({}));
        if method == "snap" {
            if let Err(e) = snap(&mut harness, params["path"].as_str().unwrap_or("snap.png")) {
                eprintln!("{e}");
            }
            continue;
        }
        // Pointer input straight into the harness (menus and popups need real input events):
        // {"method":"pointer","params":{"x":237,"y":12,"click":true}}
        if method == "pointer" {
            let pos = egui::pos2(params["x"].as_f64().unwrap_or(0.0) as f32, params["y"].as_f64().unwrap_or(0.0) as f32);
            // A gesture split over steps, so a snapshot can catch it halfway:
            // {"x":..,"y":..,"press":true,"button":"middle","alt":true,"settle":0.05}, then moves, then "release".
            let modifiers =
                egui::Modifiers { alt: params["alt"].as_bool() == Some(true), shift: params["shift"].as_bool() == Some(true), ..Default::default() };
            let button = if params["button"].as_str() == Some("middle") { egui::PointerButton::Middle } else { egui::PointerButton::Primary };
            harness.input_mut().events.push(egui::Event::ModifiersChanged(modifiers));
            harness.input_mut().events.push(egui::Event::PointerMoved(pos));
            let flag = |name: &str| params[name].as_bool() == Some(true);
            let presses: &[bool] = if flag("click") {
                &[true, false]
            } else if flag("press") {
                &[true]
            } else if flag("release") {
                &[false]
            } else {
                &[]
            };
            for &pressed in presses {
                harness.input_mut().events.push(egui::Event::PointerButton { pos, button, pressed, modifiers });
            }
            settle(&mut harness, params["settle"].as_f64().unwrap_or(0.3));
            println!("{}", json!({"method": method, "reply": {"ok": true}}));
            continue;
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        let _ = tx.send(ControlRequest { method: method.clone(), params, reply: reply_tx });
        // Requests are answered on a later frame (some wait for input to be processed).
        let deadline = Instant::now() + Duration::from_secs(10);
        let reply = loop {
            step_frame(&mut harness);
            if let Ok(v) = reply_rx.try_recv() {
                break v;
            }
            if Instant::now() > deadline {
                break json!({"ok": false, "error": "no reply within 10 s"});
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        println!("{}", json!({"method": method, "reply": reply}));
        settle(&mut harness, 0.3);
    }
    settle(&mut harness, args.settle.min(1.0));
    // stderr, so scripts reading the JSON replies on stdout are unaffected.
    eprintln!("snapshot: GPU compositor {}", harness.state().gpu_adapter().unwrap_or_else(|| "not ready (CPU frames)".into()));
    if let Err(e) = snap(&mut harness, &args.out) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
