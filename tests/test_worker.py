import os
import unittest

from fotogramma.core.scanner import get_ffprobe_path
from fotogramma.ui.workers import ScanWorker


class ScanWorkerTests(unittest.TestCase):
    def test_real_worker_scan_when_fixture_is_available(self):
        directory = os.environ.get("FOTOGRAMMA_DEMO_DIR")
        if not directory or not os.path.isdir(directory):
            self.skipTest("Imposta FOTOGRAMMA_DEMO_DIR con i 5 video dimostrativi")
        try:
            get_ffprobe_path()
        except FileNotFoundError:
            self.skipTest("ffprobe non disponibile")

        worker = ScanWorker(directory)
        completed = []
        failed = []
        worker.scanCompleted.connect(lambda videos, warnings, cancelled: completed.append((videos, warnings, cancelled)))
        worker.failed.connect(failed.append)
        worker.run()

        self.assertEqual(failed, [])
        videos, _warnings, cancelled = completed[0]
        self.assertFalse(cancelled)
        self.assertEqual(len(videos), 5)
        self.assertEqual(sum(video.category == "convert" for video in videos), 2)
        self.assertEqual(sum(video.category == "compatible" for video in videos), 2)
        self.assertEqual(sum(video.category == "error" for video in videos), 1)


if __name__ == "__main__":
    unittest.main()
