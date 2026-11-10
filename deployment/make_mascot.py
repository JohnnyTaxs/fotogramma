"""Draw the mascot that runs towards the TV on the conversion bar (needs Pillow).

Original pixel-art chibi girls in the Fotogramma colours, drawn in code, so the
art belongs to the project (MIT) and no one else's character is bundled.

    python deployment/make_mascot.py                 # writes mascot.png and tv.png
    python deployment/make_mascot.py --variant 2     # picks another girl
    python deployment/make_mascot.py --preview p.png # all variants, magnified

mascot.png is a strip of 8 frames of W x H pixels: 6 of a run cycle facing right
and 2 of her cheering. tv.png is 2 frames of TV_W x TV_H: switched off, then on.
"""
import argparse
import math
from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
W, H = 40, 48
RUN, CHEER = 6, 2
TV_W, TV_H = 40, 40

INK = (24, 10, 32, 255)
SKIN = (255, 224, 205, 255)
SKIN_SHADE = (240, 196, 180, 255)
WHITE = (255, 255, 255, 255)
CORAL = (255, 148, 130, 255)
CORAL_DARK = (214, 106, 96, 255)
LAVENDER = (212, 180, 219, 255)
MINT = (119, 221, 187, 255)
BLUSH = (255, 160, 170, 255)

VARIANTS = {
    # 1: violet ponytail, coral jacket, a film-frame hair clip
    1: dict(style="ponytail", hair=(74, 44, 107, 255), light=(126, 88, 168, 255),
            top=CORAL, top_dark=CORAL_DARK, skirt=(112, 70, 146, 255), iris=CORAL,
            extra="clip"),
    # 2: teal bob, a coral beret, a lavender hoodie
    2: dict(style="bob", hair=(48, 150, 140, 255), light=(110, 205, 190, 255),
            top=LAVENDER, top_dark=(170, 138, 182, 255), skirt=(60, 52, 110, 255), iris=MINT,
            extra="beret"),
    # 3: long silver hair, a coral ribbon, a dark dress
    3: dict(style="long", hair=(214, 216, 236, 255), light=(255, 255, 255, 255),
            top=(70, 48, 104, 255), top_dark=(52, 34, 82, 255), skirt=(70, 48, 104, 255),
            iris=LAVENDER, extra="ribbon"),
}


def girl(index: int, v: dict, cheer: bool = False) -> Image.Image:
    phase = index / RUN * 2 * math.pi
    swing = 0.0 if cheer else math.sin(phase)
    bob = (3 if index == 0 else 0) if cheer else (1 if index % 3 == 1 else 0)
    sway = round(math.sin(phase - 0.8) * 2)
    y0 = -bob
    hair, light, ink = v["hair"], v["light"], INK

    image = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(image)

    # back arm
    if cheer:
        d.rectangle((17, 18 + y0, 20, 28 + y0), fill=v["top_dark"], outline=ink)
        d.rectangle((17, 15 + y0, 20, 18 + y0), fill=SKIN_SHADE, outline=ink)
    else:
        ax = 19 - round(swing * 5)
        d.rectangle((ax, 29 + y0, ax + 3, 35 + y0), fill=v["top_dark"], outline=ink)
        d.rectangle((ax, 36 + y0, ax + 3, 38 + y0), fill=SKIN_SHADE, outline=ink)

    # legs
    for sign, shade in ((-1, SKIN_SHADE), (1, SKIN)):
        lx = (20 + sign * 2) if cheer else 20 + round(swing * sign * 6)
        lift = 0 if cheer else (2 if (swing * sign) > 0.5 else 0)
        top = 38 + (bob if cheer else 0) * 0
        d.rectangle((lx, 38, lx + 3, 43 - lift), fill=shade, outline=ink)
        d.rectangle((lx, 41 - lift, lx + 3, 43 - lift), fill=WHITE, outline=ink)
        d.rectangle((lx - 1, 44 - lift, lx + 5, 46 - lift), fill=CORAL_DARK, outline=ink)

    # skirt and torso
    d.polygon([(14, 35 + y0), (28, 35 + y0), (31, 40 + y0), (11, 40 + y0)],
              fill=v["skirt"], outline=ink)
    for k in range(5):  # film-strip hem
        d.point((13 + k * 4, 39 + y0), fill=LAVENDER)
        d.point((14 + k * 4, 39 + y0), fill=LAVENDER)
    d.rectangle((15, 28 + y0, 27, 35 + y0), fill=v["top"], outline=ink)
    d.line([(16, 32 + y0), (26, 32 + y0)], fill=v["top_dark"])
    if v["extra"] == "ribbon":
        d.polygon([(20, 28 + y0), (24, 28 + y0), (22, 32 + y0)], fill=CORAL, outline=ink)

    # front arm
    if cheer:
        d.rectangle((24, 18 + y0, 27, 28 + y0), fill=v["top"], outline=ink)
        d.rectangle((24, 15 + y0, 27, 18 + y0), fill=SKIN, outline=ink)
    else:
        fx = 24 + round(swing * 5)
        d.rectangle((fx, 29 + y0, fx + 3, 35 + y0), fill=v["top"], outline=ink)
        d.rectangle((fx, 36 + y0, fx + 3, 38 + y0), fill=SKIN, outline=ink)

    # hair behind the head
    style = v["style"]
    if style == "ponytail":
        d.polygon([(11, 14 + y0), (8 - sway, 22 + y0), (6 - sway * 2, 31 + y0),
                   (11 - sway, 35 + y0), (14, 27 + y0), (16, 20 + y0)], fill=hair, outline=ink)
        d.line([(10 - sway, 20 + y0), (9 - sway, 28 + y0)], fill=light)
    elif style == "long":
        d.polygon([(11, 14 + y0), (7 - sway, 24 + y0), (6 - sway * 2, 38 + y0),
                   (13 - sway, 40 + y0), (16, 30 + y0), (17, 20 + y0)], fill=hair, outline=ink)
        d.line([(9 - sway, 22 + y0), (9 - sway * 2, 34 + y0)], fill=light)
        d.polygon([(8, 15 + y0), (13, 13 + y0), (13, 19 + y0), (8, 19 + y0)],
                  fill=CORAL, outline=ink)
    else:  # bob
        d.polygon([(8, 14 + y0), (7 - sway, 24 + y0), (14 - sway, 28 + y0), (17, 20 + y0)],
                  fill=hair, outline=ink)
    d.ellipse((6, 4 + y0, 33, 27 + y0), fill=hair, outline=ink)

    # face and fringe
    d.ellipse((13, 9 + y0, 35, 29 + y0), fill=SKIN, outline=ink)
    d.polygon([(11, 12 + y0), (20, 5 + y0), (31, 8 + y0), (35, 15 + y0), (30, 13 + y0),
               (26, 17 + y0), (22, 13 + y0), (18, 18 + y0), (14, 15 + y0)], fill=hair, outline=ink)
    d.line([(17, 8 + y0), (24, 7 + y0)], fill=light)
    if v["extra"] == "clip":
        d.rectangle((13, 6 + y0, 18, 10 + y0), fill=LAVENDER, outline=ink)
        d.point([(14, 8 + y0), (16, 8 + y0)], fill=ink)
    elif v["extra"] == "beret":
        d.ellipse((10, 1 + y0, 30, 9 + y0), fill=CORAL, outline=ink)
        d.point((20, 0 + y0), fill=ink)
    # eye: big, with a highlight; blinks on one run frame and smiles when cheering
    if cheer:
        d.arc((25, 18 + y0, 31, 25 + y0), 200, 340, fill=ink)
        d.line([(27, 21 + y0), (30, 21 + y0)], fill=ink)
    elif index == 4:
        d.line([(26, 21 + y0), (30, 21 + y0)], fill=ink)
    else:
        d.rectangle((26, 17 + y0, 30, 24 + y0), fill=ink)
        d.rectangle((27, 20 + y0, 29, 23 + y0), fill=v["iris"])
        d.rectangle((27, 18 + y0, 28, 19 + y0), fill=WHITE)
    d.rectangle((22, 24 + y0, 24, 25 + y0), fill=BLUSH)
    if cheer:
        d.rectangle((31, 25 + y0, 33, 27 + y0), fill=INK)  # an open, happy mouth
    else:
        d.point([(32, 25 + y0), (33, 25 + y0)], fill=ink)
    return image


def tv(on: bool) -> Image.Image:
    image = Image.new("RGBA", (TV_W, TV_H), (0, 0, 0, 0))
    d = ImageDraw.Draw(image)
    if on:  # a soft glow around the set
        for r, a in ((19, 30), (17, 55)):
            d.rounded_rectangle((20 - r, 18 - r + 2, 20 + r, 18 + r - 1), 6, fill=(255, 148, 130, a))
    d.line([(14, 8), (10, 2)], fill=INK)  # antenna
    d.line([(26, 8), (30, 2)], fill=INK)
    d.point([(10, 2), (30, 2)], fill=CORAL)
    d.rounded_rectangle((3, 7, 36, 33), 4, fill=(112, 70, 146, 255), outline=INK)
    d.rectangle((6, 10, 26, 30), fill=INK)  # screen bezel
    if on:
        colours = [WHITE, (255, 226, 130, 255), CORAL, MINT, LAVENDER, (150, 110, 200, 255)]
        for i, colour in enumerate(colours):
            d.rectangle((7 + i * 3 + (1 if i else 0), 11, 9 + i * 3 + (1 if i else 0), 29), fill=colour)
        for y in range(12, 29, 3):  # scanlines
            d.line([(7, y), (25, y)], fill=(24, 10, 32, 70))
    else:
        d.rectangle((7, 11, 25, 29), fill=(33, 24, 46, 255))
        d.line([(9, 13), (13, 13)], fill=(96, 78, 120, 255))  # a glint on the glass
        d.line([(9, 15), (11, 15)], fill=(96, 78, 120, 255))
    d.rectangle((29, 13, 33, 14), fill=CORAL if on else (90, 60, 110, 255))  # knobs
    d.rectangle((29, 18, 33, 19), fill=LAVENDER)
    d.rectangle((9, 34, 12, 37), fill=INK)  # feet
    d.rectangle((27, 34, 30, 37), fill=INK)
    return image


def strip(v: dict) -> Image.Image:
    sheet = Image.new("RGBA", (W * (RUN + CHEER), H), (0, 0, 0, 0))
    for i in range(RUN):
        sheet.alpha_composite(girl(i, v), (i * W, 0))
    for i in range(CHEER):
        sheet.alpha_composite(girl(i, v, cheer=True), ((RUN + i) * W, 0))
    return sheet


def tv_strip() -> Image.Image:
    sheet = Image.new("RGBA", (TV_W * 2, TV_H), (0, 0, 0, 0))
    sheet.alpha_composite(tv(False), (0, 0))
    sheet.alpha_composite(tv(True), (TV_W, 0))
    return sheet


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--variant", type=int, default=1, choices=sorted(VARIANTS))
    parser.add_argument("--preview", help="write a magnified sheet of every variant here")
    args = parser.parse_args()
    if args.preview:
        scale, ground = 3, (40, 21, 47, 255)
        rows = [strip(VARIANTS[k]) for k in sorted(VARIANTS)]
        tvs = tv_strip()
        sheet = Image.new("RGBA", (rows[0].width + tvs.width, H * len(rows)), ground)
        for r, row in enumerate(rows):
            sheet.alpha_composite(row, (0, r * H))
        for r in range(len(rows)):
            sheet.alpha_composite(tvs, (rows[0].width, r * H + 4))
        sheet.resize((sheet.width * scale, sheet.height * scale), Image.NEAREST).save(args.preview)
        return
    strip(VARIANTS[args.variant]).save(HERE / "mascot.png")
    tv_strip().save(HERE / "tv.png")


if __name__ == "__main__":
    main()
