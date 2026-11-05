//! Help ▸ Check for Updates. This build installs from its own MSI, so it never checks for,
//! downloads or applies upstream releases: an upstream update would replace it with a build
//! that lacks our changes. The command stays and explains that.

use crate::EffectcraftApp;

#[derive(Default)]
pub(crate) struct Runtime {
    pub(crate) manual_window_open: bool,
    pub(crate) status_message: Option<String>,
}

pub(crate) fn check(app: &mut EffectcraftApp) {
    app.updates_rt.manual_window_open = true;
    app.updates_rt.status_message = Some(crate::i18n::tr("Automatic updates are turned off in this build. Install new versions from its MSI.").to_string());
}

pub(crate) fn tick(app: &mut EffectcraftApp, ctx: &egui::Context) {
    let _ = (app, ctx);
}

pub(crate) fn show(app: &mut EffectcraftApp, ctx: &egui::Context) {
    if !app.updates_rt.manual_window_open {
        return;
    }
    let mut open = true;
    let mut close = false;
    egui::Window::new(crate::i18n::tr("Check for Updates"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            if let Some(msg) = &app.updates_rt.status_message {
                ui.label(msg);
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(crate::i18n::tr("Close")).clicked() {
                    close = true;
                }
            });
        });
    if !open || close {
        app.updates_rt.manual_window_open = false;
        app.updates_rt.status_message = None;
    }
}
