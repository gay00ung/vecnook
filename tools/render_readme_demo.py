#!/usr/bin/env python3
"""Create the README preview from the existing recording; requires ffmpeg/Pillow."""
from io import BytesIO
from pathlib import Path
import subprocess

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
font = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 22)
stages = [
    (5, "01 / 03   RECOVER RECORDS", "#63d9b5"),
    (14, "02 / 03   REOPEN + SEARCH", "#78b7ff"),
    (23, "03 / 03   FIND FILTERING GUIDANCE", "#ffcd80"),
]
frames = []
for stage, (timestamp, label, accent) in enumerate(stages):
    data = subprocess.check_output([
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-ss", str(timestamp),
        "-i", str(ROOT / "docs/media/offline-demo.mp4"), "-frames:v", "1",
        "-f", "image2pipe", "-vcodec", "png", "-",
    ])
    # Keep command, query and source result intact; omit the long excerpt below.
    original = Image.open(BytesIO(data)).convert("RGB").crop((0, 0, 1280, 324))
    for tick in range(12):
        frame = Image.new("RGB", (1280, 420), "#0f141d")
        frame.paste(original, (0, 60))
        draw = ImageDraw.Draw(frame)
        draw.text((36, 15), label, fill=accent, font=font)
        draw.line((24, 280, 24, 382), fill=accent, width=4)
        for i in range(3):
            start = 36 + i * 410
            draw.rounded_rectangle((start, 404, start + 388, 409), radius=2, fill="#29343f")
            if i == stage:
                draw.rounded_rectangle((start, 404, start + 388 * (tick + 1) / 12, 409),
                                       radius=2, fill=accent)
        frames.append(frame.resize((1120, 368), Image.Resampling.LANCZOS))
out = ROOT / "docs/media/offline-demo.gif"
palette_source = Image.new("RGB", (1120, 368 * 3))
for i in range(3):
    palette_source.paste(frames[i * 12], (0, i * 368))
palette = palette_source.quantize(colors=128)
frames = [frame.quantize(palette=palette, dither=Image.Dither.NONE) for frame in frames]
frames[0].save(out, save_all=True, append_images=frames[1:], duration=200,
               loop=0, optimize=False, disposal=1)
print(out)
