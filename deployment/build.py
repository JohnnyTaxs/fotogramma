"""Build the native desktop application and its portable ZIP archive."""

from __future__ import annotations

import os
import platform
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "dist"
SYSTEM = platform.system()
MACHINE = platform.machine().lower()
VERSION_PATTERN = re.compile(r"v?(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)")


def app_version() -> str:
    """Return the version to embed in the bundle; a release tag wins over the package."""
    match = VERSION_PATTERN.fullmatch(os.environ.get("FOTOGRAMMA_VERSION", "").strip())
    if match:
        return match.group(1)
    sys.path.insert(0, str(ROOT))
    from fotogramma import __version__

    return __version__


def build_archive(artifact: Path) -> Path:
    """Create the portable ZIP, preserving macOS symlinks so signatures stay valid."""
    stem = DIST / f"Fotogramma-{SYSTEM}-{MACHINE}"
    archive = stem.with_suffix(".zip")
    archive.unlink(missing_ok=True)
    if SYSTEM == "Darwin":
        subprocess.run(
            ["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(artifact), str(archive)],
            check=True,
        )
    else:
        shutil.make_archive(str(stem), "zip", root_dir=artifact.parent, base_dir=artifact.name)
    return archive


def verify_macos_archive(artifact: Path, archive: Path) -> None:
    """Fail the build when the ZIP loses symlinks or breaks the ad-hoc signature."""
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(artifact)], check=True)
    with tempfile.TemporaryDirectory() as directory:
        subprocess.run(["ditto", "-x", "-k", str(archive), directory], check=True)
        extracted = Path(directory) / artifact.name
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(extracted)], check=True)


def main() -> None:
    version = app_version()
    subprocess.run([sys.executable, "-m", "PyInstaller", "--version"], stdout=subprocess.DEVNULL, check=True)
    subprocess.run([sys.executable, str(ROOT / "deployment" / "prepare_ffprobe.py")], check=True)
    subprocess.run(
        [sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean", str(ROOT / "deployment" / "Fotogramma.spec")],
        cwd=ROOT,
        check=True,
        env={**os.environ, "FOTOGRAMMA_APP_VERSION": version},
    )
    artifact = DIST / "Fotogramma.app" if SYSTEM == "Darwin" else DIST / "Fotogramma"
    if not artifact.exists():
        raise RuntimeError(f"Artefatto non trovato: {artifact}")
    archive = build_archive(artifact)
    if SYSTEM == "Darwin":
        verify_macos_archive(artifact, archive)
    print(f"Versione: {version}")
    print(f"Build nativa: {artifact}")
    print(f"Archivio ZIP: {archive}")


if __name__ == "__main__":
    main()
