"""Generate the synthetic clips used by the README demos, so no third-party media is involved.

Usage: python editor/scripts/demo/footage.py <out-dir>. Needs numpy and ffmpeg on PATH.
"""
import subprocess
import sys
from pathlib import Path

import numpy as np

W, H, FPS = 640, 360, 30


def mix(a, b, t):
    return a + (b - a) * t


def hills(w, h, base, amp, phase):
    x = np.linspace(0, 6.28, w)
    return (base + amp * (np.sin(x * 1.3 + phase) * 0.6 + np.sin(x * 3.1 + phase * 1.7) * 0.25)) * h


def dawn(t):
    """Sky warms up while the sun climbs from behind the hills, so every moment looks different."""
    yy, xx = np.mgrid[0:H, 0:W].astype(np.float32)
    v = (yy / H)[..., None]
    top = mix(np.array([18, 22, 58.0]), np.array([64, 118, 190.0]), t)
    low = mix(np.array([120, 58, 92.0]), np.array([250, 196, 120.0]), t)
    img = mix(top, low, v)
    sx, sy = W * (0.2 + 0.6 * t), H * (0.86 - 0.62 * t)
    d = np.hypot(xx - sx, yy - sy)
    glow = np.exp(-(d / 110.0) ** 2)[..., None]
    img = img + glow * np.array([120, 90, 40.0])
    img[d < 26] = np.array([255, 236, 190.0])
    for base, amp, phase, shade in ((0.72, 0.06, 0.4, 0.55), (0.84, 0.05, 2.1, 0.3)):
        mask = yy > hills(W, H, base, amp, phase)[None, :]
        img[mask] = mix(np.array([28, 24, 52.0]), np.array([60, 72, 84.0]), t) * shade * 1.6
    return img


def tide(t):
    """Waves pass through the frame and the water changes colour."""
    yy, xx = np.mgrid[0:H, 0:W].astype(np.float32)
    v = (yy / H)[..., None]
    img = mix(mix(np.array([16, 92, 112.0]), np.array([224, 112, 84.0]), t), np.array([10, 26, 52.0]), v)
    for i in range(7):
        y0 = H * (0.3 + i * 0.1)
        wave = y0 + 9 * np.sin(xx / (46 + i * 7) + t * 34 + i * 1.3)
        band = np.clip(1 - np.abs(yy - wave) / (2.2 + i * 0.5), 0, 1)[..., None]
        img = img + band * (70 - i * 5)
    return img


def write(path, seconds, fn):
    p = subprocess.Popen(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-",
         "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-g", "15", str(path)],
        stdin=subprocess.PIPE,
    )
    n = seconds * FPS
    for i in range(n):
        p.stdin.write(np.clip(fn(i / (n - 1)), 0, 255).astype(np.uint8).tobytes())
    p.stdin.close()
    p.wait()


out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
write(out / "dawn.mp4", 12, dawn)
write(out / "tide.mp4", 12, tide)
