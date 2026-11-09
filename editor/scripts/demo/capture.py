"""Film the README demos from the real app, without a window.

The app runs in the `snapshot` example and receives the same pointer events a hand would send, so
every timeline state in these GIFs is real. Only the cursor and the key hint are drawn on top
afterwards, because a headless app has no cursor. In the motion, one thing moves at a time, travel eases in and out, a hint fades out faster than it fades
in, and each loop ends where it began so the restart does not jump.

Usage: python editor/scripts/demo/capture.py [name ...]
"""
import shutil
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

from common import CLI, REPO, WORK, cmd, ease_in_out, run_script, snapshot

FPS = 15
ACCENT = (242, 181, 68)
TILE = (23, 26, 43)
FONT = ImageFont.truetype(str(REPO / "assets/fonts/Inter-Medium.ttf"), 15)
FULL = (5, 66, 1132, 790)
TIMELINE = (5, 515, 1132, 790)
OUT = REPO / "docs" / "images" / "demo"


class Film:
    def __init__(self, name, crop, start=(1000, 780)):
        self.name, self.crop = name, crop
        self.dir = WORK / name
        shutil.rmtree(self.dir, ignore_errors=True)
        self.dir.mkdir(parents=True)
        self.steps = [cmd("file.open", path=str(WORK / "scene.ecproj"))]
        self.done = []
        self.head = 1
        self.frames = []
        self.pos = start
        self.hint = None
        self.shots = 0
        self.last = None

    def setup(self, *steps):
        self.steps.extend(steps)
        self.head = len(self.steps)

    def restart(self, *replay):
        """The rest of the film runs in a fresh app, brought back to this state by `replay`."""
        self.done.append(self.steps)
        self.steps = self.steps[: self.head] + list(replay)

    def shot(self):
        path = self.dir / f"s{self.shots:04}.png"
        self.shots += 1
        self.steps.append({"method": "snap", "params": {"path": str(path)}})
        self.last = path
        return path

    def frame(self, down=False):
        self.frames.append((self.last, self.pos, down, self.hint))

    def begin(self, seconds=0.6):
        self.shot()
        self.hold(seconds)

    def hold(self, seconds):
        for _ in range(round(seconds * FPS)):
            self.frame()

    def say(self, text):
        self.hint = text

    def go(self, to, seconds=0.6):
        """Cursor travel: the app state stays, so one snapshot serves every frame of it."""
        a = self.pos
        n = max(2, round(seconds * FPS))
        for i in range(1, n + 1):
            k = ease_in_out(i / n)
            self.pos = (a[0] + (to[0] - a[0]) * k, a[1] + (to[1] - a[1]) * k)
            self.frame()
        self.steps.append({"method": "pointer", "params": {"x": to[0], "y": to[1], "settle": 0.1}})

    def click(self, settle=0.5):
        x, y = self.pos
        self.steps.append({"method": "pointer", "params": {"x": x, "y": y, "click": True, "settle": settle}})
        self.frame(True)
        self.frame(True)
        self.shot()
        self.frame()

    def drag(self, to, seconds=1.0, button="left", alt=False, shift=False, settle=0.06, curve=ease_in_out, press=True):
        x0, y0 = self.pos
        common = {"button": button, "alt": alt, "shift": shift, "settle": settle}
        if press:
            self.steps.append({"method": "pointer", "params": {"x": x0, "y": y0, "press": True, **common}})
        n = max(2, round(seconds * FPS))
        for i in range(1, n + 1):
            k = curve(i / n)
            self.pos = (x0 + (to[0] - x0) * k, y0 + (to[1] - y0) * k)
            self.steps.append({"method": "pointer", "params": {"x": self.pos[0], "y": self.pos[1], **common}})
            self.shot()
            self.frame(True)

    def pause_down(self, seconds):
        for _ in range(round(seconds * FPS)):
            self.frame(True)

    def release(self, button="left"):
        x, y = self.pos
        self.steps.append({"method": "pointer", "params": {"x": x, "y": y, "release": True, "button": button, "settle": 0.3}})
        self.shot()
        self.frame()

    def film(self):
        for steps in self.done + [self.steps]:
            for r in snapshot(steps, settle=6.0):
                if isinstance(r.get("reply"), dict) and r["reply"].get("ok") is False:
                    print("step failed:", r)
        self.assemble()

    def assemble(self):
        seq = self.dir / "seq"
        seq.mkdir(exist_ok=True)
        x0, y0, x1, y1 = self.crop
        alpha, shown = 0.0, None
        cache = {}
        for i, (path, pos, down, hint) in enumerate(self.frames):
            base = cache.get(path)
            if base is None:
                cache.clear()
                base = cache[path] = Image.open(path).convert("RGB").crop(self.crop)
            img = base.copy()
            # Fade in over 0.4 s, out over 0.27 s: leaving is about two thirds of arriving.
            if hint:
                shown, alpha = hint, min(1.0, alpha + 1 / 6) if hint == shown else 1 / 6
            else:
                alpha = max(0.0, alpha - 1 / 4)
            if shown and alpha > 0:
                draw_hint(img, shown, 1 - (1 - alpha) ** 3)
            draw_cursor(img, (pos[0] - x0, pos[1] - y0), down)
            img.save(seq / f"f{i:04}.png")
        OUT.mkdir(parents=True, exist_ok=True)
        gif = OUT / f"{self.name}.gif"
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error", "-framerate", str(FPS), "-i", str(seq / "f%04d.png"), "-vf",
             "split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle",
             "-loop", "0", str(gif)],
            check=True,
        )
        print(gif.name, f"{gif.stat().st_size / 1e6:.2f} MB, {len(self.frames) / FPS:.1f} s, {x1 - x0}x{y1 - y0}")


def draw_hint(img, text, alpha):
    w = round(FONT.getlength(text)) + 24
    h = 28
    x, y = 12, img.height - h - 10
    layer = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    d.rounded_rectangle((0, 0, w - 1, h - 1), 7, fill=TILE + (255,), outline=ACCENT + (255,))
    d.text((12, 5), text, font=FONT, fill=ACCENT + (255,))
    layer.putalpha(layer.getchannel("A").point(lambda v: int(v * alpha)))
    img.paste(layer, (x, y), layer)


ARROW = [(0, 0), (0, 16), (4, 12.5), (6.8, 18.6), (9.4, 17.4), (6.6, 11.6), (11.5, 11.5)]


def draw_cursor(img, pos, down):
    scale = 3
    size = 30
    layer = Image.new("RGBA", (size * scale, size * scale), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    o = 8
    if down:
        d.ellipse([(o - 7) * scale, (o - 7) * scale, (o + 7) * scale, (o + 7) * scale], outline=ACCENT + (255,), width=scale * 2)
    pts = [((o + x) * scale, (o + y) * scale) for x, y in ARROW]
    d.polygon(pts, fill=(255, 255, 255, 255))
    d.line(pts + pts[:1], fill=(10, 10, 14, 255), width=scale)
    layer = layer.resize((size, size), Image.LANCZOS)
    img.paste(layer, (round(pos[0]) - o, round(pos[1]) - o), layer)


# Timeline geometry of the scene at this window size.
def tx(t):
    return 617 + 41 * t


ROW = {"Title": 640, "Caption": 659, "Dawn": 697, "Tide": 716, "Backdrop": 735}
SHOTS_ROW, TITLES_ROW = 678, 621


def time_at(seconds):
    return cmd("time.set", time=seconds)


def slip():
    # The timeline only, playhead before the clips: repeated slips of a clip in the viewer make the
    # headless GPU wait time out, cause unknown. The clip's frames on its bar show the slip.
    f = Film("slip", TIMELINE)
    f.begin(0.8)
    f.go((tx(4.0), ROW["Dawn"]), 0.8)
    f.say("Alt + drag: pick another moment of the clip")
    f.hold(0.4)
    f.drag((tx(4.0) - 170, ROW["Dawn"]), 1.6, alt=True, settle=0.12)
    f.pause_down(0.8)
    f.drag((tx(4.0), ROW["Dawn"]), 1.2, alt=True, settle=0.12, press=False)
    f.release()
    f.say(None)
    f.go((1000, 780), 0.7)
    f.hold(0.8)
    f.film()


def pointer_drag(a, b, n=6, release=True):
    """The same drag without any shots, to bring a restarted app back to a state."""
    steps = [{"method": "pointer", "params": {"x": a[0], "y": a[1], "press": True, "settle": 0.06}}]
    for i in range(1, n + 1):
        k = i / n
        steps.append({"method": "pointer", "params": {"x": a[0] + (b[0] - a[0]) * k, "y": a[1] + (b[1] - a[1]) * k, "settle": 0.06}})
    if release:
        steps.append({"method": "pointer", "params": {"x": b[0], "y": b[1], "release": True, "settle": 0.3}})
    return steps


def groups():
    # A drop after a long run of shots makes the headless GPU wait time out, so each drop starts a
    # fresh app. The cause is not known; short scripts with the same drop pass.
    f = Film("groups", FULL)
    f.setup(time_at(3.5))
    f.begin(0.8)
    f.say("A group's Video switch hides every layer inside it")
    f.go((15, SHOTS_ROW), 0.8)
    f.click()
    f.hold(1.2)
    f.click()
    f.hold(0.5)
    f.say("Collapsed, it keeps a bar showing what it covers")
    f.go((135, SHOTS_ROW), 0.7)
    f.click()
    f.hold(1.4)
    f.click()
    f.hold(0.5)
    f.say("Drag a layer in or out of a group")
    f.go((190, ROW["Caption"]), 0.7)
    f.drag((190, 707), 0.9)
    f.restart(*pointer_drag((190, ROW["Caption"]), (190, 707), release=False))
    f.release()
    f.hold(1.0)
    f.go((190, 697), 0.3)
    f.drag((190, TITLES_ROW), 0.9)
    f.restart(*pointer_drag((190, ROW["Caption"]), (190, 707)), *pointer_drag((190, 697), (190, TITLES_ROW), release=False))
    f.release()
    f.say(None)
    f.go((1000, 780), 0.7)
    f.hold(0.6)
    f.film()


def snap():
    f = Film("snap", TIMELINE)
    f.begin(0.6)
    start = (tx(4.5), ROW["Caption"])
    f.say("Shift + drag: edges snap to visible targets")
    f.go(start, 0.7)
    f.drag((start[0] + 62, start[1]), 1.0, shift=True, settle=0.1)
    f.pause_down(1.0)
    f.drag((start[0] + 205, start[1]), 1.1, shift=True, settle=0.1, press=False)
    f.pause_down(1.2)
    f.drag(start, 1.0, shift=True, settle=0.1, press=False)
    f.release()
    f.say(None)
    f.go((1000, 780), 0.6)
    f.hold(0.5)
    f.film()


def pan():
    f = Film("pan", TIMELINE)
    f.setup(cmd("timeline.zoomIn"), cmd("timeline.zoomIn"), cmd("timeline.zoomIn"), cmd("timeline.zoomIn"))
    f.begin(0.6)
    f.say("Middle mouse button: pan the timeline")
    f.go((900, 770), 0.6)
    f.drag((640, 770), 1.2, button="middle", settle=0.08)
    f.pause_down(0.7)
    f.drag((900, 770), 1.2, button="middle", settle=0.08, press=False)
    f.release("middle")
    f.say(None)
    f.go((1000, 780), 0.4)
    f.hold(0.5)
    f.film()


def trim():
    f = Film("trim", TIMELINE)
    f.begin(0.6)
    edge = (tx(6), ROW["Tide"])
    f.say("Clip edges stick to the ends of the source")
    f.go(edge, 0.7)
    f.drag((tx(3) + 4, edge[1]), 1.2, settle=0.1)
    f.pause_down(1.2)
    f.say("Keep pulling and the edge goes past it: frozen frame, hatched")
    f.drag((tx(3) - 30, edge[1]), 0.7, settle=0.1, press=False)
    f.pause_down(1.2)
    f.drag(edge, 1.0, settle=0.1, press=False)
    f.release()
    f.say(None)
    f.go((1000, 780), 0.5)
    f.hold(0.5)
    f.film()


def frames():
    f = Film("frames", TIMELINE)
    f.begin(0.5)
    f.say("Video frames on the tracks; one button turns them off")
    f.go((560, 564), 0.8)
    f.click()
    f.hold(1.2)
    f.click()
    f.hold(0.6)
    f.say(None)
    f.go((1000, 780), 0.5)
    f.hold(0.5)
    f.film()


def bartext():
    f = Film("bartext", TIMELINE)
    f.setup(cmd("timeline.zoomIn"), cmd("timeline.zoomIn"))
    f.begin(0.6)
    f.say("A text layer's bar repeats its text, faintly")
    f.go((900, 770), 0.6)
    f.hold(0.8)
    f.drag((640, 770), 1.2, button="middle", settle=0.08)
    f.pause_down(0.8)
    f.drag((900, 770), 1.2, button="middle", settle=0.08, press=False)
    f.release("middle")
    f.say(None)
    f.go((1000, 780), 0.4)
    f.hold(0.5)
    f.film()


def hero():
    jsx = REPO / "editor/scripts/demo/hero.jsx"
    project, video = WORK / "hero.ecproj", WORK / "hero.mp4"
    run_script(jsx, project, REPO)
    subprocess.run([str(CLI), "render", "--project", str(project), "--comp", "Hero", "--out", str(video), "--format", "h264",
                    "--bitrate", "20000", "--quality", "best"], check=True)
    OUT.mkdir(parents=True, exist_ok=True)
    gif = OUT / "hero.gif"
    # `-loop -1` plays once: a title that restarts every few seconds would fight the text under it.
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-i", str(video), "-vf",
         "fps=20,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle",
         "-loop", "-1", str(gif)],
        check=True,
    )
    print(gif.name, f"{gif.stat().st_size / 1e6:.2f} MB")


FILMS = {"hero": hero, "groups": groups, "snap": snap, "pan": pan, "slip": slip, "trim": trim, "frames": frames, "bartext": bartext}


if __name__ == "__main__":
    if not (WORK / "dawn.mp4").exists():
        subprocess.run([sys.executable, str(Path(__file__).with_name("footage.py")), str(WORK)], check=True)
    run_script(REPO / "editor/scripts/demo/scene.jsx", WORK / "scene.ecproj", WORK)
    for name in sys.argv[1:] or FILMS:
        FILMS[name]()
