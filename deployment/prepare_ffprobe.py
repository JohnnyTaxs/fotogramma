"""Vendor ffmpeg/ffprobe and every non-system native dependency."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

from ffprobe_download import download_media_tools

ROOT = Path(__file__).resolve().parents[1]
SYSTEM = platform.system()
MACHINE = platform.machine().lower()
VENDOR_DIR = ROOT / "deployment" / "vendor" / f"{SYSTEM}-{MACHINE}"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def homebrew_dependencies(binary: Path) -> list[Path]:
    result = subprocess.run(["otool", "-L", str(binary)], capture_output=True, text=True, check=True)
    dependencies = []
    for line in result.stdout.splitlines()[1:]:
        install_name = line.strip().split(" ", 1)[0]
        if install_name.startswith(("/opt/homebrew/", "/usr/local/")):
            dependencies.append(Path(install_name))
        elif install_name.startswith(("@rpath/", "@loader_path/")):
            filename = Path(install_name).name
            candidates = [binary.parent / filename, Path("/opt/homebrew/lib") / filename, Path("/usr/local/lib") / filename]
            dependency = next((path for path in candidates if path.is_file()), None)
            if dependency:
                dependencies.append(dependency)
    return dependencies


def prepare_macos() -> tuple[dict[str, Path], int]:
    sources = {}
    for name in ("ffmpeg", "ffprobe"):
        source = shutil.which(name)
        if not source:
            raise RuntimeError(f"Per compilare su macOS serve {name}: installa FFmpeg con brew.")
        sources[name] = Path(source).resolve()

    targets = {}
    for name, source in sources.items():
        target = VENDOR_DIR / name
        shutil.copy2(source, target)
        target.chmod(0o755)
        targets[name] = target

    queue = list(targets.values())
    processed = set()
    library_dir = VENDOR_DIR / "lib"
    library_dir.mkdir(parents=True, exist_ok=True)
    while queue:
        binary = queue.pop()
        if binary in processed:
            continue
        processed.add(binary)
        for dependency in homebrew_dependencies(binary):
            destination = library_dir / dependency.name
            if not destination.exists():
                shutil.copy2(dependency, destination)
                destination.chmod(0o755)
                queue.append(destination)

    environment = os.environ.copy()
    environment["DYLD_LIBRARY_PATH"] = str(library_dir)
    environment["DYLD_PRINT_LIBRARIES"] = "1"
    for target in targets.values():
        result = subprocess.run([str(target), "-version"], capture_output=True, text=True, env=environment, check=False)
        if result.returncode != 0:
            raise RuntimeError(f"{target.name} portabile non funziona: {result.stderr}")
        external = [line for line in result.stderr.splitlines() if "/opt/homebrew/" in line or "/usr/local/" in line]
        if external:
            raise RuntimeError("Dipendenze esterne ancora presenti:\n" + "\n".join(external))
    return targets, len(processed) - len(targets)


def write_manifest(binaries: dict[str, Path]) -> None:
    files = []
    for path in sorted(VENDOR_DIR.rglob("*")):
        if path.is_file() and path.name != "manifest.json":
            files.append({"path": str(path.relative_to(VENDOR_DIR)), "bytes": path.stat().st_size, "sha256": sha256(path)})
    manifest = {
        "system": SYSTEM,
        "machine": MACHINE,
        "binaries": sorted(binaries),
        "files": files,
    }
    (VENDOR_DIR / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    if VENDOR_DIR.exists():
        shutil.rmtree(VENDOR_DIR)
    VENDOR_DIR.mkdir(parents=True, exist_ok=True)
    if SYSTEM == "Darwin":
        binaries, count = prepare_macos()
        print(f"Raccolte {count} librerie native per FFmpeg.")
    elif SYSTEM in {"Windows", "Linux"}:
        binaries = download_media_tools(str(VENDOR_DIR))
        for binary in binaries.values():
            result = subprocess.run([str(binary), "-version"], capture_output=True, text=True, check=False)
            if result.returncode != 0:
                raise RuntimeError(result.stderr)
    else:
        raise RuntimeError(f"Build non supportata su {SYSTEM}")
    write_manifest(binaries)
    print(f"Strumenti FFmpeg pronti in {VENDOR_DIR}")


if __name__ == "__main__":
    main()
