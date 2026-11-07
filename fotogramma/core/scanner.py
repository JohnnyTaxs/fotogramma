"""Core video discovery and metadata scanning used by the CLI and web app."""

from __future__ import annotations

import json
import math
import os
import shutil
import subprocess
import sys
import threading
import time
import uuid
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import asdict, dataclass
from typing import Callable, Dict, List, Optional, Sequence, Tuple

from runtime import FROZEN, resource_path

VIDEO_EXTENSIONS = frozenset({".mp4", ".mkv", ".webm"})
COMPATIBLE_CODECS = frozenset({"hevc", "av1"})
PROBE_TIMEOUT_SECONDS = 45


class ProbeCancelled(RuntimeError):
    """Internal signal used to stop a probe without classifying it as a bad video."""


@dataclass
class VideoInfo:
    path: str
    relative_path: str
    name: str
    folder: str
    extension: str
    category: str
    codec: Optional[str] = None
    size: Optional[int] = None
    duration: Optional[float] = None
    width: Optional[int] = None
    height: Optional[int] = None
    error: Optional[str] = None

    def to_dict(self) -> Dict[str, object]:
        return asdict(self)


def get_ffprobe_path() -> str:
    """Prefer the bundled binary in frozen apps, then PATH and source-local files."""
    executable = "ffprobe.exe" if os.name == "nt" else "ffprobe"
    if FROZEN:
        bundled = resource_path(executable)
        if bundled.is_file():
            return str(bundled)

    path = shutil.which("ffprobe")
    if path:
        return path

    locations = (
        os.path.join(os.getcwd(), executable),
        str(resource_path(executable)),
    )
    for candidate in locations:
        if os.path.isfile(candidate):
            return candidate

    raise FileNotFoundError(
        "ffprobe non trovato. Installa FFmpeg oppure copia ffprobe nella cartella dell'app."
    )


def get_ffprobe_version(ffprobe_bin: str) -> Optional[str]:
    try:
        result = subprocess.run(
            [ffprobe_bin, "-version"],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=8,
            creationflags=_subprocess_flags(),
            check=False,
            env=ffprobe_environment(),
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if result.returncode != 0 or not result.stdout:
        return None
    return result.stdout.splitlines()[0].strip()


def _subprocess_flags() -> int:
    return getattr(subprocess, "CREATE_NO_WINDOW", 0) if os.name == "nt" else 0


def ffprobe_environment() -> Dict[str, str]:
    """Provide bundled native libraries to ffprobe in frozen macOS builds."""
    environment = os.environ.copy()
    if FROZEN and sys.platform == "darwin":
        library_dir = str(resource_path("lib"))
        current = environment.get("DYLD_LIBRARY_PATH", "")
        environment["DYLD_LIBRARY_PATH"] = library_dir + (
            os.pathsep + current if current else ""
        )
    return environment


def discover_videos(
    directory: str,
    cancel_event: Optional[threading.Event] = None,
    on_progress: Optional[Callable[[int, str], None]] = None,
    on_error: Optional[Callable[[str, str], None]] = None,
) -> List[str]:
    """Recursively discover videos, reporting skipped subdirectories explicitly."""
    candidates: List[str] = []
    root_directory = os.path.normcase(os.path.abspath(directory))

    def walk_error(error: OSError) -> None:
        filename = os.fsdecode(os.fspath(error.filename)) if error.filename else directory
        filename = os.path.abspath(filename)
        if on_error:
            on_error(filename, error.strerror or str(error))
        if os.path.normcase(filename) == root_directory:
            raise error

    for root, dirs, files in os.walk(
        directory, onerror=walk_error, followlinks=False
    ):
        if cancel_event and cancel_event.is_set():
            break
        dirs.sort(key=str.casefold)
        for filename in sorted(files, key=str.casefold):
            if cancel_event and cancel_event.is_set():
                break
            if os.path.splitext(filename)[1].lower() not in VIDEO_EXTENSIONS:
                continue
            path = os.path.join(root, filename)
            candidates.append(path)
            if on_progress:
                on_progress(len(candidates), os.path.relpath(path, directory))
    return candidates


def _number(value: object) -> Optional[float]:
    try:
        number = float(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return None
    return number if math.isfinite(number) and number >= 0 else None


def _integer(value: object) -> Optional[int]:
    number = _number(value)
    return int(number) if number is not None else None


def _error_video(
    file_path: str, root_directory: str, message: str, size: Optional[int] = None
) -> VideoInfo:
    relative_path = os.path.relpath(file_path, root_directory)
    folder = os.path.dirname(relative_path)
    return VideoInfo(
        path=os.path.abspath(file_path),
        relative_path=relative_path,
        name=os.path.basename(file_path),
        folder="" if folder == "." else folder,
        extension=os.path.splitext(file_path)[1].lower().lstrip("."),
        category="error",
        size=size,
        error=message,
    )


def probe_video(
    file_path: str,
    root_directory: str,
    ffprobe_bin: str,
    cancel_event: Optional[threading.Event] = None,
) -> VideoInfo:
    """Read the first video stream and stop ffprobe promptly on cancellation."""
    try:
        size: Optional[int] = os.path.getsize(file_path)
    except OSError as exc:
        return _error_video(file_path, root_directory, str(exc))

    command = [
        ffprobe_bin,
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=codec_name,width,height:format=duration,size",
        "-of",
        "json",
        file_path,
    ]
    if cancel_event and cancel_event.is_set():
        raise ProbeCancelled()

    try:
        process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            creationflags=_subprocess_flags(),
            env=ffprobe_environment(),
        )
        deadline = time.monotonic() + PROBE_TIMEOUT_SECONDS
        while True:
            if cancel_event and cancel_event.is_set():
                process.kill()
                process.communicate()
                raise ProbeCancelled()
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                process.kill()
                process.communicate()
                return _error_video(
                    file_path,
                    root_directory,
                    f"ffprobe non ha risposto entro {PROBE_TIMEOUT_SECONDS} secondi.",
                    size,
                )
            try:
                stdout, stderr = process.communicate(timeout=min(0.2, remaining))
                break
            except subprocess.TimeoutExpired:
                continue
    except ProbeCancelled:
        raise
    except OSError as exc:
        return _error_video(file_path, root_directory, str(exc), size)

    if process.returncode != 0:
        details = (stderr or stdout or "File non leggibile da ffprobe.").strip()
        details = details.splitlines()[-1][:400]
        return _error_video(file_path, root_directory, details, size)

    try:
        metadata = json.loads(stdout)
        streams = metadata.get("streams") or []
        if not streams:
            raise ValueError("Nessun flusso video trovato.")
        stream = streams[0]
        codec = str(stream.get("codec_name") or "").lower() or None
        if not codec:
            raise ValueError("Codec video non disponibile.")
    except (ValueError, TypeError, json.JSONDecodeError) as exc:
        return _error_video(file_path, root_directory, str(exc), size)

    relative_path = os.path.relpath(file_path, root_directory)
    folder = os.path.dirname(relative_path)
    format_metadata = metadata.get("format") or {}
    duration = _number(format_metadata.get("duration"))
    return VideoInfo(
        path=os.path.abspath(file_path),
        relative_path=relative_path,
        name=os.path.basename(file_path),
        folder="" if folder == "." else folder,
        extension=os.path.splitext(file_path)[1].lower().lstrip("."),
        category="compatible" if codec in COMPATIBLE_CODECS else "convert",
        codec=codec,
        size=_integer(format_metadata.get("size")) or size,
        duration=duration,
        width=_integer(stream.get("width")),
        height=_integer(stream.get("height")),
    )


def probe_videos(
    candidates: Sequence[str],
    root_directory: str,
    ffprobe_bin: str,
    cancel_event: Optional[threading.Event] = None,
    on_progress: Optional[Callable[[int, int, str, VideoInfo], None]] = None,
) -> Tuple[List[VideoInfo], bool]:
    """Probe one worker-sized batch at a time so cancellation has no queued wave."""
    results: List[VideoInfo] = []
    processed = 0
    cancelled = False
    worker_count = min(4, max(1, os.cpu_count() or 2))
    batch_size = worker_count

    with ThreadPoolExecutor(max_workers=worker_count) as executor:
        for offset in range(0, len(candidates), batch_size):
            if cancel_event and cancel_event.is_set():
                cancelled = True
                break
            batch = candidates[offset : offset + batch_size]
            futures = {
                executor.submit(probe_video, path, root_directory, ffprobe_bin, cancel_event): path
                for path in batch
            }
            for future in as_completed(futures):
                path = futures[future]
                try:
                    info = future.result()
                except ProbeCancelled:
                    cancelled = True
                    continue
                results.append(info)
                processed += 1
                if on_progress:
                    on_progress(
                        processed,
                        len(candidates),
                        os.path.relpath(path, root_directory),
                        info,
                    )
            if cancel_event and cancel_event.is_set() and processed < len(candidates):
                cancelled = True
                break

    results.sort(key=lambda item: item.relative_path.casefold())
    return results, cancelled


def summarize(videos: Sequence[VideoInfo], total: Optional[int] = None) -> Dict[str, int]:
    convertible = [video for video in videos if video.category == "convert"]
    return {
        "total": len(videos) if total is None else total,
        "to_convert": len(convertible),
        "compatible": sum(video.category == "compatible" for video in videos),
        "errors": sum(video.category == "error" for video in videos),
        "total_bytes": sum(video.size or 0 for video in videos),
        "to_convert_bytes": sum(video.size or 0 for video in convertible),
    }


class ScanActiveError(RuntimeError):
    pass


class ScanNotFoundError(RuntimeError):
    pass


class ScanManager:
    """Owns the latest scan and its background worker."""

    ACTIVE_STATES = frozenset({"discovering", "probing", "cancelling"})

    def __init__(self) -> None:
        self._lock = threading.RLock()
        self._scan: Optional[Dict[str, object]] = None
        self._cancel_event: Optional[threading.Event] = None

    def start(self, directory: str) -> Dict[str, object]:
        if not directory.strip():
            raise ValueError("Indica una cartella da analizzare.")
        directory = os.path.abspath(os.path.expanduser(directory))
        if not os.path.isdir(directory):
            raise ValueError("La cartella indicata non esiste o non è accessibile.")

        with self._lock:
            if self._scan and self._scan["status"] in self.ACTIVE_STATES:
                raise ScanActiveError("È già in corso un'analisi.")
            ffprobe_bin = get_ffprobe_path()
            scan_id = uuid.uuid4().hex
            self._cancel_event = threading.Event()
            self._scan = {
                "id": scan_id,
                "directory": directory,
                "status": "discovering",
                "total": 0,
                "processed": 0,
                "current_file": "",
                "videos": [],
                "summary": summarize([], 0),
                "error": None,
                "warnings": [],
                "partial": False,
                "started_at": time.time(),
                "finished_at": None,
            }
            thread = threading.Thread(
                target=self._run,
                args=(scan_id, directory, ffprobe_bin, self._cancel_event),
                name=f"video-scan-{scan_id[:8]}",
                daemon=True,
            )
            thread.start()
            return self._snapshot_locked()

    def current(self) -> Dict[str, object]:
        with self._lock:
            return self._snapshot_locked()

    def get(self, scan_id: str) -> Dict[str, object]:
        with self._lock:
            if not self._scan or self._scan["id"] != scan_id:
                raise ScanNotFoundError("Analisi non trovata.")
            return self._snapshot_locked()

    def cancel(self, scan_id: str) -> Dict[str, object]:
        with self._lock:
            if not self._scan or self._scan["id"] != scan_id:
                raise ScanNotFoundError("Analisi non trovata.")
            if self._scan["status"] in self.ACTIVE_STATES:
                self._scan["status"] = "cancelling"
                if self._cancel_event:
                    self._cancel_event.set()
            return self._snapshot_locked()

    def _run(
        self,
        scan_id: str,
        directory: str,
        ffprobe_bin: str,
        cancel_event: threading.Event,
    ) -> None:
        try:
            def discovered(found: int, current_file: str) -> None:
                with self._lock:
                    if self._is_current_locked(scan_id):
                        self._scan["total"] = found
                        self._scan["current_file"] = current_file

            discovery_warnings: List[Dict[str, str]] = []

            def discovery_error(path: str, message: str) -> None:
                warning = {"path": path, "message": message}
                discovery_warnings.append(warning)
                with self._lock:
                    if self._is_current_locked(scan_id):
                        self._scan["warnings"] = list(discovery_warnings)
                        self._scan["partial"] = True

            candidates = discover_videos(
                directory, cancel_event, discovered, discovery_error
            )
            with self._lock:
                if not self._is_current_locked(scan_id):
                    return
                if cancel_event.is_set():
                    self._scan["partial"] = True
                    self._finish_locked("cancelled")
                    return
                self._scan["status"] = "probing"
                self._scan["total"] = len(candidates)
                self._scan["current_file"] = ""

            def progressed(
                processed: int, total: int, current_file: str, _info: VideoInfo
            ) -> None:
                with self._lock:
                    if self._is_current_locked(scan_id):
                        self._scan["processed"] = processed
                        self._scan["total"] = total
                        self._scan["current_file"] = current_file

            videos, cancelled = probe_videos(
                candidates,
                directory,
                ffprobe_bin,
                cancel_event,
                progressed,
            )
            with self._lock:
                if not self._is_current_locked(scan_id):
                    return
                self._scan["videos"] = [video.to_dict() for video in videos]
                self._scan["summary"] = summarize(videos, len(candidates))
                self._scan["warnings"] = list(discovery_warnings)
                self._scan["partial"] = bool(discovery_warnings) or cancelled
                self._finish_locked("cancelled" if cancelled else "complete")
        except Exception as exc:  # Surface unexpected worker failures through the API.
            with self._lock:
                if self._is_current_locked(scan_id):
                    self._scan["error"] = str(exc)
                    self._finish_locked("error")

    def _is_current_locked(self, scan_id: str) -> bool:
        return bool(self._scan and self._scan["id"] == scan_id)

    def _finish_locked(self, status: str) -> None:
        if not self._scan:
            return
        self._scan["status"] = status
        self._scan["current_file"] = ""
        self._scan["finished_at"] = time.time()

    def _snapshot_locked(self) -> Dict[str, object]:
        if not self._scan:
            return {"scan": None}
        snapshot = dict(self._scan)
        snapshot["videos"] = list(self._scan["videos"])  # type: ignore[index]
        snapshot["summary"] = dict(self._scan["summary"])  # type: ignore[arg-type]
        snapshot["warnings"] = list(self._scan["warnings"])  # type: ignore[index]
        return {"scan": snapshot}
