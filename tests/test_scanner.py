import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import unittest
from unittest.mock import MagicMock, patch

from fotogramma.core.scanner import (
    ProbeCancelled,
    ScanActiveError,
    ScanManager,
    VideoInfo,
    discover_videos,
    probe_video,
    probe_videos,
    summarize,
)


def fake_process(returncode=0, stdout="", stderr=""):
    process = MagicMock()
    process.returncode = returncode
    process.communicate.return_value = (stdout, stderr)
    return process


class ProbeVideoTests(unittest.TestCase):
    def setUp(self):
        self.tempdir = tempfile.TemporaryDirectory()
        self.root = self.tempdir.name
        self.video = Path(self.root) / "clip.mp4"
        self.video.write_bytes(b"fake video data")

    def tearDown(self):
        self.tempdir.cleanup()

    def probe_with_metadata(self, metadata):
        process = fake_process(stdout=json.dumps(metadata))
        with patch("fotogramma.core.scanner.subprocess.Popen", return_value=process) as popen:
            info = probe_video(str(self.video), self.root, "/fake/ffprobe")
        return info, popen

    def test_h264_is_marked_for_conversion_with_full_metadata(self):
        info, run = self.probe_with_metadata(
            {
                "streams": [{"codec_name": "h264", "width": 1920, "height": 1080}],
                "format": {"duration": "125.4", "size": "2048"},
            }
        )
        self.assertEqual(info.category, "convert")
        self.assertEqual(info.codec, "h264")
        self.assertEqual(info.duration, 125.4)
        self.assertEqual((info.width, info.height), (1920, 1080))
        self.assertEqual(info.size, 2048)
        self.assertEqual(info.relative_path, "clip.mp4")
        command = run.call_args.args[0]
        self.assertIn("v:0", command)
        self.assertIn("-show_entries", command)

    def test_hevc_and_av1_are_already_compatible(self):
        for codec in ("hevc", "av1"):
            with self.subTest(codec=codec):
                info, _run = self.probe_with_metadata(
                    {"streams": [{"codec_name": codec}], "format": {}}
                )
                self.assertEqual(info.category, "compatible")

    def test_other_codecs_are_marked_for_conversion(self):
        info, _run = self.probe_with_metadata(
            {"streams": [{"codec_name": "vp9"}], "format": {}}
        )
        self.assertEqual(info.category, "convert")

    def test_ffprobe_failure_is_visible_as_an_error(self):
        process = fake_process(
            returncode=1, stderr="Invalid data found\nMoov atom missing\n"
        )
        with patch("fotogramma.core.scanner.subprocess.Popen", return_value=process):
            info = probe_video(str(self.video), self.root, "/fake/ffprobe")
        self.assertEqual(info.category, "error")
        self.assertEqual(info.error, "Moov atom missing")
        self.assertGreater(info.size, 0)

    def test_missing_video_stream_is_an_error(self):
        info, _run = self.probe_with_metadata({"streams": [], "format": {}})
        self.assertEqual(info.category, "error")
        self.assertIn("Nessun flusso video", info.error)

    def test_cancellation_kills_the_active_ffprobe_process(self):
        cancel_event = threading.Event()
        process = fake_process()

        def communicate(timeout=None):
            if timeout is not None:
                cancel_event.set()
                raise subprocess.TimeoutExpired("ffprobe", timeout)
            return "", ""

        process.communicate.side_effect = communicate
        with patch("fotogramma.core.scanner.subprocess.Popen", return_value=process):
            with self.assertRaises(ProbeCancelled):
                probe_video(
                    str(self.video), self.root, "/fake/ffprobe", cancel_event
                )
        process.kill.assert_called_once_with()


class ProbeBatchCancellationTests(unittest.TestCase):
    def test_cancellation_does_not_start_a_second_worker_batch(self):
        candidates = [f"/library/video-{index}.mp4" for index in range(12)]
        cancel_event = threading.Event()
        calls = []

        def cancelled_probe(path, _root, _binary, event):
            calls.append(path)
            event.set()
            raise ProbeCancelled()

        with patch("fotogramma.core.scanner.probe_video", side_effect=cancelled_probe):
            started = time.monotonic()
            results, cancelled = probe_videos(
                candidates, "/library", "/fake/ffprobe", cancel_event
            )

        expected_workers = min(4, max(1, os.cpu_count() or 2))
        self.assertTrue(cancelled)
        self.assertEqual(results, [])
        self.assertEqual(len(calls), expected_workers)
        self.assertLess(time.monotonic() - started, 1)


class DiscoveryTests(unittest.TestCase):
    def test_discovers_supported_extensions_recursively(self):
        with tempfile.TemporaryDirectory() as root:
            Path(root, "movie.MP4").touch()
            Path(root, "notes.txt").touch()
            nested = Path(root, "nested")
            nested.mkdir()
            Path(nested, "clip.mkv").touch()
            Path(nested, "browser.webm").touch()
            Path(nested, "audio.mp3").touch()

            found = discover_videos(root)
            relative = sorted(os.path.relpath(path, root) for path in found)
            self.assertEqual(
                relative,
                ["movie.MP4", os.path.join("nested", "browser.webm"), os.path.join("nested", "clip.mkv")],
            )

    @patch("fotogramma.core.scanner.os.walk")
    def test_root_directory_failure_is_not_silently_ignored(self, walk):
        def fail_root(directory, onerror, followlinks):
            onerror(PermissionError(13, "Permission denied", directory))
            return []

        walk.side_effect = fail_root
        with self.assertRaises(PermissionError):
            discover_videos("/blocked")

    @patch("fotogramma.core.scanner.os.walk")
    def test_nested_directory_failure_is_reported_and_scan_continues(self, walk):
        nested = os.path.join("/library", "private")

        def skip_nested(directory, onerror, followlinks):
            yield directory, ["private"], ["movie.mp4"]
            onerror(PermissionError(13, "Permission denied", nested))

        walk.side_effect = skip_nested
        warnings = []
        found = discover_videos(
            "/library", on_error=lambda path, message: warnings.append((path, message))
        )
        self.assertEqual(found, [os.path.join("/library", "movie.mp4")])
        self.assertEqual(os.path.normcase(warnings[0][0]), os.path.normcase(os.path.abspath(nested)))
        self.assertEqual(warnings[0][1], "Permission denied")


class SummaryTests(unittest.TestCase):
    def test_summary_counts_categories_and_bytes(self):
        videos = [
            VideoInfo("/a", "a", "a", "", "mp4", "convert", size=100),
            VideoInfo("/b", "b", "b", "", "mkv", "compatible", size=200),
            VideoInfo("/c", "c", "c", "", "webm", "error", size=50),
        ]
        self.assertEqual(
            summarize(videos),
            {
                "total": 3,
                "to_convert": 1,
                "compatible": 1,
                "errors": 1,
                "total_bytes": 350,
                "to_convert_bytes": 100,
            },
        )


class ScanManagerTests(unittest.TestCase):
    @patch("fotogramma.core.scanner.get_ffprobe_path", return_value="/fake/ffprobe")
    @patch("fotogramma.core.scanner.probe_videos")
    @patch("fotogramma.core.scanner.discover_videos")
    def test_background_scan_can_be_read_after_completion(self, discover, probe, _ffprobe):
        with tempfile.TemporaryDirectory() as base:
            root = os.path.join(base, "libreria ")
            os.mkdir(root)
            candidate = os.path.join(root, "movie.mp4")
            info = VideoInfo(candidate, "movie.mp4", "movie.mp4", "", "mp4", "convert", size=100)
            discover.return_value = [candidate]
            probe.return_value = ([info], False)
            manager = ScanManager()
            scan_id = manager.start(root)["scan"]["id"]

            deadline = time.time() + 2
            snapshot = manager.get(scan_id)["scan"]
            while snapshot["status"] in ScanManager.ACTIVE_STATES and time.time() < deadline:
                time.sleep(0.01)
                snapshot = manager.get(scan_id)["scan"]

            self.assertEqual(snapshot["status"], "complete")
            self.assertEqual(os.path.normcase(snapshot["directory"]), os.path.normcase(os.path.abspath(root)))
            self.assertEqual(snapshot["summary"]["to_convert"], 1)
            self.assertEqual(snapshot["videos"][0]["relative_path"], "movie.mp4")

    @patch("fotogramma.core.scanner.get_ffprobe_path", return_value="/fake/ffprobe")
    @patch("fotogramma.core.scanner.probe_videos", return_value=([], False))
    @patch("fotogramma.core.scanner.discover_videos")
    def test_discovery_warning_marks_results_as_partial(
        self, discover, _probe, _ffprobe
    ):
        def report_warning(_root, _cancel, _progress, on_error):
            on_error(os.path.join(_root, "private"), "Permission denied")
            return []

        discover.side_effect = report_warning
        with tempfile.TemporaryDirectory() as root:
            manager = ScanManager()
            scan_id = manager.start(root)["scan"]["id"]
            deadline = time.time() + 2
            snapshot = manager.get(scan_id)["scan"]
            while snapshot["status"] in ScanManager.ACTIVE_STATES and time.time() < deadline:
                time.sleep(0.01)
                snapshot = manager.get(scan_id)["scan"]

        self.assertEqual(snapshot["status"], "complete")
        self.assertTrue(snapshot["partial"])
        self.assertEqual(snapshot["warnings"][0]["message"], "Permission denied")

    @patch("fotogramma.core.scanner.get_ffprobe_path", return_value="/fake/ffprobe")
    @patch("fotogramma.core.scanner.probe_videos")
    @patch("fotogramma.core.scanner.discover_videos")
    def test_active_scan_can_be_cancelled_and_cannot_be_started_twice(
        self, discover, probe, _ffprobe
    ):
        with tempfile.TemporaryDirectory() as root:
            candidate = os.path.join(root, "movie.mp4")
            discover.return_value = [candidate]
            probe_started = threading.Event()

            def blocking_probe(_candidates, _root, _binary, cancel_event, _progress):
                probe_started.set()
                cancel_event.wait(1)
                return [], True

            probe.side_effect = blocking_probe
            manager = ScanManager()
            scan_id = manager.start(root)["scan"]["id"]
            self.assertTrue(probe_started.wait(1))
            with self.assertRaises(ScanActiveError):
                manager.start(root)

            self.assertEqual(manager.cancel(scan_id)["scan"]["status"], "cancelling")
            deadline = time.time() + 2
            snapshot = manager.get(scan_id)["scan"]
            while snapshot["status"] in ScanManager.ACTIVE_STATES and time.time() < deadline:
                time.sleep(0.01)
                snapshot = manager.get(scan_id)["scan"]
            self.assertEqual(snapshot["status"], "cancelled")

    def test_rejects_an_empty_directory(self):
        with self.assertRaises(ValueError):
            ScanManager().start("   ")


if __name__ == "__main__":
    unittest.main()
