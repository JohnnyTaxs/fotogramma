"""Safe H.265 conversion profiles, output allocation and command generation."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class ConversionProfile:
    key: str
    label: str
    description: str
    crf: int
    preset: str
    pixel_format: str = "yuv420p10le"
    tune: str | None = None
    aq_mode: int | None = None
    audio_mode: str = "copy"
    audio_bitrate: int = 160


PROFILES = {
    "balanced": ConversionProfile(
        "balanced",
        "Film e serie TV · Bilanciato",
        "Qualità elevata e dimensioni contenute. Video 10-bit, audio originale.",
        crf=22,
        preset="medium",
    ),
    "quality": ConversionProfile(
        "quality",
        "Alta qualità · Film",
        "Più dettaglio e file più grandi. Video 10-bit, audio originale.",
        crf=20,
        preset="slow",
    ),
    "anime": ConversionProfile(
        "anime",
        "Anime · gradienti e linee",
        "Preset animation e AQ mode 3 per ridurre banding e artefatti.",
        crf=23,
        preset="slow",
        tune="animation",
        aq_mode=3,
    ),
    "light": ConversionProfile(
        "light",
        "Leggero · spazio ridotto",
        "Compressione più forte per archivi meno pesanti. Video 10-bit, audio originale.",
        crf=26,
        preset="medium",
    ),
}


def custom_profile(
    crf: int,
    preset: str,
    pixel_format: str,
    tune: str | None,
    aq_mode: int | None,
    audio_mode: str,
    audio_bitrate: int,
) -> ConversionProfile:
    return ConversionProfile(
        "custom",
        "Personalizzato",
        "Impostazioni scelte manualmente.",
        crf=crf,
        preset=preset,
        pixel_format=pixel_format,
        tune=tune or None,
        aq_mode=aq_mode,
        audio_mode=audio_mode,
        audio_bitrate=audio_bitrate,
    )


def build_ffmpeg_args(profile: ConversionProfile, source: str, destination: str) -> list[str]:
    args = [
        "-n",
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostats",
        "-i",
        source,
        "-map",
        "0",
        "-c:v",
        "libx265",
        "-preset",
        profile.preset,
        "-crf",
        str(profile.crf),
        "-pix_fmt",
        profile.pixel_format,
    ]
    if profile.tune:
        args.extend(["-tune", profile.tune])
    if profile.aq_mode is not None:
        args.extend(["-x265-params", f"aq-mode={profile.aq_mode}"])
    if profile.audio_mode == "copy":
        args.extend(["-c:a", "copy"])
    else:
        args.extend(["-c:a", "aac", "-b:a", f"{profile.audio_bitrate}k"])
    args.extend(["-c:s", "copy", "-c:d", "copy", "-map_metadata", "0", "-map_chapters", "0"])
    if destination.lower().endswith(".mp4"):
        args.extend(["-movflags", "+faststart"])
    args.extend(["-progress", "pipe:1", destination])
    return args


def allocate_output_path(source: str, destination_dir: str, container: str = "mkv") -> Path:
    if container not in {"mkv", "mp4"}:
        raise ValueError("Contenitore non supportato")
    directory = Path(destination_dir)
    suffix = f".h265.{container}"
    candidate = directory / f"{Path(source).stem}{suffix}"
    counter = 1
    while candidate.exists():
        candidate = directory / f"{Path(source).stem} ({counter}){suffix}"
        counter += 1
    return candidate
