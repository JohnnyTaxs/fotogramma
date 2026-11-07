# Native Qt onedir build for Fotogramma Desktop.
import os
from pathlib import Path
import platform

ROOT = Path.cwd()
SYSTEM = platform.system()
MACHINE = platform.machine().lower()
APP_VERSION = os.environ.get("FOTOGRAMMA_APP_VERSION") or "0.0.0"
VENDOR = ROOT / "deployment" / "vendor" / f"{SYSTEM}-{MACHINE}"
BINARY_NAME = "ffprobe.exe" if SYSTEM == "Windows" else "ffprobe"
FFMPEG = VENDOR / ("ffmpeg.exe" if SYSTEM == "Windows" else "ffmpeg")
FFPROBE = VENDOR / BINARY_NAME
MEDIA_BINARIES = [FFMPEG, FFPROBE]
for media_binary in MEDIA_BINARIES:
    if not media_binary.is_file():
        raise RuntimeError(f"Binario mancante: {media_binary}. Esegui prima deployment/prepare_ffprobe.py")

if SYSTEM == "Darwin":
    ICON = ROOT / "deployment" / "Fotogramma.icns"
elif SYSTEM == "Windows":
    ICON = ROOT / "deployment" / "Fotogramma.ico"
else:
    ICON = ROOT / "deployment" / "Fotogramma.png"

binaries = [(str(path), ".") for path in MEDIA_BINARIES]
library_dir = VENDOR / "lib"
if library_dir.is_dir():
    binaries.extend((str(path), "lib") for path in library_dir.rglob("*") if path.is_file())

a = Analysis(
    [str(ROOT / "fotogramma" / "__main__.py")],
    pathex=[str(ROOT)],
    binaries=binaries,
    datas=[(str(ROOT / "static"), "static")],
    hiddenimports=["PySide6.QtCore", "PySide6.QtGui", "PySide6.QtWidgets"],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=["PySide6.QtQml", "PySide6.QtQuick", "PySide6.QtWebEngineCore", "PySide6.QtWebEngineWidgets"],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="Fotogramma",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    console=False,
    argv_emulation=False,
    icon=str(ICON) if ICON.is_file() else None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    upx_exclude=[],
    name="Fotogramma",
)

if SYSTEM == "Darwin":
    app = BUNDLE(
        coll,
        name="Fotogramma.app",
        icon=str(ICON) if ICON.is_file() else None,
        bundle_identifier="net.tassoni.fotogramma.desktop",
        version=APP_VERSION,
        info_plist={"CFBundleVersion": APP_VERSION, "NSHumanReadableCopyright": "MIT License"},
    )
