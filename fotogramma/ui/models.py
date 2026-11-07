"""Efficient native table model and filtering proxy for large libraries."""

from __future__ import annotations

from PySide6.QtCore import QAbstractTableModel, QModelIndex, QSortFilterProxyModel, Qt
from PySide6.QtGui import QBrush, QColor, QFont, QIcon, QPainter, QPainterPath, QPixmap

from fotogramma.core.scanner import VideoInfo
from fotogramma.ui.formatting import codec_label, format_bytes, format_duration
from fotogramma.ui.theme import COLORS

INFO_ROLE = Qt.UserRole + 1
SORT_ROLE = Qt.UserRole + 2
CATEGORY_ROLE = Qt.UserRole + 3
CATEGORY_ORDER = {"convert": 0, "compatible": 1, "error": 2}


def badge_icon(text: str) -> QIcon:
    pixmap = QPixmap(50, 34)
    pixmap.fill(Qt.transparent)
    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.Antialiasing)
    path = QPainterPath()
    path.addRoundedRect(1, 1, 48, 32, 6, 6)
    painter.fillPath(path, QColor("#67394e"))
    painter.setPen(QColor("#ffd9d0"))
    font = QFont("IBM Plex Mono", 8)
    painter.setFont(font)
    painter.drawText(pixmap.rect(), Qt.AlignCenter, text.upper()[:6])
    painter.end()
    return QIcon(pixmap)


class VideoTableModel(QAbstractTableModel):
    HEADERS = ("Video", "Codec", "Durata", "Risoluzione", "Dimensione", "Azione")

    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self.videos: list[VideoInfo] = []
        self._badges: dict[str, QIcon] = {}

    def rowCount(self, _parent=QModelIndex()) -> int:
        return len(self.videos)

    def columnCount(self, _parent=QModelIndex()) -> int:
        return len(self.HEADERS)

    def set_videos(self, videos: list[VideoInfo]) -> None:
        self.beginResetModel()
        self.videos = videos
        self.endResetModel()

    def video(self, row: int) -> VideoInfo:
        return self.videos[row]

    def headerData(self, section, orientation, role=Qt.DisplayRole):
        if orientation == Qt.Horizontal and role == Qt.DisplayRole:
            return self.HEADERS[section]
        return super().headerData(section, orientation, role)

    def data(self, index: QModelIndex, role=Qt.DisplayRole):
        if not index.isValid():
            return None
        video = self.videos[index.row()]
        column = index.column()
        if role == INFO_ROLE:
            return video
        if role == CATEGORY_ROLE:
            return video.category
        if role == SORT_ROLE:
            return self._sort_value(video, column)
        if role == Qt.ToolTipRole:
            details = video.path
            if video.error:
                details += f"\n\n{video.error}"
            return details
        if role == Qt.AccessibleDescriptionRole:
            return f"{video.name}, {video.relative_path}, {self._status(video)}"
        if role == Qt.DecorationRole and column == 0:
            extension = video.extension or "video"
            if extension not in self._badges:
                self._badges[extension] = badge_icon(extension)
            return self._badges[extension]
        if role == Qt.TextAlignmentRole:
            if column in (2, 4):
                return Qt.AlignRight | Qt.AlignVCenter
            return Qt.AlignLeft | Qt.AlignVCenter
        if role == Qt.ForegroundRole and column == 5:
            colors = {
                "convert": COLORS["coral"],
                "compatible": COLORS["mint"],
                "error": COLORS["error"],
            }
            return QBrush(QColor(colors[video.category]))
        if role == Qt.FontRole:
            font = QFont("IBM Plex Mono" if column in (1, 2, 4) else "Sora", 10)
            if column in (0, 5):
                font.setWeight(QFont.DemiBold)
            return font
        if role != Qt.DisplayRole:
            return None
        if column == 0:
            folder = video.folder or "Cartella principale"
            return f"{video.name}\n{folder}"
        if column == 1:
            return codec_label(video.codec)
        if column == 2:
            return format_duration(video.duration)
        if column == 3:
            return f"{video.width} × {video.height}" if video.width and video.height else "—"
        if column == 4:
            return format_bytes(video.size)
        if column == 5:
            return self._status(video)
        return None

    @staticmethod
    def _status(video: VideoInfo) -> str:
        if video.category == "convert":
            return "Converti in H.265"
        if video.category == "error":
            return "Verifica file"
        return "Pronto"

    @staticmethod
    def _sort_value(video: VideoInfo, column: int):
        if column == 0:
            return video.relative_path.casefold()
        if column == 1:
            return video.codec or ""
        if column == 2:
            return video.duration if video.duration is not None else -1
        if column == 3:
            return (video.width or 0) * (video.height or 0)
        if column == 4:
            return video.size if video.size is not None else -1
        return CATEGORY_ORDER.get(video.category, 99)


class VideoFilterProxy(QSortFilterProxyModel):
    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self.category = "all"
        self.query = ""
        self.setSortRole(SORT_ROLE)
        self.setFilterCaseSensitivity(Qt.CaseInsensitive)

    def set_category(self, category: str) -> None:
        if category != self.category:
            self.category = category
            self.invalidate()

    def set_query(self, query: str) -> None:
        normalized = query.strip().casefold()
        if normalized != self.query:
            self.query = normalized
            self.invalidate()

    def filterAcceptsRow(self, source_row: int, _source_parent: QModelIndex) -> bool:
        video = self.sourceModel().video(source_row)
        if self.category != "all" and video.category != self.category:
            return False
        if not self.query:
            return True
        haystack = f"{video.name} {video.relative_path} {video.codec or ''}".casefold()
        return self.query in haystack
