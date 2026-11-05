//! Manual timing probe; excluded from CI because wall-clock budgets depend on the machine.
use std::{hint::black_box, time::Instant};

use effectcraft_engine::Session;
use effectcraft_ui_egui::EffectcraftApp;
use egui_kittest::Harness;
use serde_json::json;

#[test]
#[ignore = "manual 300-layer/50-group timing probe"]
fn groups_frame_cost_on_300_layers() {
    let mut means = Vec::new();
    for expanded in [false, true] {
        for grouped in [false, true] {
            let mut s = Session::default();
            s.execute("comp.new", json!({"name":"Timing","width":64,"height":36,"duration":6})).unwrap();
            for i in 0..300 {
                s.execute("layer.newSolid", json!({"name":format!("Layer {i}"),"color":"#406080","width":16,"height":16})).unwrap();
            }
            let ids: Vec<_> = s.active_comp().unwrap().layers.iter().map(|l| l.id.0).collect();
            if grouped {
                for chunk in ids.chunks(6) {
                    s.execute("layer.group", json!({"layers":chunk})).unwrap();
                }
            }
            s.execute("edit.deselectAll", json!({})).unwrap();
            let mut app = EffectcraftApp::new(s);
            if expanded {
                for l in &app.session.active_comp().unwrap().layers {
                    app.ui.timeline.open_layers.insert(l.id.0);
                    app.ui.timeline.open_groups.insert(l.props.sub("transform").unwrap().uid);
                }
            }
            let mut h = Harness::builder().with_size(egui::vec2(1900., 1000.)).with_step_dt(1.0 / 60.0).build_eframe(|_| app);
            h.run_steps(30);
            let mut samples = Vec::new();
            for _ in 0..80 {
                let start = Instant::now();
                h.step();
                samples.push(start.elapsed().as_secs_f64() * 1000.);
            }
            let mean = samples.iter().sum::<f64>() / samples.len() as f64;
            samples.sort_by(f64::total_cmp);
            let c = h.state().session.active_comp().unwrap();
            let start = Instant::now();
            for _ in 0..1000 {
                let c = black_box(c);
                for l in &c.layers {
                    black_box(c.layer_group_video_enabled(l.id));
                }
            }
            let gates = start.elapsed().as_secs_f64();
            eprintln!("expanded={expanded}, grouped={grouped}: mean={mean:.3} ms, p95={:.3} ms, 300 Video gates={gates:.3} ms", samples[76]);
            if grouped {
                let mut muted = c.clone();
                for g in &mut muted.layer_groups {
                    g.video = false;
                }
                let start = Instant::now();
                for _ in 0..1000 {
                    let c = black_box(&muted);
                    for l in &c.layers {
                        black_box(c.layer_group_video_enabled(l.id));
                    }
                }
                eprintln!("expanded={expanded}: 300 Video gates, all groups off={:.3} ms", start.elapsed().as_secs_f64());
            }
            means.push(mean);
        }
    }
    assert!(means[3] - means[2] < 1.0, "group overhead with expanded properties exceeds 1 ms/frame: {means:?}");
}
