"""Build the Rust desktop app and its portable ZIP archive.

The script never cross-compiles: it runs on the target operating system and
packages the matching vendored FFmpeg toolchain from
``deployment/vendor/rust/<System>-<machine>``.

    python3 deployment/build_rust.py

On macOS it also writes a drag-to-Applications ``.dmg`` (needs
``pip install dmgbuild``); the ZIP stays because the self-updater installs it.

Environment:
    FOTOGRAMMA_VERSION   overrides the version embedded in the bundle
    FOTOGRAMMA_RUST_SKIP_BUILD  set to 1 to reuse an existing release binary
    FOTOGRAMMA_REQUIRE_DMG      set to 1 to fail when the DMG cannot be built
"""

from __future__ import annotations

import os
import platform
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT
DIST = ROOT / "dist"
SYSTEM = platform.system()
MACHINE = platform.machine().lower()
VENDOR = ROOT / "deployment" / "vendor" / "rust" / f"{SYSTEM}-{MACHINE}"
BINARY_NAME = "fotogramma.exe" if SYSTEM == "Windows" else "fotogramma"
MAX_APP_BYTES = int(os.environ.get("FOTOGRAMMA_MAX_APP_BYTES", 45_000_000))
MAX_ZIP_BYTES = int(os.environ.get("FOTOGRAMMA_MAX_ZIP_BYTES", 18_000_000))
MAX_DMG_BYTES = int(os.environ.get("FOTOGRAMMA_MAX_DMG_BYTES", 18_000_000))
DMG_DIR = ROOT / "deployment" / "dmg"
# Icon slots in the DMG window; they match the arrow in background.png.
DMG_WINDOW = (660, 400)
DMG_APP_POSITION = (165, 205)
DMG_APPLICATIONS_POSITION = (495, 205)


def workspace_version() -> str:
    """Read the version from the Cargo workspace so the bundle cannot drift."""
    text = (RUST / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not match:
        raise RuntimeError("Cannot read the workspace version from Cargo.toml")
    override = os.environ.get("FOTOGRAMMA_VERSION", "").strip()
    return override.lstrip("v") if override else match.group(1)


def directory_size(path: Path) -> int:
    return sum(item.stat().st_size for item in path.rglob("*") if item.is_file())


def cargo_build() -> Path:
    if os.environ.get("FOTOGRAMMA_RUST_SKIP_BUILD") != "1":
        subprocess.run(
            ["cargo", "build", "--release", "--locked", "--package", "fotogramma"],
            cwd=RUST,
            check=True,
        )
    binary = RUST / "target" / "release" / BINARY_NAME
    if not binary.is_file():
        raise RuntimeError(f"Release binary not found: {binary}")
    return binary


def check_vendor() -> tuple[Path, Path, Path]:
    binaries = VENDOR / "bin"
    libraries = VENDOR / "lib"
    ffmpeg = binaries / ("ffmpeg.exe" if SYSTEM == "Windows" else "ffmpeg")
    ffprobe = binaries / ("ffprobe.exe" if SYSTEM == "Windows" else "ffprobe")
    for required in (ffmpeg, ffprobe):
        if not required.exists():
            raise RuntimeError(
                f"Missing vendored media tool: {required}. "
                "Build the minimal FFmpeg toolchain first (deployment/ffmpeg/README.md)."
            )
    if SYSTEM != "Windows" and not libraries.is_dir():
        raise RuntimeError(
            f"Missing vendored media libraries: {libraries}. "
            "Build the minimal FFmpeg toolchain first (deployment/ffmpeg/README.md)."
        )
    return ffmpeg, ffprobe, libraries


def copy_media_tools(destination: Path, flat_libraries: bool = False) -> None:
    """Copy ``vendor/bin`` and ``vendor/lib`` into the application layout.

    Windows needs every DLL next to the executables, so ``flat_libraries``
    puts them in the same folder; Linux keeps ``lib/`` beside ``bin/`` and the
    binaries use an ``$ORIGIN/../lib`` rpath.
    """
    _, _, libraries = check_vendor()
    binaries = destination / "bin"
    binaries.mkdir(parents=True, exist_ok=True)
    # The vendor bin folder holds the executables and, on Windows, the DLLs.
    for item in (VENDOR / "bin").iterdir():
        if item.is_file():
            shutil.copy2(item, binaries / item.name)
    if libraries.is_dir():
        target = binaries if flat_libraries else destination / "lib"
        target.mkdir(parents=True, exist_ok=True)
        for item in libraries.iterdir():
            if item.is_file():
                shutil.copy2(item, target / item.name)


def macos_bundle(binary: Path, version: str) -> Path:
    app = DIST / "Fotogramma.app"
    if app.exists():
        shutil.rmtree(app)
    contents = app / "Contents"
    macos = contents / "MacOS"
    resources = contents / "Resources"
    macos.mkdir(parents=True)
    resources.mkdir(parents=True)

    target = macos / "fotogramma"
    shutil.copy2(binary, target)
    target.chmod(0o755)
    shutil.copy2(ROOT / "deployment" / "Fotogramma.icns", resources / "Fotogramma.icns")
    copy_media_tools(resources)

    info = {
        "CFBundleName": "Fotogramma",
        "CFBundleDisplayName": "Fotogramma",
        "CFBundleIdentifier": "net.tassoni.fotogramma.desktop",
        "CFBundleExecutable": "fotogramma",
        "CFBundlePackageType": "APPL",
        "CFBundleIconFile": "Fotogramma",
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "LSMinimumSystemVersion": "11.0",
        "NSHighResolutionCapable": True,
        "NSHumanReadableCopyright": "MIT License",
    }
    (contents / "Info.plist").write_bytes(plistlib.dumps(info))

    subprocess.run(
        ["codesign", "--force", "--deep", "--sign", "-", "--timestamp=none", str(app)],
        check=True,
    )
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    return app


def linux_bundle(binary: Path) -> Path:
    directory = DIST / "Fotogramma"
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    target = directory / "fotogramma"
    shutil.copy2(binary, target)
    target.chmod(0o755)
    copy_media_tools(directory)
    for tool in (directory / "bin").iterdir():
        tool.chmod(0o755)
    return directory


def windows_bundle(binary: Path) -> Path:
    directory = DIST / "Fotogramma"
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    shutil.copy2(binary, directory / "fotogramma.exe")
    copy_media_tools(directory, flat_libraries=True)
    return directory


def build_archive(artifact: Path) -> Path:
    stem = DIST / f"Fotogramma-{SYSTEM}-{MACHINE}"
    archive = stem.with_suffix(".zip")
    archive.unlink(missing_ok=True)
    if SYSTEM == "Darwin":
        subprocess.run(
            [
                "ditto",
                "-c",
                "-k",
                "--sequesterRsrc",
                "--keepParent",
                str(artifact),
                str(archive),
            ],
            check=True,
        )
    else:
        shutil.make_archive(str(stem), "zip", root_dir=artifact.parent, base_dir=artifact.name)
    return archive


def verify_macos_archive(artifact: Path, archive: Path) -> None:
    """The ZIP round-trip must keep symlinks, signatures and permissions."""
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(artifact)], check=True)
    with tempfile.TemporaryDirectory() as directory:
        subprocess.run(["ditto", "-x", "-k", str(archive), directory], check=True)
        extracted = Path(directory) / artifact.name
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(extracted)], check=True)
        for relative in ("Contents/MacOS/fotogramma", "Contents/Resources/bin/ffmpeg"):
            if not (extracted / relative).is_file():
                raise RuntimeError(f"Archive is missing {relative}")


def build_dmg(app: Path, version: str) -> Path | None:
    """A disk image with the app and an Applications shortcut to drag it onto."""
    try:
        import dmgbuild
    except ImportError:
        if os.environ.get("FOTOGRAMMA_REQUIRE_DMG") == "1":
            raise RuntimeError("dmgbuild is required: pip install dmgbuild")
        print("dmgbuild is not installed: skipping the DMG (pip install dmgbuild)")
        return None

    dmg = DIST / f"Fotogramma-{SYSTEM}-{MACHINE}.dmg"
    dmg.unlink(missing_ok=True)
    with tempfile.TemporaryDirectory() as directory:
        # One TIFF with both resolutions, so the background stays sharp on Retina.
        background = Path(directory) / "background.tiff"
        subprocess.run(
            [
                "tiffutil",
                "-cathidpicheck",
                str(DMG_DIR / "background.png"),
                str(DMG_DIR / "background@2x.png"),
                "-out",
                str(background),
            ],
            check=True,
        )
        settings = {
            "format": "UDZO",
            "compression_level": 9,
            "files": [str(app)],
            "symlinks": {"Applications": "/Applications"},
            "icon": str(ROOT / "deployment" / "Fotogramma.icns"),
            "background": str(background),
            "icon_size": 128,
            "text_size": 13,
            "window_rect": ((200, 140), DMG_WINDOW),
            "default_view": "icon-view",
            "show_icon_preview": False,
            "show_status_bar": False,
            "show_tab_view": False,
            "show_toolbar": False,
            "show_pathbar": False,
            "show_sidebar": False,
            "arrange_by": None,
            "icon_locations": {
                app.name: DMG_APP_POSITION,
                "Applications": DMG_APPLICATIONS_POSITION,
            },
        }
        dmgbuild.build_dmg(str(dmg), "Fotogramma", settings=settings)
    return dmg


def verify_dmg(dmg: Path) -> None:
    """Mount the image and check what a user will see and drag."""
    with tempfile.TemporaryDirectory() as directory:
        mountpoint = Path(directory) / "volume"
        mountpoint.mkdir()
        subprocess.run(
            ["hdiutil", "attach", "-nobrowse", "-readonly", "-mountpoint", str(mountpoint), str(dmg)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        try:
            app = mountpoint / "Fotogramma.app"
            link = mountpoint / "Applications"
            if not (app / "Contents" / "MacOS" / "fotogramma").is_file():
                raise RuntimeError("The DMG does not contain Fotogramma.app")
            if not link.is_symlink() or os.readlink(link) != "/Applications":
                raise RuntimeError("The DMG has no Applications shortcut")
            if not (mountpoint / ".DS_Store").is_file():
                raise RuntimeError("The DMG window layout (.DS_Store) is missing")
            subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
        finally:
            subprocess.run(["hdiutil", "detach", "-force", str(mountpoint)], check=False, stdout=subprocess.DEVNULL)


def report_largest(artifact: Path, count: int = 12) -> None:
    """Show what dominates the bundle, so a budget failure is actionable."""
    files = sorted(
        (
            (item.stat().st_size, item)
            for item in artifact.rglob("*")
            if item.is_file()
        ),
        reverse=True,
    )
    print(f"Bundle contents ({len(files)} files):")
    for size, item in files[:count]:
        print(f"  {size / 1_000_000:6.2f} MB  {item.relative_to(artifact)}")


def enforce_budget(artifact: Path, archive: Path) -> None:
    app_bytes = directory_size(artifact)
    zip_bytes = archive.stat().st_size
    print(f"App: {app_bytes / 1_000_000:.1f} MB (budget {MAX_APP_BYTES / 1_000_000:.0f} MB)")
    print(f"ZIP: {zip_bytes / 1_000_000:.1f} MB (budget {MAX_ZIP_BYTES / 1_000_000:.0f} MB)")
    report_largest(artifact)
    if app_bytes > MAX_APP_BYTES:
        raise RuntimeError("Application exceeds the size budget")
    if zip_bytes > MAX_ZIP_BYTES:
        raise RuntimeError("ZIP archive exceeds the size budget")


def main() -> int:
    version = workspace_version()
    binary = cargo_build()
    DIST.mkdir(exist_ok=True)
    if SYSTEM == "Darwin":
        artifact = macos_bundle(binary, version)
    elif SYSTEM == "Linux":
        artifact = linux_bundle(binary)
    elif SYSTEM == "Windows":
        artifact = windows_bundle(binary)
    else:
        raise SystemExit(f"Unsupported system: {SYSTEM}")

    archive = build_archive(artifact)
    if SYSTEM == "Darwin":
        verify_macos_archive(artifact, archive)
    enforce_budget(artifact, archive)
    if SYSTEM == "Darwin":
        dmg = build_dmg(artifact, version)
        if dmg is not None:
            verify_dmg(dmg)
            dmg_bytes = dmg.stat().st_size
            print(f"DMG: {dmg_bytes / 1_000_000:.1f} MB (budget {MAX_DMG_BYTES / 1_000_000:.0f} MB)")
            if dmg_bytes > MAX_DMG_BYTES:
                raise RuntimeError("DMG exceeds the size budget")
            print(f"Disk image: {dmg}")
    print(f"Version: {version}")
    print(f"Bundle: {artifact}")
    print(f"Archive: {archive}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
