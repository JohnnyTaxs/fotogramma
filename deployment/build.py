"""Build the native desktop application and its portable ZIP archive."""

from __future__ import annotations

import platform
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "dist"
SYSTEM = platform.system()
MACHINE = platform.machine().lower()


def main() -> None:
    subprocess.run([sys.executable, "-m", "PyInstaller", "--version"], stdout=subprocess.DEVNULL, check=True)
    subprocess.run([sys.executable, str(ROOT / "deployment" / "prepare_ffprobe.py")], check=True)
    subprocess.run(
        [sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean", str(ROOT / "deployment" / "Fotogramma.spec")],
        cwd=ROOT,
        check=True,
    )
    artifact = DIST / "Fotogramma.app" if SYSTEM == "Darwin" else DIST / "Fotogramma"
    if not artifact.exists():
        raise RuntimeError(f"Artefatto non trovato: {artifact}")
    archive = shutil.make_archive(
        str(DIST / f"Fotogramma-{SYSTEM}-{MACHINE}"),
        "zip",
        root_dir=artifact.parent,
        base_dir=artifact.name,
    )
    print(f"Build nativa: {artifact}")
    print(f"Archivio ZIP: {archive}")


if __name__ == "__main__":
    main()
