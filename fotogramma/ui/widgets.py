"""Small native widgets used by the main window."""

from __future__ import annotations

from PySide6.QtCore import Qt
from PySide6.QtGui import QColor, QPainter, QPainterPath
from PySide6.QtWidgets import QWidget


class CompositionBar(QWidget):
    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self.total = 0
        self.convert = 0
        self.compatible = 0
        self.errors = 0
        self.setMinimumHeight(10)
        self.setMaximumHeight(10)

    def set_values(self, total: int, convert: int, compatible: int, errors: int) -> None:
        self.total = total
        self.convert = convert
        self.compatible = compatible
        self.errors = errors
        self.update()

    def paintEvent(self, _event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        path = QPainterPath()
        path.addRoundedRect(0, 0, self.width(), self.height(), 4, 4)
        painter.fillPath(path, QColor("#55305e"))
        if self.total <= 0:
            return
        segments = (
            (self.convert, QColor("#ff9482")),
            (self.compatible, QColor("#77ddbb")),
            (self.errors, QColor("#ff8585")),
        )
        painter.setClipPath(path)
        x = 0.0
        for count, color in segments:
            width = self.width() * count / self.total
            painter.fillRect(int(x), 0, int(width + 1), self.height(), color)
            x += width
