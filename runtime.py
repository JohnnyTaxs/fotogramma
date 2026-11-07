"""Runtime paths shared by source and PyInstaller-frozen executions."""

from pathlib import Path
import sys

FROZEN = bool(getattr(sys, "frozen", False))
APP_ROOT = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent))


def resource_path(*parts: str) -> Path:
    """Return a path inside the source tree or the frozen bundle."""
    return APP_ROOT.joinpath(*parts)
