//! A small real-engine check. It is not the editor UI or a performance benchmark.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::{error::Error, path::Path, process::ExitCode};

use effectcraft_engine::{EngineError, Session};
use serde_json::json;

type CheckResult<T> = Result<T, Box<dyn Error>>;

fn session() -> Session {
    let mut session = effectcraft_host::session();
    // Keep this check independent from the installed EffectCraft application's settings.
    session.models_dir = None;
    session.config = None;
    session.check_footage_on_open = false;
    session
}

fn require(condition: bool, message: &str) -> CheckResult<()> {
    if condition { Ok(()) } else { Err(message.into()) }
}

fn frame(session: &Session) -> CheckResult<Vec<u8>> {
    let comp = session.active_comp_id().ok_or(EngineError::NoComp)?;
    let (width, height, pixels) = session.render_rgba8(comp, session.time(), 32)?;
    require(width == 32 && height == 32 && pixels.len() == 32 * 32 * 4, "Unexpected frame size")?;
    Ok(pixels)
}

fn check(directory: &Path) -> CheckResult<()> {
    let mut editor = session();
    editor.execute(
        "comp.new",
        json!({"name": "Integration check", "width": 32, "height": 32,
        "frameRate": 24, "duration": 2, "background": "#000000"}),
    )?;
    let created = editor.execute("layer.newSolid", json!({"name": "Test solid", "color": "#ffffff"}))?;
    let layer = created.get("layer").and_then(|id| id.as_u64()).ok_or("No layer ID returned")?;
    let initial = frame(&editor)?;
    require(initial.chunks_exact(4).any(|p| p.first().is_some_and(|r| *r > 200)), "The rendered solid is missing")?;

    editor.execute("prop.set", json!({"layer": layer, "path": "transform/opacity", "value": 0}))?;
    let changed = frame(&editor)?;
    require(initial != changed, "Changing opacity did not affect the frame")?;
    editor.execute("edit.undo", json!({}))?;
    require(frame(&editor)? == initial, "Undo did not restore the frame")?;
    editor.execute("edit.redo", json!({}))?;
    require(frame(&editor)? == changed, "Redo did not restore the edit")?;

    // Adjacent frames must remain adjacent, including at a fractional frame rate.
    for rate in [24.0, 30000.0 / 1001.0] {
        editor.execute("comp.settings", json!({"frameRate": rate}))?;
        for index in [7, 8, 9] {
            let time = editor.execute("time.set", json!({"frame": index}))?;
            require(time.get("frame").and_then(|v| v.as_i64()) == Some(index), "Frame navigation lost precision")?;
        }
    }

    let path = directory.join("roundtrip.ecproj");
    let path = path.to_str().ok_or("The test path cannot be represented as UTF-8")?;
    editor.execute("file.saveAs", json!({"path": path}))?;
    let mut reopened = session();
    reopened.execute("file.open", json!({"path": path}))?;
    reopened.execute("comp.open", json!({"comp": "Integration check"}))?;
    let property = reopened.execute("prop.get", json!({"layer": layer, "path": "transform/opacity"}))?;
    require(property.get("value").and_then(|v| v.as_f64()) == Some(0.0), "Saved opacity was not preserved")?;
    require(frame(&reopened)? == changed, "Reopening changed the rendered frame")?;

    // File-open errors must keep the current document intact.
    let invalid = directory.join("invalid.ecproj");
    std::fs::write(&invalid, b"{ invalid project")?;
    let invalid = invalid.to_str().ok_or("The test path cannot be represented as UTF-8")?;
    require(reopened.execute("file.open", json!({"path": invalid})).is_err(), "A malformed project was accepted")?;
    require(frame(&reopened)? == changed, "A failed open damaged the current document")?;
    Ok(())
}

fn run() -> CheckResult<()> {
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../output/tmp");
    std::fs::create_dir_all(&output)?;
    let directory = tempfile::Builder::new().prefix("engine-check-").tempdir_in(output)?;
    let result = check(directory.path());
    // Remove generated projects on success and ordinary errors.
    directory.close()?;
    result
}

fn main() -> ExitCode {
    eprintln!("Checking the pinned EffectCraft engine...");
    match run() {
        Ok(()) => {
            println!("PASS: EffectCraft create/render/edit/undo/redo/frame navigation/save/open/error recovery");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Engine integration check failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn engine_roundtrip() -> super::CheckResult<()> {
        super::run()
    }
}
