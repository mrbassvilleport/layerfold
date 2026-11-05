use super::{EngineError, Event, Result, Session, Value, commands, guard};

/// Commands that write files named by these parameters. A script may use them only with Settings ▸
/// Scripting & Expressions ▸ Allow Scripts to Write Files and Access Network on, except for saving
/// a project file ([`Session::refuse_script_write`]). `camera.exportBlender` checks for itself.
pub const SCRIPT_WRITES: &[(&str, &[&str])] = &[
    ("file.save", &["path"]),
    ("file.saveAs", &["path"]),
    ("file.saveCopy", &["path"]),
    ("file.saveCopyAsXml", &["path"]),
    ("file.collectFiles", &["folder"]),
    ("file.exportLottie", &["path"]),
    ("file.exportTimeline", &["path"]),
    ("file.createProxy", &["path", "output"]),
    ("comp.saveFrameAs", &["path"]),
    ("comp.saveFrameAsPsd", &["path"]),
    ("comp.saveFrameAsExr", &["path"]),
    ("anim.savePreset", &["path"]),
    ("view.exportGuides", &["path"]),
    ("render.saveCurrentPreview", &["path"]),
    ("render.preRender", &["output"]),
    ("renderQueue.add", &["output", "path"]),
    ("essential.exportTemplate", &["path"]),
    ("shortcuts.export", &["path"]),
    ("templates.create", &["projectPath", "footageDir"]),
];

impl Session {
    pub fn new() -> Session {
        Session::default()
    }

    /// Run a command by id.
    pub fn execute(&mut self, id: &str, params: Value) -> Result<Value> {
        let spec = commands::find(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        self.refuse_script_write(id, &params)?;
        if let Err(why) = (spec.enabled)(self) {
            // Explicit targets (agents, scripts) don't need a UI selection.
            let explicit = (["layer", "layers", "prop", "keys"].iter().any(|k| params.get(k).is_some()) && commands::has_comp(self).is_ok())
                || ["item", "items"].iter().any(|k| params.get(k).is_some())
                // An explicit composition (the enablement looks at the active one).
                || params.get("comp").is_some_and(|c| !c.is_null() && self.resolve_comp(Some(c)).is_ok());
            if !explicit {
                return Err(EngineError::Disabled(id.to_string(), why));
            }
        }
        // Last-resort guard: a panicking command reports an error instead of crashing the app.
        let r = guard::guarded(&format!("command `{id}`"), || (spec.run)(self, &params))?;
        self.sanitize_active_layer();
        self.learn_observe(id, &params);
        if spec.journal {
            self.journal.push((id.to_string(), params));
            if self.journal.len() > 10_000 {
                self.journal.drain(..1000);
            }
        }
        Ok(r)
    }

    /// [`Session::execute`] for agents and scripts (MCP, CLI, control channel): unknown top-level
    /// parameters are rejected with the accepted keys instead of being silently ignored.
    pub fn execute_checked(&mut self, id: &str, params: Value) -> Result<Value> {
        let spec = commands::find(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        commands::check_params(spec, &params)?;
        self.execute(id, params)
    }

    /// [`Session::execute`] for a command a script asked for (the JavaScript engine's
    /// `app.run`, ScriptUI callbacks…). The command and everything it runs count as the script's:
    /// it may not change the settings that limit scripts (Settings ▸ Scripting & Expressions ▸
    /// Application Scripting), which only the user can, from the UI, CLI or control channel.
    pub fn execute_for_script(&mut self, id: &str, params: Value) -> Result<Value> {
        self.script_calls.0 = self.script_calls.0.saturating_add(1);
        let r = self.execute(id, params);
        self.script_calls.0 = self.script_calls.0.saturating_sub(1);
        r
    }

    /// A command run for a script may not write a file it names unless the user allowed that. The
    /// exception is a project file: its extension can't start anything, and saving is what the
    /// scripting API promises. Without this a script could put a text file anywhere, such as a
    /// batch file in the Startup folder.
    fn refuse_script_write(&self, id: &str, params: &Value) -> Result<()> {
        if !self.running_for_script() || self.prefs.scripting.allow_scripts_write_files {
            return Ok(());
        }
        let Some((_, names)) = SCRIPT_WRITES.iter().find(|(c, _)| *c == id) else { return Ok(()) };
        for path in names.iter().filter_map(|n| params.get(*n)).filter(|v| !v.is_null()) {
            let project_file = path.as_str().and_then(|p| std::path::Path::new(p).extension()).is_some_and(|e| e.eq_ignore_ascii_case("ecproj"));
            if !(matches!(id, "file.save" | "file.saveAs") && project_file) {
                return Err(EngineError::Other(format!(
                    "{id}: scripts can't write files unless Settings ▸ Scripting & Expressions ▸ Allow Scripts to Write Files and Access Network is on"
                )));
            }
        }
        Ok(())
    }

    /// Whether the command running was issued by a script ([`Session::execute_for_script`]).
    pub fn running_for_script(&self) -> bool {
        self.script_calls.0 > 0
    }

    /// Whether `id` names a known command whose `enabled` predicate returns `Ok` for this session;
    /// an unknown id or a predicate error reads as disabled.
    pub fn is_enabled(&self, id: &str) -> bool {
        commands::find(id).is_some_and(|c| (c.enabled)(self).is_ok())
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.events.push(Event::Toast { message: msg.into(), error: false });
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
}
