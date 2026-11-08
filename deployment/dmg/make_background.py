"""Draw the DMG window background (needs Pillow; the result is committed).

    python3 deployment/dmg/make_background.py

Writes ``background.png`` (660x400) and ``background@2x.png`` (1320x800). The
icon slots match ``settings.py``: the app at x=165, Applications at x=495.
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = Path(__file__).resolve().parent
FONT = HERE.parents[1] / "static" / "fonts" / "Sora-Variable.ttf"
WIDTH, HEIGHT = 660, 400
SCALE = 2

BG_TOP = (0x35, 0x1A, 0x40)
BG_BOTTOM = (0x21, 0x11, 0x27)
CORAL = (0xFF, 0x94, 0x82)
LAVENDER = (0xD4, 0xB4, 0xDB)
TEXT = (0xFC, 0xEC, 0xF6)
MUTED = (0xC7, 0xA9, 0xC5)

APP_X, APPLICATIONS_X, ICON_Y = 165, 495, 205


def font(size: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(str(FONT), size * SCALE)


def bezier(p0, p1, p2, p3, steps=80):
    points = []
    for index in range(steps + 1):
        t = index / steps
        u = 1 - t
        points.append(
            (
                u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0],
                u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1],
            )
        )
    return points


def draw(scale: int) -> Image.Image:
    width, height = WIDTH * scale, HEIGHT * scale
    image = Image.new("RGB", (width, height))
    pixels = image.load()
    for y in range(height):
        mix = y / (height - 1)
        row = tuple(round(BG_TOP[i] * (1 - mix) + BG_BOTTOM[i] * mix) for i in range(3))
        for x in range(width):
            pixels[x, y] = row
    canvas = ImageDraw.Draw(image)

    def at(value):
        return value * scale

    title = ImageFont.truetype(str(FONT), 30 * scale)
    small = ImageFont.truetype(str(FONT), 12 * scale)
    caption = ImageFont.truetype(str(FONT), 13 * scale)
    canvas.text((at(28), at(22)), "Fotogramma", font=title, fill=TEXT)
    canvas.text((at(30), at(64)), "H.265 cinema library", font=small, fill=MUTED)

    # A hand-drawn curve from the app slot towards the Applications slot.
    start = (at(APP_X + 85), at(ICON_Y - 4))
    end = (at(APPLICATIONS_X - 92), at(ICON_Y - 4))
    curve = bezier(
        start,
        (at(APP_X + 120), at(ICON_Y - 60)),
        (at(APPLICATIONS_X - 130), at(ICON_Y + 50)),
        end,
    )
    glow = Image.new("RGBA", image.size, (0, 0, 0, 0))
    ImageDraw.Draw(glow).line(curve, fill=CORAL + (120,), width=at(9), joint="curve")
    glow = glow.filter(ImageFilter.GaussianBlur(at(5)))
    image.paste(glow, (0, 0), glow)
    canvas = ImageDraw.Draw(image)
    for index in range(len(curve) - 1):
        mix = index / (len(curve) - 1)
        color = tuple(round(CORAL[i] * (1 - mix) + LAVENDER[i] * mix) for i in range(3))
        canvas.line([curve[index], curve[index + 1]], fill=color, width=at(4))
    # Arrow head, aligned with the end of the curve.
    tip = curve[-1]
    before = curve[-4]
    angle = math.atan2(tip[1] - before[1], tip[0] - before[0])
    size = at(15)
    head = [
        tip,
        (
            tip[0] - size * math.cos(angle - 0.45),
            tip[1] - size * math.sin(angle - 0.45),
        ),
        (
            tip[0] - size * math.cos(angle + 0.45),
            tip[1] - size * math.sin(angle + 0.45),
        ),
    ]
    canvas.polygon(head, fill=LAVENDER)

    text = "Drag Fotogramma to Applications"
    box = canvas.textbbox((0, 0), text, font=caption)
    canvas.text(
        ((width - (box[2] - box[0])) / 2, at(ICON_Y + 118)),
        text,
        font=caption,
        fill=MUTED,
    )
    return image


def main() -> None:
    draw(1).save(HERE / "background.png", dpi=(72, 72), optimize=True)
    draw(SCALE).save(HERE / "background@2x.png", dpi=(144, 144), optimize=True)
    print("wrote", HERE / "background.png", HERE / "background@2x.png")


if __name__ == "__main__":
    main()
