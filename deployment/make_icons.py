"""Rebuild every app icon from deployment/Fotogramma.png (needs Pillow).

The master has anti-aliased corners whose colour is white, which left a pale
fringe. The corners are filled with the artwork's own purple, then a clean
rounded mask is cut at 4x and scaled down. Outputs:

* fotogramma.iconset/ and Fotogramma.icns: the macOS shape, 824 of 1024 px with
  a margin and a soft shadow, like other Dock icons;
* Fotogramma.ico and Fotogramma-window.png: a rounded square that fills the
  canvas (Windows taskbar, title bar, the logo inside the app).
"""
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

HERE = Path(__file__).resolve().parent
PURPLE = (59, 34, 89)
RADIUS = 0.2237  # of the side, the macOS icon curve (close enough to a squircle)


def artwork() -> Image.Image:
    master = Image.open(HERE / "Fotogramma.png").convert("RGBA")
    base = Image.new("RGBA", master.size, PURPLE + (255,))
    # Where the master is partly transparent its colour is white: drop it.
    solid = Image.composite(master, base, master.split()[3].point(lambda a: 255 if a > 200 else 0))
    return solid.convert("RGB")


def rounded(art: Image.Image, side: int) -> Image.Image:
    """The artwork cut to a rounded square of `side` px, with clean edges."""
    big = side * 4
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, big - 1, big - 1), int(big * RADIUS), fill=255)
    mask = mask.resize((side, side), Image.LANCZOS)
    out = art.resize((side, side), Image.LANCZOS).convert("RGBA")
    out.putalpha(mask)
    return out


def mac_icon(art: Image.Image) -> Image.Image:
    canvas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    shape = rounded(art, 824)
    shadow = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    shadow.paste((0, 0, 0, 90), (100, 112), shape.split()[3])
    shadow = shadow.filter(ImageFilter.GaussianBlur(14))
    canvas.alpha_composite(shadow)
    canvas.alpha_composite(shape, (100, 100))
    return canvas


def main() -> None:
    art = artwork()
    mac = mac_icon(art)
    iconset = HERE / "fotogramma.iconset"
    for name, px in [
        ("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64),
        ("64x64", 64), ("64x64@2x", 128), ("128x128", 128), ("128x128@2x", 256),
        ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024),
    ]:
        mac.resize((px, px), Image.LANCZOS).save(iconset / f"icon_{name}.png")
    if sys.platform == "darwin":
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(HERE / "Fotogramma.icns")], check=True)

    window = rounded(art, 1024)
    window.resize((256, 256), Image.LANCZOS).save(HERE / "Fotogramma-window.png")
    window.resize((256, 256), Image.LANCZOS).save(
        HERE / "Fotogramma.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )


if __name__ == "__main__":
    main()
