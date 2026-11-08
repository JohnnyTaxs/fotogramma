import os
from pathlib import Path
import shutil
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from fotogramma.core.conversion import PROFILES, allocate_output_path, allocate_partial_path, build_ffmpeg_args, custom_profile
from fotogramma.core.verification import replace_verified_output, validate_conversion_output
from fotogramma.core.media_tools import get_ffmpeg_path
from fotogramma.core.scanner import get_ffprobe_path, probe_video
from fotogramma.ui.workers import ConversionWorker


class ConversionProfileTests(unittest.TestCase):
    def test_profile_preserves_audio_subtitles_metadata_and_original(self):
        args = build_ffmpeg_args(PROFILES["max_quality"], "/video/original.mkv", "/video/output.mkv")
        self.assertEqual(args[0], "-n")
        self.assertIn("libx265", args)
        self.assertIn("veryslow", args)
        self.assertIn("aq-mode=3", args)
        self.assertEqual(args[args.index("-map") + 1], "0")
        self.assertEqual(args[args.index("-c:a") + 1], "copy")
        self.assertEqual(args[args.index("-c:s") + 1], "copy")
        self.assertEqual(args[args.index("-map_metadata") + 1], "0")
        self.assertEqual(args[args.index("-map_chapters") + 1], "0")

    def test_mp4_uses_faststart_and_aac_when_selected(self):
        profile = custom_profile(22, "medium", "yuv420p", None, None, "aac", 192)
        args = build_ffmpeg_args(profile, "/video/original.mkv", "/video/output.mp4")
        self.assertIn("+faststart", args)
        self.assertEqual(args[args.index("-c:a") + 1], "aac")
        self.assertIn("192k", args)
        self.assertIn("yuv420p", args)

    def test_output_allocation_never_overwrites(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "movie.mkv"
            source.touch()
            first = allocate_output_path(str(source), directory)
            self.assertEqual(first.name, "movie.h265.mkv")
            first.touch()
            second = allocate_output_path(str(source), directory)
            self.assertEqual(second.name, "movie (1).h265.mkv")


class RealConversionTests(unittest.TestCase):
    def test_short_video_is_converted_to_hevc_without_touching_original(self):
        directory = os.environ.get("FOTOGRAMMA_DEMO_DIR")
        if not directory or not os.path.isdir(directory):
            self.skipTest("Imposta FOTOGRAMMA_DEMO_DIR con i video dimostrativi")
        source = Path(directory) / "Vacanze" / "Gita al lago.mp4"
        if not source.is_file():
            self.skipTest("Fixture H.264 non disponibile")
        try:
            ffmpeg = get_ffmpeg_path()
            ffprobe = get_ffprobe_path()
        except FileNotFoundError:
            self.skipTest("FFmpeg non disponibile")

        video = probe_video(str(source), directory, ffprobe)
        original_bytes = source.read_bytes()
        with tempfile.TemporaryDirectory() as output_dir:
            worker = ConversionWorker([video], PROFILES["fast"], output_dir)
            completed = []
            failed = []
            worker.conversionCompleted.connect(completed.append)
            worker.failed.connect(failed.append)
            worker.run()

            self.assertEqual(failed, [])
            self.assertEqual(len(completed), 1)
            results = completed[0]
            self.assertEqual(len(results), 1, results)
            self.assertTrue(results[0].success, results[0].message)
            output = Path(results[0].destination)
            self.assertTrue(output.is_file())
            self.assertGreater(output.stat().st_size, 0)
            metadata = probe_video(str(output), output_dir, ffprobe)
            self.assertEqual(metadata.codec, "hevc", metadata.error)
        self.assertEqual(source.read_bytes(), original_bytes)


class ReplacementTests(unittest.TestCase):
    def demo_video(self, directory: str):
        source = Path(directory) / "Vacanze" / "Gita al lago.mp4"
        if not source.is_file():
            self.skipTest("Imposta FOTOGRAMMA_DEMO_DIR con i video dimostrativi")
        return source

    def test_replacement_happens_only_after_full_validation(self):
        directory = os.environ.get("FOTOGRAMMA_DEMO_DIR")
        if not directory or not os.path.isdir(directory):
            self.skipTest("Imposta FOTOGRAMMA_DEMO_DIR con i video dimostrativi")
        source = self.demo_video(directory)
        ffprobe = get_ffprobe_path()
        original_bytes = source.read_bytes()
        with tempfile.TemporaryDirectory() as output_dir:
            working_copy = Path(output_dir) / source.name
            shutil.copy2(source, working_copy)
            worker = ConversionWorker([probe_video(str(working_copy), output_dir, ffprobe)], PROFILES["fast"], output_dir, replace_original=True)
            completed = []
            failed = []
            worker.conversionCompleted.connect(completed.append)
            worker.failed.connect(failed.append)
            worker.run()

            self.assertEqual(failed, [])
            result = completed[0][0]
            self.assertTrue(result.success, result.message)
            self.assertTrue(result.verified)
            self.assertTrue(result.replaced)
            metadata = probe_video(str(working_copy), output_dir, ffprobe)
            self.assertEqual(metadata.codec, "hevc")
            valid, _message = validate_conversion_output(probe_video(str(working_copy), output_dir, ffprobe), working_copy, ffprobe)
            self.assertTrue(valid)
            self.assertFalse(list(Path(output_dir).glob("*.fotogramma-backup")))
            self.assertNotEqual(working_copy.read_bytes(), original_bytes)
        self.assertEqual(source.read_bytes(), original_bytes)

    def test_failed_validation_preserves_original(self):
        directory = os.environ.get("FOTOGRAMMA_DEMO_DIR")
        if not directory or not os.path.isdir(directory):
            self.skipTest("Imposta FOTOGRAMMA_DEMO_DIR con i video dimostrativi")
        source = self.demo_video(directory)
        ffprobe = get_ffprobe_path()
        original_bytes = source.read_bytes()
        with tempfile.TemporaryDirectory() as output_dir:
            working_copy = Path(output_dir) / source.name
            shutil.copy2(source, working_copy)
            with patch("fotogramma.ui.workers.validate_conversion_output", return_value=(False, "Test fallito")):
                worker = ConversionWorker([probe_video(str(working_copy), output_dir, ffprobe)], PROFILES["fast"], output_dir, replace_original=True)
                completed = []
                worker.conversionCompleted.connect(completed.append)
                worker.run()

            result = completed[0][0]
            self.assertFalse(result.success)
            self.assertIn("Originale non modificato", result.message)
            self.assertEqual(working_copy.read_bytes(), original_bytes)
            self.assertFalse(list(Path(output_dir).glob("*.fotogramma-backup")))

    def test_replacement_and_backup_restore(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "movie.mkv"
            source.write_bytes(b"original")
            converted = Path(directory) / "movie.h265.partial.mkv"
            converted.write_bytes(b"converted")
            with patch("fotogramma.core.verification.probe_video", return_value=SimpleNamespace(category="compatible", error=None)):
                success, _message = replace_verified_output(str(source), converted, "/fake/ffprobe")
            self.assertTrue(success)
            self.assertEqual(source.read_bytes(), b"converted")
            self.assertFalse(Path(str(source) + ".fotogramma-backup").exists())

    def test_final_probe_failure_restores_original(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "movie.mkv"
            source.write_bytes(b"original")
            converted = Path(directory) / "movie.h265.partial.mkv"
            converted.write_bytes(b"converted")
            with patch("fotogramma.core.verification.probe_video", return_value=SimpleNamespace(category="error", error="bad")):
                success, _message = replace_verified_output(str(source), converted, "/fake/ffprobe")
            self.assertFalse(success)
            self.assertEqual(source.read_bytes(), b"original")
            self.assertFalse(converted.exists())
            self.assertFalse(Path(str(source) + ".fotogramma-backup").exists())

    def test_replacement_rejects_webm(self):
        with self.assertRaises(ValueError):
            allocate_partial_path("/video/movie.webm")


if __name__ == "__main__":
    unittest.main()
