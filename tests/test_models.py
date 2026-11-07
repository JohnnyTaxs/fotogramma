import os
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication

from fotogramma.core.scanner import VideoInfo
from fotogramma.ui.models import CATEGORY_ROLE, INFO_ROLE, VideoFilterProxy, VideoTableModel


def video(name, category="convert", codec="h264", size=100, duration=10.0):
    return VideoInfo(
        path=f"/library/{name}",
        relative_path=name,
        name=name,
        folder="",
        extension=name.rsplit(".", 1)[-1],
        category=category,
        codec=codec,
        size=size,
        duration=duration,
        width=1920,
        height=1080,
    )


class VideoModelTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.model = VideoTableModel()
        self.proxy = VideoFilterProxy()
        self.proxy.setSourceModel(self.model)
        self.model.set_videos([
            video("zeta.mp4", size=100, duration=4),
            video("alpha.mkv", category="compatible", codec="hevc", size=400, duration=12),
            video("broken.webm", category="error", codec=None, size=50, duration=None),
        ])

    def test_model_exposes_native_columns_and_metadata(self):
        self.assertEqual(self.model.rowCount(), 3)
        self.assertEqual(self.model.columnCount(), 6)
        index = self.model.index(0, 0)
        self.assertIn("zeta.mp4", self.model.data(index, Qt.DisplayRole))
        self.assertFalse(self.model.data(index, Qt.DecorationRole).isNull())
        self.assertEqual(self.model.data(index, INFO_ROLE).codec, "h264")
        self.assertEqual(self.model.data(index, CATEGORY_ROLE), "convert")
        self.assertIn("/library/zeta.mp4", self.model.data(index, Qt.ToolTipRole))

    def test_category_filter(self):
        self.proxy.set_category("convert")
        self.assertEqual(self.proxy.rowCount(), 1)
        self.proxy.set_category("compatible")
        self.assertEqual(self.proxy.rowCount(), 1)
        self.proxy.set_category("error")
        self.assertEqual(self.proxy.rowCount(), 1)
        self.proxy.set_category("all")
        self.assertEqual(self.proxy.rowCount(), 3)

    def test_search_covers_name_path_and_codec(self):
        self.proxy.set_query("hevc")
        self.assertEqual(self.proxy.rowCount(), 1)
        self.proxy.set_query("BROKEN")
        self.assertEqual(self.proxy.rowCount(), 1)
        self.proxy.set_query("nothing-matches")
        self.assertEqual(self.proxy.rowCount(), 0)

    def test_sort_by_name_size_and_duration(self):
        self.proxy.sort(0, Qt.AscendingOrder)
        first = self.proxy.mapToSource(self.proxy.index(0, 0)).row()
        self.assertEqual(self.model.video(first).name, "alpha.mkv")
        self.proxy.sort(4, Qt.DescendingOrder)
        first = self.proxy.mapToSource(self.proxy.index(0, 4)).row()
        self.assertEqual(self.model.video(first).size, 400)
        self.proxy.sort(2, Qt.DescendingOrder)
        first = self.proxy.mapToSource(self.proxy.index(0, 2)).row()
        self.assertEqual(self.model.video(first).duration, 12)


if __name__ == "__main__":
    unittest.main()
