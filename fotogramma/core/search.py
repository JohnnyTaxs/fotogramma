"""Build an external 1337x search URL without doing network requests in the app."""

from __future__ import annotations

import re
from pathlib import Path
from urllib.parse import quote_plus

RELEASE_TAGS = re.compile(
    r"\b(?:bluray|webdl|webrip|web-dl|hdtv|bdrip|brrip|2160p|1080p|720p|576p|480p|x264|x265|h264|h265|hevc)\b",
    re.IGNORECASE,
)


def title_for_search(filename: str) -> str:
    stem = Path(filename).stem.replace(".", " ").replace("_", " ")
    stem = RELEASE_TAGS.split(stem, maxsplit=1)[0]
    stem = re.sub(r"\s+", " ", stem).strip(" -_")
    return stem or Path(filename).stem


def h265_search_url(filename: str) -> str:
    title = title_for_search(filename)
    return f"https://1337x.to/search/{quote_plus(f'{title} H.265')}/1/"
