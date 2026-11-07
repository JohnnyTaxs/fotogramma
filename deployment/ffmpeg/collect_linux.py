"""Collect a relocatable FFmpeg toolchain for the Rust Linux build.

The packaged layout is ``bin/ffmpeg``, ``bin/ffprobe`` and ``lib/``. FFmpeg is
built with an ``$ORIGIN/../lib`` runpath, so copying the shared objects is
enough; the script verifies the runpath and that both binaries start.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

# glibc and the dynamic loader are provided by the target distribution.
SYSTEM_NAMES = (
    "libc.so",
    "libm.so",
    "libdl.so",
    "libpthread.so",
    "librt.so",
    "libresolv.so",
    "libutil.so",
    "libnsl.so",
    "libanl.so",
    "ld-linux",
)
SONAME = re.compile(r"\.so\.\d+$")


def run(*args: str) -> str:
    result = subprocess.run(args, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise RuntimeError(f"{' '.join(args)}: {result.stderr.strip()}")
    return result.stdout


def is_system(name: str) -> bool:
    return name.startswith(SYSTEM_NAMES) or name.startswith("ld-linux")


def dependencies(binary: Path) -> list[Path]:
    found = []
    for line in run("ldd", str(binary)).splitlines():
        line = line.strip()
        if "=>" not in line:
            continue
        candidate = line.split("=>", 1)[1].strip().split(" (", 1)[0].strip()
        if candidate and candidate != "not found":
            found.append(Path(candidate))
    return found


def collect_shared_objects(source: Path, destination: Path) -> dict[str, Path]:
    """Copy FFmpeg's own sonames, then every non-glibc dependency."""
    collected: dict[str, Path] = {}
    for item in source.iterdir():
        if not item.name.startswith("lib") or not SONAME.search(item.name):
            continue
        if item.is_symlink():
            resolved = item.resolve()
            if not resolved.is_file():
                continue
        else:
            resolved = item
        # Keep the soname (libavcodec.so.63), not the full version, so the
        # loader finds exactly the name recorded in DT_NEEDED.
        collected[item.name] = resolved

    pending = [destination / "bin" / "ffmpeg", destination / "bin" / "ffprobe"]
    pending.extend(collected.values())
    while pending:
        current = pending.pop()
        for dependency in dependencies(current):
            name = dependency.name
            if is_system(name) or name in collected:
                continue
            collected[name] = dependency
            pending.append(dependency)
    return collected


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
    for binary in binaries:
        shutil.copy2(binary, destination / "bin" / binary.name)

    for name, path in collect_shared_objects(prefix / "lib", destination).items():
        shutil.copy2(path, destination / "lib" / name)

    # Rewrite the runpath on every collected file so no build prefix leaks
    # into the portable archive. ``$ORIGIN/../lib`` resolves to lib/ for a
    # binary in bin/ and to the same directory for a library in lib/.
    relocatable = [destination / "bin" / "ffmpeg", destination / "bin" / "ffprobe"]
    relocatable.extend(destination / "lib" / name for name in collected)
    for path in relocatable:
        subprocess.run(
            ["patchelf", "--set-rpath", "$ORIGIN/../lib", str(path)],
            check=True,
        )

    for path in relocatable:
        dynamic = run("readelf", "-d", str(path))
        if "$ORIGIN/../lib" not in dynamic:
            raise SystemExit(f"Missing $ORIGIN/../lib runpath in {path}")

    for binary in (destination / "bin" / "ffmpeg", destination / "bin" / "ffprobe"):
        output = run(str(binary), "-hide_banner", "-version")
        if "version 9." not in output:
            raise SystemExit(f"{binary} did not start")

    files = []
    for path in sorted(destination.rglob("*")):
        if path.is_file():
            files.append(
                {
                    "path": str(path.relative_to(destination)),
                    "bytes": path.stat().st_size,
                    "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                }
            )
    manifest = {
        "system": "Linux",
        "machine": os.uname().machine,
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
