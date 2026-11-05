"""Shared paths and helpers for the README demo capture scripts."""
import json
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
WORK = REPO / "editor" / "output" / "public" / "demo"
SNAPSHOT = REPO / "editor" / "target" / "release" / "examples" / "snapshot.exe"
CLI = REPO / "editor" / "target" / "release" / "effectcraft-cli.exe"


def ease_in_out(t):
    """cubic-bezier(0.6, 0, 0.2, 1) in spirit: a move between two states starts and lands softly."""
    return t * t * t * (t * (6 * t - 15) + 10)


def run_script(jsx, out_project, root):
    tmp = WORK / (Path(jsx).stem + ".run.jsx")
    tmp.write_text(f'var ROOT_DIR = "{root.as_posix()}/";\n' + Path(jsx).read_text(encoding="utf-8"), encoding="utf-8")
    subprocess.run([str(CLI), "script", str(tmp), "--empty", "--save-as", str(out_project)], check=True)


def snapshot(steps, size=(1440, 860), scale=1.0, settle=2.5):
    """Runs the real app without a window and returns the parsed reply of every step."""
    script = WORK / "steps.jsonl"
    script.write_text("\n".join(json.dumps(s) for s in steps), encoding="utf-8")
    res = subprocess.run(
        [str(SNAPSHOT), "--empty", "--out", str(WORK / "last.png"), "--size", f"{size[0]}x{size[1]}", "--scale", str(scale),
         "--settle", str(settle), "--script", str(script)],
        capture_output=True, text=True, encoding="utf-8", env={**__import__("os").environ, "EFFECTCRAFT_SYSTEM_LOCALE": "en"},
    )
    if res.returncode:
        raise SystemExit(res.stderr[-2000:])
    return [json.loads(l) for l in res.stdout.splitlines() if l.startswith("{")]


def cmd(command, **params):
    return {"method": "engine.execute", "params": {"command": command, "params": params}}
