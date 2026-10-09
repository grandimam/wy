#!/usr/bin/env python3
"""Render a wy terminal preview (WY_TUI_PREVIEW_DIR JSON) to a PNG.

wy uses the terminal's own colours, so this maps the named colours to one
common dark palette. Usage: render-preview.py preview.json out.png
"""
import json
import sys

from PIL import Image, ImageDraw, ImageFont

PALETTE = {
    None: (214, 214, 214),  # default text
    "Cyan": (86, 182, 194),
    "Green": (152, 195, 121),
    "Red": (224, 108, 117),
    "Yellow": (229, 192, 123),
    "DarkGray": (122, 130, 142),
}
BACKGROUND = (30, 33, 39)
CELL_W, CELL_H, PAD = 10, 22, 24


def colour(value):
    if isinstance(value, list):
        return tuple(value)
    return PALETTE.get(value, PALETTE[None])


def main(source, target):
    data = json.load(open(source))
    width, height, cells = data["width"], data["height"], data["cells"]
    try:
        regular = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 16, index=0)
        bold = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 16, index=1)
    except OSError:
        regular = bold = ImageFont.load_default()
    image = Image.new("RGB", (width * CELL_W + 2 * PAD, height * CELL_H + 2 * PAD), BACKGROUND)
    draw = ImageDraw.Draw(image)
    for index, cell in enumerate(cells):
        x = PAD + (index % width) * CELL_W
        y = PAD + (index // width) * CELL_H
        fg, bg = colour(cell["fg"]), (colour(cell["bg"]) if cell["bg"] is not None else BACKGROUND)
        if cell.get("reversed"):
            fg, bg = BACKGROUND, fg
        if bg != BACKGROUND:
            draw.rectangle([x, y, x + CELL_W, y + CELL_H], fill=bg)
        text = cell["text"]
        if text.strip():
            draw.text((x, y + 2), text, fill=fg, font=bold if cell.get("bold") else regular)
    image.save(target)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
