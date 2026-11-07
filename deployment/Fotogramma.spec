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
STRIP = SYSTEM != "Windows"

# Qt plugins the app actually loads: Cocoa platform and the macOS style.
# TLS, network information, image formats, icon engines and input methods are unused.
KEPT_PLUGIN_CATEGORIES = {"platforms", "styles"}
DROPPED_PLUGIN_FILES = {"libqminimal.dylib"}

# Only the widgets the app imports; everything else stays out of the bundle.
EXCLUDED_MODULES = [
    "PySide6.QtBluetooth",
    "PySide6.QtCharts",
    "PySide6.QtConcurrent",
    "PySide6.QtDBus",
    "PySide6.QtDataVisualization",
    "PySide6.QtGraphs",
    "PySide6.QtHelp",
    "PySide6.QtHttpServer",
    "PySide6.QtLocation",
    "PySide6.QtMultimedia",
    "PySide6.QtMultimediaWidgets",
    "PySide6.QtNetwork",
    "PySide6.QtNfc",
    "PySide6.QtOpenGL",
    "PySide6.QtOpenGLWidgets",
    "PySide6.QtPdf",
    "PySide6.QtPdfWidgets",
    "PySide6.QtPositioning",
    "PySide6.QtPrintSupport",
    "PySide6.QtQml",
    "PySide6.QtQuick",
    "PySide6.QtQuickControls2",
    "PySide6.QtQuickWidgets",
    "PySide6.QtRemoteObjects",
    "PySide6.QtScxml",
    "PySide6.QtSensors",
    "PySide6.QtSerialBus",
    "PySide6.QtSerialPort",
    "PySide6.QtSpatialAudio",
    "PySide6.QtSql",
    "PySide6.QtStateMachine",
    "PySide6.QtSvg",
    "PySide6.QtSvgWidgets",
    "PySide6.QtTest",
    "PySide6.QtTextToSpeech",
    "PySide6.QtUiTools",
    "PySide6.QtWebChannel",
    "PySide6.QtWebEngineCore",
    "PySide6.QtWebEngineWidgets",
    "PySide6.QtWebSockets",
    "PySide6.QtXml",
]


def keep_binary(entry) -> bool:
    """Drop unused macOS Qt plugins; other platforms keep their full set."""
    destination = entry[0].replace("\\", "/")
    if SYSTEM != "Darwin" or not destination.startswith("PySide6/Qt/plugins/"):
        return True
    parts = destination.split("/")
    if len(parts) < 5:
        return True
    if parts[3] not in KEPT_PLUGIN_CATEGORIES:
        return False
    return parts[-1] not in DROPPED_PLUGIN_FILES


def keep_data(entry) -> bool:
    """Qt translations are never loaded: the app ships its own Italian strings."""
    return not entry[0].replace("\\", "/").startswith("PySide6/Qt/translations/")


for media_binary in (FFMPEG, FFPROBE):
    if not media_binary.is_file():
        raise RuntimeError(f"Binario mancante: {media_binary}. Esegui prima deployment/prepare_ffprobe.py")

if SYSTEM == "Darwin":
    ICON = ROOT / "deployment" / "Fotogramma.icns"
elif SYSTEM == "Windows":
    ICON = ROOT / "deployment" / "Fotogramma.ico"
else:
    ICON = ROOT / "deployment" / "Fotogramma.png"

binaries = [(str(path), ".") for path in (FFMPEG, FFPROBE)]
library_dir = VENDOR / "lib"
if library_dir.is_dir():
    # Destination "." merges these with the copies PyInstaller resolves as
    # dependencies; "lib" would ship every FFmpeg dylib twice.
    binaries.extend((str(path), ".") for path in library_dir.rglob("*") if path.is_file())

a = Analysis(
    [str(ROOT / "fotogramma" / "__main__.py")],
    pathex=[str(ROOT)],
    binaries=binaries,
    datas=[
        (str(ROOT / "static" / "fonts"), "static/fonts"),
        (str(ROOT / "static" / "icons" / "fotogramma.png"), "static/icons"),
    ],
    hiddenimports=["PySide6.QtCore", "PySide6.QtGui", "PySide6.QtWidgets"],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=EXCLUDED_MODULES,
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
    strip=STRIP,
    upx=False,
    console=False,
    argv_emulation=False,
    icon=str(ICON) if ICON.is_file() else None,
)

coll = COLLECT(
    exe,
    [binary for binary in a.binaries if keep_binary(binary)],
    [data for data in a.datas if keep_data(data)],
    strip=STRIP,
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
