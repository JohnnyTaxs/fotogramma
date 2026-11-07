"""Background Qt workers; no HTTP server or browser is involved."""

from __future__ import annotations

import os
import subprocess
import threading
from dataclasses import dataclass

from PySide6.QtCore import QThread, Signal

from fotogramma.core.conversion import allocate_output_path, allocate_partial_path, build_ffmpeg_args
from fotogramma.core.media_tools import get_ffmpeg_path
from fotogramma.core.scanner import discover_videos, ffprobe_environment, get_ffprobe_path, probe_videos
from fotogramma.core.verification import replace_verified_output, validate_conversion_output


class ScanWorker(QThread):
    progress = Signal(int, int, str)
    scanCompleted = Signal(object, object, bool)
    failed = Signal(str)

    def __init__(self, directory: str, parent=None) -> None:
        super().__init__(parent)
        self.directory = directory
        self.cancel_event = threading.Event()

    def run(self) -> None:
        try:
            ffprobe_bin = get_ffprobe_path()
            warnings = []

            def discovery_error(path: str, message: str) -> None:
                warnings.append({"path": path, "message": message})

            def discovered(count: int, current_file: str) -> None:
                self.progress.emit(count, -1, current_file)

            candidates = discover_videos(
                self.directory,
                self.cancel_event,
                discovered,
                discovery_error,
            )
            if self.cancel_event.is_set():
                self.scanCompleted.emit([], warnings, True)
                return

            def progressed(processed: int, total: int, current_file: str, _info) -> None:
                self.progress.emit(processed, total, current_file)

            videos, cancelled = probe_videos(
                candidates,
                self.directory,
                ffprobe_bin,
                self.cancel_event,
                progressed,
            )
            self.scanCompleted.emit(videos, warnings, cancelled)
        except Exception as exc:  # Present unexpected worker failures in the native UI.
            self.failed.emit(str(exc))

    def cancel(self) -> None:
        self.cancel_event.set()


@dataclass
class ConversionResult:
    source: str
    destination: str
    success: bool
    message: str = ""
    cancelled: bool = False
    verified: bool = False
    replaced: bool = False


class ConversionWorker(QThread):
    fileStarted = Signal(int, int, str, str)
    progress = Signal(int, int, int, str)
    verificationStarted = Signal(int, str)
    fileCompleted = Signal(object)
    conversionCompleted = Signal(object)
    failed = Signal(str)

    def __init__(self, videos, profile, destination_dir: str, container: str = "mkv", replace_original: bool = False, parent=None) -> None:
        super().__init__(parent)
        self.videos = list(videos)
        self.profile = profile
        self.destination_dir = destination_dir
        self.container = container
        self.replace_original = replace_original
        self.cancel_event = threading.Event()

    def run(self) -> None:
        try:
            ffmpeg = get_ffmpeg_path()
            os.makedirs(self.destination_dir, exist_ok=True)
            results = []
            for index, video in enumerate(self.videos, start=1):
                if self.cancel_event.is_set():
                    break
                if self.replace_original:
                    try:
                        destination = allocate_partial_path(video.path)
                    except ValueError as exc:
                        result = ConversionResult(video.path, video.path, False, str(exc))
                        self.fileCompleted.emit(result)
                        continue
                else:
                    destination = allocate_output_path(video.path, self.destination_dir, self.container)
                self.fileStarted.emit(index, len(self.videos), video.path, str(destination))
                result = self._convert_one(ffmpeg, video, destination, index, len(self.videos))
                results.append(result)
                self.fileCompleted.emit(result)
            self.conversionCompleted.emit(results)
        except Exception as exc:
            self.failed.emit(str(exc))

    def _convert_one(self, ffmpeg: str, video, destination, file_index: int, total: int) -> ConversionResult:
        args = build_ffmpeg_args(self.profile, video.path, str(destination))
        process = subprocess.Popen(
            [ffmpeg, *args],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            env=ffprobe_environment(),
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0) if os.name == "nt" else 0,
        )
        log_lines = []
        cancelled = False
        for raw_line in process.stdout:
            line = raw_line.strip()
            if self.cancel_event.is_set():
                process.kill()
                cancelled = True
            if line.startswith("out_time_ms="):
                if video.duration:
                    try:
                        elapsed = int(line.split("=", 1)[1]) / 1_000_000
                        percent = min(99, max(0, round(elapsed / video.duration * 100)))
                    except (ValueError, ZeroDivisionError):
                        percent = 0
                    self.progress.emit(file_index, total, percent, video.relative_path)
            elif line.startswith("progress=end"):
                self.progress.emit(file_index, total, 100, video.relative_path)
            elif line:
                log_lines.append(line)
                log_lines = log_lines[-8:]
        return_code = process.wait()
        process.stdout.close()
        if cancelled or self.cancel_event.is_set():
            return ConversionResult(video.path, str(destination), False, "Conversione interrotta", True)
        if return_code != 0:
            message = "\n".join(log_lines) or f"ffmpeg è terminato con codice {return_code}"
            return ConversionResult(video.path, str(destination), False, message)
        if not destination.is_file() or destination.stat().st_size == 0:
            return ConversionResult(video.path, str(destination), False, "File di output non creato")
        if self.replace_original:
            self.verificationStarted.emit(file_index, video.relative_path)
            ffprobe_bin = get_ffprobe_path()
            valid, validation_message = validate_conversion_output(video, destination, ffprobe_bin)
            if not valid:
                return ConversionResult(video.path, str(destination), False, f"Originale non modificato: {validation_message}")
            replaced, replacement_message = replace_verified_output(video.path, destination, ffprobe_bin)
            if not replaced:
                return ConversionResult(video.path, str(destination), False, replacement_message)
            return ConversionResult(video.path, video.path, True, replacement_message, False, True, True)
        return ConversionResult(video.path, str(destination), True)

    def cancel(self) -> None:
        self.cancel_event.set()
