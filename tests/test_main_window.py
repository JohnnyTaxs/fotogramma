import os
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QItemSelectionModel
from PySide6.QtWidgets import QApplication

from fotogramma.core.scanner import VideoInfo
from fotogramma.ui.conversion_dialog import ConversionDialog
from fotogramma.ui.main_window import MainWindow


def video(name, category, codec, size):
    return VideoInfo(
        path=f"/demo/{name}",
        relative_path=name,
        name=name,
        folder="",
        extension=name.rsplit(".", 1)[-1],
        category=category,
        codec=codec,
        size=size,
        duration=5,
        width=640,
        height=360,
    )


class MainWindowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.window = MainWindow()

    def tearDown(self):
        self.window.deleteLater()

    def test_results_update_stats_filters_and_composition(self):
        self.window._scan_completed(
            [
                video("one.mp4", "convert", "h264", 100),
                video("two.mp4", "convert", "h264", 200),
                video("three.mp4", "compatible", "hevc", 300),
                video("broken.mkv", "error", None, 50),
            ],
            [],
            False,
        )
        self.assertEqual(self.window.stack.currentIndex(), 1)
        self.assertEqual(self.window.summary["total"], 4)
        self.assertEqual(self.window.summary["to_convert"], 2)
        self.assertIn("4", self.window.stat_buttons["all"].text())
        self.assertIn("2", self.window.stat_buttons["convert"].text())
        self.assertEqual(self.window.proxy.rowCount(), 2)
        self.assertTrue(self.window.stat_buttons["convert"].isChecked())

        self.window.set_filter("compatible")
        self.assertEqual(self.window.proxy.rowCount(), 1)
        self.window.search_edit.setText("three")
        self.assertEqual(self.window.proxy.rowCount(), 1)
        self.window.search_edit.setText("missing")
        self.assertEqual(self.window.proxy.rowCount(), 0)

    def test_partial_warning_is_visible(self):
        self.window._scan_completed(
            [video("one.mp4", "convert", "h264", 100)],
            [{"path": "/demo/private", "message": "Permission denied"}],
            False,
        )
        self.assertFalse(self.window.partial_notice.isHidden())
        self.assertIn("incompleto", self.window.partial_notice.text())

    def test_selection_enables_online_search_and_conversion(self):
        self.window._scan_completed(
            [video("one.mp4", "convert", "h264", 100), video("two.mp4", "convert", "h264", 200)],
            [],
            False,
        )
        selection = self.window.table.selectionModel()
        selection.select(
            self.window.proxy.index(0, 0),
            QItemSelectionModel.Select | QItemSelectionModel.Rows,
        )
        self.window._update_actions()
        self.assertTrue(self.window.search_online_button.isEnabled())
        self.assertTrue(self.window.convert_button.isEnabled())

        selection.select(
            self.window.proxy.index(1, 0),
            QItemSelectionModel.Select | QItemSelectionModel.Rows,
        )
        self.window._update_actions()
        self.assertFalse(self.window.search_online_button.isEnabled())
        self.assertTrue(self.window.convert_button.isEnabled())

    def test_conversion_dialog_shows_real_ffmpeg_command(self):
        dialog = ConversionDialog([video("one.mp4", "convert", "h264", 100)])
        try:
            self.assertIn("libx265", dialog.preview.toPlainText())
            self.assertIn("one.h265.mkv", dialog.preview.toPlainText())
            anime_index = dialog.profile_combo.findData("anime")
            dialog.profile_combo.setCurrentIndex(anime_index)
            self.assertIn("aq-mode=3", dialog.preview.toPlainText())
            self.assertIn("animation", dialog.preview.toPlainText())
            dialog.profile_combo.setCurrentIndex(dialog.profile_combo.findData("custom"))
            dialog.crf_spin.setValue(27)
            self.assertIn("27", dialog.preview.toPlainText())
        finally:
            dialog.deleteLater()


if __name__ == "__main__":
    unittest.main()
