"""Collect a relocatable FFmpeg toolchain for the Rust build.

The packaged layout is always ``bin/ffmpeg``, ``bin/ffprobe`` and ``lib/`` for
every non-system dylib. ``install_name_tool`` rewrites every dependency so the
copy works after moving the application, and every Mach-O file is ad-hoc
signed again because rewriting invalidates the linker signature.

Run it after building FFmpeg with ``deployment/ffmpeg/build_minimal.sh``:

    python3 deployment/ffmpeg/collect_macos.py <ffmpeg-prefix> <vendor-dir>
"""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

SYSTEM_PREFIXES = ("/usr/lib/", "/System/", "/System/Library/")

MACOS_FRAMEWORKS = ("/System/Library/Frameworks/",)


def run(*args: str) -> str:
    result = subprocess.run(args, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise RuntimeError(f"{' '.join(args)}: {result.stderr.strip()}")
    return result.stdout


def dependencies(binary: Path) -> list[str]:
    output = run("otool", "-L", str(binary))
    lines = [line.strip() for line in output.splitlines()[1:] if line.strip()]
    return [line.split(" (compatibility")[0].strip() for line in lines]


def is_system(path: str) -> bool:
    return path.startswith(SYSTEM_PREFIXES) or path.startswith(MACOS_FRAMEWORKS)


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    prefix = Path(sys.argv[1]).resolve()
    destination = Path(sys.argv[2]).resolve()
    binaries = [prefix / "bin" / "ffmpeg", prefix / "bin" / "ffprobe"]
    for binary in binaries:
        if not binary.is_file():
            raise SystemExit(f"Missing FFmpeg binary: {binary}")

    if destination.exists():
        shutil.rmtree(destination)
    (destination / "bin").mkdir(parents=True)
    (destination / "lib").mkdir(parents=True)

    # Collect every non-system dependency, recursively, flattening names.
    collected: dict[str, Path] = {}
    pending = list(binaries)
    while pending:
        current = pending.pop()
        for dependency in dependencies(current):
            if is_system(dependency):
                continue
            path = Path(dependency)
            if not path.is_file():
                raise SystemExit(f"Missing dependency {dependency} of {current}")
            name = path.name
            if name not in collected:
                collected[name] = path
                pending.append(path)

    for binary in binaries:
        shutil.copy2(binary, destination / "bin" / binary.name)
    for name, path in collected.items():
        shutil.copy2(path, destination / "lib" / name)

    # Every binary points at ../lib, every library at its own directory.
    for binary in (destination / "bin" / "ffmpeg", destination / "bin" / "ffprobe"):
        for dependency in dependencies(binary):
            if is_system(dependency):
                continue
            target = f"@loader_path/../lib/{Path(dependency).name}"
            run("install_name_tool", "-change", dependency, target, str(binary))
    for library in (destination / "lib").iterdir():
        run("install_name_tool", "-id", f"@loader_path/{library.name}", str(library))
        for dependency in dependencies(library):
            if is_system(dependency):
                continue
            target = f"@loader_path/{Path(dependency).name}"
            run("install_name_tool", "-change", dependency, target, str(library))

    for binary in (destination / "bin" / "ffmpeg", destination / "bin" / "ffprobe"):
        run("codesign", "--force", "--sign", "-", "--timestamp=none", str(binary))
    for library in (destination / "lib").iterdir():
        run("codesign", "--force", "--sign", "-", "--timestamp=none", str(library))

    # Verify with the same relocation rules the packaged app uses.
    for binary in (destination / "bin" / "ffmpeg", destination / "bin" / "ffprobe"):
        for dependency in dependencies(binary):
            if is_system(dependency):
                continue
            if not dependency.startswith("@loader_path/"):
                raise SystemExit(f"Unrelocated dependency {dependency} in {binary}")
        output = run(str(binary), "-hide_banner", "-version")
        if "ffmpeg version" not in output and "ffprobe version" not in output:
            raise SystemExit(f"{binary} did not start")

    files = []
    for path in sorted(destination.rglob("*")):
        if path.is_file():
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            files.append(
                {
                    "path": str(path.relative_to(destination)),
                    "bytes": path.stat().st_size,
                    "sha256": digest,
                }
            )
    manifest = {
        "system": "Darwin",
        "machine": "arm64",
        "ffmpeg": "9.0.2",
        "files": files,
    }
    (destination / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    total = sum(item["bytes"] for item in files)
    print(f"Collected {len(files)} files, {total / 1_000_000:.1f} MB in {destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
