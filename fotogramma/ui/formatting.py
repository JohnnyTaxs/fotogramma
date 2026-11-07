"""Italian display formatting shared by native views."""

from __future__ import annotations


def format_int(value: int) -> str:
    return f"{value:,}".replace(",", "X").replace(".", ",").replace("X", ".")


def format_bytes(value: int | float | None) -> str:
    if value is None:
        return "—"
    amount = max(0.0, float(value))
    units = ["B", "KB", "MB", "GB", "TB", "PB"]
    index = 0
    while amount >= 1024 and index < len(units) - 1:
        amount /= 1024
        index += 1
    if index == 0:
        return f"{int(amount)} {units[index]}"
    return f"{amount:.1f}".replace(".", ",") + f" {units[index]}"


def format_duration(value: float | None) -> str:
    if value is None:
        return "—"
    total = max(0, round(value))
    hours, remainder = divmod(total, 3600)
    minutes, seconds = divmod(remainder, 60)
    if hours:
        return f"{hours}:{minutes:02d}:{seconds:02d}"
    return f"{minutes}:{seconds:02d}"


def codec_label(codec: str | None) -> str:
    if not codec:
        return "Sconosciuto"
    labels = {"hevc": "H.265", "h264": "H.264", "av1": "AV1", "vp9": "VP9", "mpeg4": "MPEG-4"}
    return labels.get(codec, codec.upper())
