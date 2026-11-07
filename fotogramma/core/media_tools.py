"""Locate the ffmpeg executable in source and frozen applications."""

from __future__ import annotations

import os
import shutil

from runtime import FROZEN, resource_path


def get_ffmpeg_path() -> str:
    executable = "ffmpeg.exe" if os.name == "nt" else "ffmpeg"
    if FROZEN:
        bundled = resource_path(executable)
        if bundled.is_file():
            return str(bundled)

    system_binary = shutil.which("ffmpeg")
    if system_binary:
        return system_binary
    for candidate in (os.path.join(os.getcwd(), executable), str(resource_path(executable))):
        if os.path.isfile(candidate):
            return candidate
    raise FileNotFoundError(
        "ffmpeg non trovato. Installa FFmpeg oppure usa la build portabile che lo include."
    )
