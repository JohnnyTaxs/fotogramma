"""Full validation and atomic replacement for verified H.265 outputs."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess

from fotogramma.core.media_tools import get_ffmpeg_path
from fotogramma.core.scanner import VideoInfo, ffprobe_environment, probe_video


def stream_counts(path: str, ffprobe_bin: str) -> dict[str, int]:
    result = subprocess.run(
        [ffprobe_bin, "-v", "error", "-show_entries", "stream=codec_type", "-of", "json", path],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        env=ffprobe_environment(),
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "Impossibile leggere gli stream")
    counts: dict[str, int] = {}
    for stream in json.loads(result.stdout).get("streams", []):
        codec_type = stream.get("codec_type") or "unknown"
        counts[codec_type] = counts.get(codec_type, 0) + 1
    return counts


def validate_conversion_output(original: VideoInfo, output: Path, ffprobe_bin: str) -> tuple[bool, str]:
    """Validate metadata, duration, streams and a full decode before replacement."""
    metadata = probe_video(str(output), str(output.parent), ffprobe_bin)
    if metadata.error or metadata.category != "compatible" or metadata.codec != "hevc":
        return False, metadata.error or "Il codec di output non è HEVC"

    if original.duration and metadata.duration:
        tolerance = max(1.0, original.duration * 0.01)
        if abs(original.duration - metadata.duration) > tolerance:
            return False, f"Durata non coerente: originale {original.duration:.2f}s, output {metadata.duration:.2f}s"
    elif original.duration and not metadata.duration:
        return False, "Durata del file convertito non disponibile"

    if stream_counts(original.path, ffprobe_bin) != stream_counts(str(output), ffprobe_bin):
        return False, "Il numero o il tipo di stream non coincide con l’originale"

    result = subprocess.run(
        [
            get_ffmpeg_path(),
            "-v",
            "error",
            "-nostats",
            "-i",
            str(output),
            "-f",
            "null",
            "-",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        env=ffprobe_environment(),
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0) if os.name == "nt" else 0,
        check=False,
    )
    if result.returncode != 0:
        details = (result.stderr or result.stdout).strip().splitlines()
        return False, f"Decodifica completa non riuscita: {details[-1] if details else 'errore sconosciuto'}"
    return True, "Verifica completa superata"


def replace_verified_output(original_path: str, output: Path, ffprobe_bin: str) -> tuple[bool, str]:
    """Atomically replace the original, restoring a backup on any failure."""
    source = Path(original_path)
    backup = source.with_name(source.name + ".fotogramma-backup")
    if backup.exists():
        return False, f"Backup già presente: {backup}. Spostalo o eliminalo prima di riprovare."
    try:
        os.replace(source, backup)
        os.replace(output, source)
        final = probe_video(str(source), str(source.parent), ffprobe_bin)
        if final.error or final.category != "compatible":
            raise RuntimeError(final.error or "Controllo finale del codec non riuscito")
        backup.unlink()
        return True, "Originale sostituito dopo verifica completa"
    except Exception as exc:
        if backup.exists():
            try:
                os.replace(backup, source)
            except OSError as restore_error:
                return False, f"{exc}. Ripristino non riuscito: {restore_error}. Backup: {backup}"
        return False, f"Originale non modificato: {exc}"
