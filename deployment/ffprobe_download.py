"""Download ffmpeg and ffprobe from community static archives."""

from __future__ import annotations

import os
from pathlib import Path
import platform
import shutil
import tarfile
import urllib.request
import zipfile


def download_media_tools(dest_dir: str) -> dict[str, Path]:
    system = platform.system()
    machine = platform.machine().lower()
    destination = Path(dest_dir)
    destination.mkdir(parents=True, exist_ok=True)

    if system == "Windows":
        url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
        archive = destination / "ffmpeg.zip"
        binary_names = ("ffmpeg.exe", "ffprobe.exe")
    elif system == "Linux":
        if "arm64" in machine or "aarch64" in machine:
            arch = "arm64"
        elif "x86_64" in machine or "amd64" in machine:
            arch = "amd64"
        else:
            arch = "i686"
        url = f"https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-{arch}-static.tar.xz"
        archive = destination / "ffmpeg.tar.xz"
        binary_names = ("ffmpeg", "ffprobe")
    else:
        raise RuntimeError("Automatic download is available only on Windows and Linux.")

    print(f"Downloading FFmpeg tools from {url} ...")
    outputs = {}
    try:
        urllib.request.urlretrieve(url, archive)
        for binary_name in binary_names:
            final_path = destination / binary_name
            if archive.suffix == ".zip":
                with zipfile.ZipFile(archive) as bundle:
                    member = next(
                        name for name in bundle.namelist()
                        if name == binary_name or name.endswith("/" + binary_name)
                    )
                    with bundle.open(member) as source, final_path.open("wb") as target:
                        shutil.copyfileobj(source, target)
            else:
                with tarfile.open(archive, "r:xz") as bundle:
                    member = next(
                        item for item in bundle.getmembers()
                        if item.isfile() and item.name.endswith("/" + binary_name)
                    )
                    source = bundle.extractfile(member)
                    if source is None:
                        raise RuntimeError(f"{binary_name} was not found in the archive")
                    with source, final_path.open("wb") as target:
                        shutil.copyfileobj(source, target)
            if final_path.stat().st_size < 1_000_000:
                raise RuntimeError(f"The {binary_name} binary is invalid")
            if system != "Windows":
                final_path.chmod(0o755)
            outputs[Path(binary_name).stem] = final_path
        return outputs
    finally:
        try:
            archive.unlink()
        except OSError:
            pass


def download_ffprobe(dest_dir: str) -> str:
    """Backward-compatible helper for callers that only need ffprobe."""
    return str(download_media_tools(dest_dir)["ffprobe"])
