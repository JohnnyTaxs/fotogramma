import os
from pathlib import Path
import tempfile
import unittest

from fotogramma.core.conversion import PROFILES, allocate_output_path, build_ffmpeg_args, custom_profile
from fotogramma.core.media_tools import get_ffmpeg_path
from fotogramma.core.scanner import get_ffprobe_path, probe_video
from fotogramma.core.search import h265_search_url, title_for_search
from fotogramma.ui.workers import ConversionWorker


class ConversionProfileTests(unittest.TestCase):
    def test_profile_preserves_audio_subtitles_metadata_and_original(self):
        args = build_ffmpeg_args(PROFILES["anime"], "/video/original.mkv", "/video/output.mkv")
        self.assertEqual(args[0], "-n")
        self.assertIn("libx265", args)
        self.assertIn("animation", args)
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


class SearchTests(unittest.TestCase):
    def test_search_url_cleans_release_tags(self):
        self.assertEqual(title_for_search("1917 - Bluray-2160p - [IT+EN].mkv"), "1917")
        self.assertEqual(
            h265_search_url("1917 - Bluray-2160p - [IT+EN].mkv"),
            "https://1337x.to/search/1917+H.265/1/",
        )

    def test_search_keeps_episode_information(self):
        title = title_for_search("Example Show - S01E02 - Title - WEBDL-1080p.mkv")
        self.assertIn("S01E02", title)
        self.assertIn("Example Show", title)


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
            worker = ConversionWorker([video], PROFILES["light"], output_dir)
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


if __name__ == "__main__":
    unittest.main()
