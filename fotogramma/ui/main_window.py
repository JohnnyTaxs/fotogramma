"""Native Qt main window for Fotogramma Desktop."""

from __future__ import annotations

from pathlib import Path

from PySide6.QtCore import QSettings, QSize, Qt, QUrl
from PySide6.QtGui import QCloseEvent, QDesktopServices
from PySide6.QtWidgets import (
    QAbstractItemView,
    QDialog,
    QFileDialog,
    QFrame,
    QGridLayout,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMessageBox,
    QProgressBar,
    QPushButton,
    QSizePolicy,
    QStackedWidget,
    QStyle,
    QTableView,
    QVBoxLayout,
    QWidget,
)

from fotogramma.core.scanner import get_ffprobe_path, get_ffprobe_version, summarize
from fotogramma.core.search import h265_search_url
from fotogramma.ui.conversion_dialog import ConversionDialog, ConversionProgressDialog
from fotogramma.ui.formatting import codec_label, format_bytes, format_duration, format_int
from fotogramma.ui.models import VideoFilterProxy, VideoTableModel
from fotogramma.ui.widgets import CompositionBar
from fotogramma.ui.workers import ScanWorker


class MainWindow(QMainWindow):
    def __init__(self) -> None:
        super().__init__()
        self.settings = QSettings("Fotogramma", "Desktop")
        self.worker: ScanWorker | None = None
        self.videos = []
        self.summary = summarize([])
        self.warnings = []
        self.cancelled = False

        self.setWindowTitle("Fotogramma — H.265 cinema library")
        self.setMinimumSize(1050, 680)
        self.resize(1280, 820)
        geometry = self.settings.value("geometry")
        if geometry:
            self.restoreGeometry(geometry)

        self.model = VideoTableModel(self)
        self.proxy = VideoFilterProxy(self)
        self.proxy.setSourceModel(self.model)
        self.proxy.rowsInserted.connect(self._update_visible_count)
        self.proxy.rowsRemoved.connect(self._update_visible_count)
        self.proxy.modelReset.connect(self._update_visible_count)
        self.proxy.layoutChanged.connect(self._update_visible_count)

        central = QWidget(self)
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)
        layout.setContentsMargins(24, 18, 24, 20)
        layout.setSpacing(16)
        layout.addWidget(self._build_header())
        layout.addWidget(self._build_control_deck())

        self.stack = QStackedWidget()
        self.stack.addWidget(self._build_welcome_page())
        self.stack.addWidget(self._build_results_page())
        self.stack.addWidget(self._build_error_page())
        layout.addWidget(self.stack, 1)

        self.directory_edit.setText(self.settings.value("directory", ""))
        self._refresh_engine_status()
        self.statusBar().showMessage("Nessun file viene modificato", 4000)

    def _build_header(self) -> QWidget:
        header = QWidget()
        layout = QHBoxLayout(header)
        layout.setContentsMargins(2, 0, 2, 0)
        title_box = QVBoxLayout()
        title = QLabel("Fotogramma")
        title.setObjectName("brandTitle")
        subtitle = QLabel("H.265 cinema library")
        subtitle.setObjectName("brandSubtitle")
        title_box.addWidget(title)
        title_box.addWidget(subtitle)
        layout.addLayout(title_box)
        layout.addStretch(1)
        privacy = QLabel("Solo sul PC · Solo lettura")
        privacy.setObjectName("muted")
        layout.addWidget(privacy)
        layout.addSpacing(16)
        self.engine_status = QLabel("Controllo ffprobe…")
        self.engine_status.setObjectName("engineStatus")
        layout.addWidget(self.engine_status)
        return header

    def _build_control_deck(self) -> QFrame:
        deck = QFrame()
        deck.setObjectName("controlDeck")
        grid = QGridLayout(deck)
        grid.setContentsMargins(18, 16, 18, 16)
        grid.setHorizontalSpacing(10)
        grid.setVerticalSpacing(7)

        title = QLabel("Cartella della libreria")
        title.setStyleSheet("font-weight: 650;")
        grid.addWidget(title, 0, 0)
        hint = QLabel("Analisi ricorsiva di MP4, MKV e WebM. Nessuna modifica ai file.")
        hint.setObjectName("fieldHint")
        grid.addWidget(hint, 1, 0)

        self.directory_edit = QLineEdit()
        self.directory_edit.setObjectName("mono")
        self.directory_edit.setPlaceholderText("Es. /Filmati oppure D:\\Video")
        self.directory_edit.setAccessibleName("Percorso della libreria")
        self.directory_edit.setClearButtonEnabled(True)
        self.directory_edit.returnPressed.connect(self.start_scan)
        grid.addWidget(self.directory_edit, 2, 0)

        self.browse_button = QPushButton("Sfoglia")
        self.browse_button.setIcon(self.style().standardIcon(QStyle.SP_DirIcon))
        self.browse_button.clicked.connect(self.browse_directory)
        grid.addWidget(self.browse_button, 2, 1)

        self.scan_button = QPushButton("Analizza libreria")
        self.scan_button.setObjectName("primaryButton")
        self.scan_button.setIcon(self.style().standardIcon(QStyle.SP_MediaPlay))
        self.scan_button.clicked.connect(self.start_scan)
        grid.addWidget(self.scan_button, 2, 2)

        self.progress_label = QLabel("Ricerca dei video…")
        self.progress_label.setObjectName("muted")
        self.progress_label.setVisible(False)
        grid.addWidget(self.progress_label, 3, 0)
        self.cancel_button = QPushButton("Interrompi")
        self.cancel_button.clicked.connect(self.cancel_scan)
        self.cancel_button.setVisible(False)
        grid.addWidget(self.cancel_button, 3, 1, 1, 2)

        self.progress_bar = QProgressBar()
        self.progress_bar.setTextVisible(False)
        self.progress_bar.setVisible(False)
        grid.addWidget(self.progress_bar, 4, 0, 1, 3)

        self.folder_error = QLabel()
        self.folder_error.setStyleSheet("color: #ff8585;")
        self.folder_error.setVisible(False)
        grid.addWidget(self.folder_error, 5, 0, 1, 3)
        return deck

    def _build_welcome_page(self) -> QWidget:
        panel = QFrame()
        panel.setObjectName("heroPanel")
        layout = QHBoxLayout(panel)
        layout.setContentsMargins(42, 32, 42, 32)
        layout.setSpacing(36)

        left = QVBoxLayout()
        title = QLabel("Pronta per H.265.")
        title.setObjectName("heroTitle")
        left.addWidget(title)
        subtitle = QLabel(
            "Scegli una cartella e avvia l’analisi. Fotogramma separa i video già pronti "
            "da quelli che potresti convertire, senza spostare o modificare nulla."
        )
        subtitle.setObjectName("heroSubtitle")
        subtitle.setWordWrap(True)
        left.addWidget(subtitle)
        left.addSpacing(18)
        for number, heading, description in (
            ("01", "Scansione", "Lettura del primo flusso video tramite ffprobe."),
            ("02", "Selezione", "Filtri, ricerca e ordinamento per isolare i candidati."),
            ("03", "Esportazione", "Un file di testo con i soli percorsi da convertire."),
        ):
            step = QLabel(f"{number}  {heading}\n{description}")
            step.setStyleSheet("color: #c7a9c5;")
            left.addWidget(step)
        left.addStretch(1)
        layout.addLayout(left, 3)

        right = QVBoxLayout()
        right.addStretch(1)
        for text, compatible in (("H.265 · HEVC", True), ("AV1 · AOMedia", True), ("H.264 · AVC", False), ("VP9 · WebM", False)):
            chip = QLabel(text)
            chip.setAlignment(Qt.AlignCenter)
            color = "#77ddbb" if compatible else "#ff9482"
            chip.setStyleSheet(
                f"color: {color}; background: #211127; border: 1px solid #62406b; "
                "border-radius: 12px; padding: 20px; font-family: 'IBM Plex Mono'; font-size: 18px;"
            )
            right.addWidget(chip)
        right.addStretch(1)
        layout.addLayout(right, 2)
        return panel

    def _build_results_page(self) -> QWidget:
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(13)

        heading = QHBoxLayout()
        title_box = QVBoxLayout()
        self.results_title = QLabel("La tua libreria")
        self.results_title.setObjectName("pageTitle")
        self.results_path = QLabel()
        self.results_path.setObjectName("mono")
        self.results_path.setTextInteractionFlags(Qt.TextSelectableByMouse)
        title_box.addWidget(self.results_title)
        title_box.addWidget(self.results_path)
        heading.addLayout(title_box)
        heading.addStretch(1)
        self.export_button = QPushButton("Esporta da convertire")
        self.export_button.setObjectName("primaryButton")
        self.export_button.setIcon(self.style().standardIcon(QStyle.SP_DialogSaveButton))
        self.export_button.clicked.connect(self.export_videos)
        heading.addWidget(self.export_button)
        layout.addLayout(heading)

        stats = QHBoxLayout()
        stats.setSpacing(10)
        self.stat_buttons = {}
        for key, label in (("all", "Video presenti"), ("convert", "Da convertire"), ("compatible", "Già compatibili"), ("error", "Non leggibili")):
            card = QPushButton()
            card.setObjectName("statCard")
            card.setCheckable(True)
            card.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred)
            card.clicked.connect(lambda _checked=False, category=key: self.set_filter(category))
            stats.addWidget(card, 2 if key == "convert" else 1)
            self.stat_buttons[key] = card
        self.stat_buttons["convert"].setProperty("accent", "convert")
        self.stat_buttons["convert"].setStyleSheet("font-size: 18px;")
        layout.addLayout(stats)

        composition_row = QHBoxLayout()
        composition_row.setSpacing(12)
        self.composition_bar = CompositionBar()
        composition_row.addWidget(self.composition_bar, 1)
        self.composition_caption = QLabel()
        self.composition_caption.setObjectName("muted")
        composition_row.addWidget(self.composition_caption)
        layout.addLayout(composition_row)

        self.partial_notice = QLabel()
        self.partial_notice.setStyleSheet("color: #ffab98;")
        self.partial_notice.setWordWrap(True)
        self.partial_notice.setVisible(False)
        layout.addWidget(self.partial_notice)

        self.insight_label = QLabel()
        self.insight_label.setObjectName("muted")
        self.insight_label.setWordWrap(True)
        layout.addWidget(self.insight_label)

        controls = QHBoxLayout()
        controls.setSpacing(8)
        self.filter_buttons = {}
        for key, label in (("all", "Tutti"), ("convert", "Da convertire"), ("compatible", "Compatibili"), ("error", "Errori")):
            button = QPushButton(label)
            button.setObjectName("filterButton")
            button.setCheckable(True)
            button.clicked.connect(lambda _checked=False, category=key: self.set_filter(category))
            controls.addWidget(button)
            self.filter_buttons[key] = button
        controls.addStretch(1)
        self.search_edit = QLineEdit()
        self.search_edit.setPlaceholderText("Cerca nella libreria…")
        self.search_edit.setClearButtonEnabled(True)
        self.search_edit.setAccessibleName("Cerca un video o una cartella")
        self.search_edit.setMaximumWidth(340)
        self.search_edit.textChanged.connect(self._search_changed)
        controls.addWidget(self.search_edit)
        layout.addLayout(controls)

        action_bar = QHBoxLayout()
        self.selected_label = QLabel("Seleziona un video per vedere le azioni")
        self.selected_label.setObjectName("muted")
        action_bar.addWidget(self.selected_label, 1)
        self.search_online_button = QPushButton("Cerca H.265 online")
        self.search_online_button.setIcon(self.style().standardIcon(QStyle.SP_DriveNetIcon))
        self.search_online_button.setToolTip("Apre 1337x.to nel browser; nessun download integrato")
        self.search_online_button.clicked.connect(self.open_h265_search)
        self.search_online_button.setEnabled(False)
        action_bar.addWidget(self.search_online_button)
        self.convert_button = QPushButton("Converti selezionati")
        self.convert_button.setObjectName("primaryButton")
        self.convert_button.setIcon(self.style().standardIcon(QStyle.SP_MediaPlay))
        self.convert_button.setToolTip("Crea nuovi file H.265 senza modificare gli originali")
        self.convert_button.clicked.connect(self.start_conversion)
        self.convert_button.setEnabled(False)
        action_bar.addWidget(self.convert_button)
        layout.addLayout(action_bar)

        self.table = QTableView()
        self.table.setModel(self.proxy)
        self.table.setAlternatingRowColors(True)
        self.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.table.setShowGrid(False)
        self.table.setSortingEnabled(True)
        self.table.sortByColumn(0, Qt.AscendingOrder)
        self.table.setIconSize(QSize(46, 30))
        self.table.setAccessibleName("Elenco dei video trovati")
        self.table.doubleClicked.connect(self.show_video_details)
        self.table.selectionModel().selectionChanged.connect(self._update_actions)
        self.table.verticalHeader().setVisible(False)
        self.table.verticalHeader().setDefaultSectionSize(50)
        self.table.horizontalHeader().setSectionResizeMode(0, QHeaderView.Stretch)
        for column in range(1, 6):
            self.table.horizontalHeader().setSectionResizeMode(column, QHeaderView.ResizeToContents)
        layout.addWidget(self.table, 1)

        self.visible_count = QLabel("Nessun video")
        self.visible_count.setObjectName("muted")
        layout.addWidget(self.visible_count)
        return page

    def _build_error_page(self) -> QWidget:
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.setAlignment(Qt.AlignCenter)
        layout.setSpacing(16)
        title = QLabel("L’analisi si è fermata")
        title.setObjectName("pageTitle")
        title.setAlignment(Qt.AlignCenter)
        layout.addWidget(title)
        self.error_label = QLabel()
        self.error_label.setObjectName("heroSubtitle")
        self.error_label.setAlignment(Qt.AlignCenter)
        self.error_label.setWordWrap(True)
        layout.addWidget(self.error_label)
        self.retry_button = QPushButton("Riprova")
        self.retry_button.setObjectName("primaryButton")
        self.retry_button.clicked.connect(self.start_scan)
        layout.addWidget(self.retry_button, 0, Qt.AlignCenter)
        return page

    def _refresh_engine_status(self) -> None:
        try:
            executable = get_ffprobe_path()
            version = get_ffprobe_version(executable)
            parts = version.split() if version else []
            self.engine_status.setText(f"ffprobe {parts[2]}" if len(parts) > 2 else "ffprobe non disponibile")
            self.engine_status.setStyleSheet("color: #77ddbb;" if version else "color: #ff8585;")
        except FileNotFoundError:
            self.engine_status.setText("ffprobe mancante")
            self.engine_status.setStyleSheet("color: #ff8585;")

    def browse_directory(self) -> None:
        if self.worker and self.worker.isRunning():
            return
        selected = QFileDialog.getExistingDirectory(
            self,
            "Scegli la cartella da analizzare",
            self.directory_edit.text() or str(Path.home()),
            QFileDialog.ShowDirsOnly | QFileDialog.DontResolveSymlinks,
        )
        if selected:
            self.directory_edit.setText(selected)
            self.folder_error.setVisible(False)

    def start_scan(self) -> None:
        if self.worker and self.worker.isRunning():
            self.folder_error.setText("È già in corso un’analisi.")
            self.folder_error.setVisible(True)
            return
        directory = self.directory_edit.text()
        if not directory.strip():
            self.folder_error.setText("Inserisci o scegli una cartella da analizzare.")
            self.folder_error.setVisible(True)
            self.directory_edit.setFocus()
            return
        if not Path(directory).is_dir():
            self.folder_error.setText("La cartella indicata non esiste o non è accessibile.")
            self.folder_error.setVisible(True)
            self.directory_edit.setFocus()
            return

        self.folder_error.setVisible(False)
        self.settings.setValue("directory", directory)
        self._set_busy(True)
        self.worker = ScanWorker(directory, self)
        self.worker.progress.connect(self._scan_progress)
        self.worker.scanCompleted.connect(self._scan_completed)
        self.worker.failed.connect(self._scan_failed)
        self.worker.finished.connect(self._worker_finished)
        self.worker.start()

    def _set_busy(self, busy: bool) -> None:
        self.scan_button.setDisabled(busy)
        self.browse_button.setDisabled(busy)
        self.directory_edit.setDisabled(busy)
        self.progress_label.setVisible(busy)
        self.progress_bar.setVisible(busy)
        self.cancel_button.setVisible(busy)
        self.cancel_button.setEnabled(busy)
        if busy:
            self.progress_bar.setRange(0, 0)
            self.progress_label.setText("Ricerca dei video…")

    def cancel_scan(self) -> None:
        if self.worker and self.worker.isRunning():
            self.cancel_button.setDisabled(True)
            self.progress_label.setText("Interruzione in corso…")
            self.worker.cancel()

    def _scan_progress(self, processed: int, total: int, current_file: str) -> None:
        if total == -1:
            self.progress_bar.setRange(0, 0)
            self.progress_label.setText(f"{format_int(processed)} video trovati · {current_file}")
        else:
            self.progress_bar.setRange(0, total)
            self.progress_bar.setValue(processed)
            self.progress_label.setText(f"{format_int(processed)} / {format_int(total)} · {current_file}")

    def _scan_completed(self, videos, warnings, cancelled) -> None:
        self.videos = videos
        self.warnings = warnings
        self.cancelled = cancelled
        self.summary = summarize(videos)
        self.model.set_videos(videos)
        self.table.sortByColumn(0, Qt.AscendingOrder)
        self.results_path.setText(self.settings.value("directory", ""))
        self._update_stat_cards()
        self.composition_bar.set_values(
            self.summary["total"],
            self.summary["to_convert"],
            self.summary["compatible"],
            self.summary["errors"],
        )
        total = self.summary["total"]
        if total:
            self.composition_caption.setText(
                f"{round(self.summary['to_convert'] / total * 100)}% da convertire · "
                f"{round(self.summary['compatible'] / total * 100)}% compatibili"
            )
        else:
            self.composition_caption.setText("Nessun video trovato")
        convert_count = self.summary["to_convert"]
        if convert_count:
            self.insight_label.setText(
                f"{format_int(convert_count)} file richiedono un’azione. Selezionali nella tabella per cercare online o convertire direttamente."
            )
        else:
            self.insight_label.setText("Tutti i file sono già compatibili con H.265 o AV1: non serve alcuna conversione.")
        self.partial_notice.setVisible(cancelled or bool(warnings))
        messages = []
        if cancelled:
            messages.append("Analisi interrotta: i risultati sono parziali.")
        if warnings:
            messages.append(f"{len(warnings)} cartelle non accessibili: il riepilogo può essere incompleto.")
        self.partial_notice.setText(" ".join(messages))
        self.partial_notice.setToolTip("\n".join(f"{item['path']}: {item['message']}" for item in warnings))
        self.set_filter("convert")
        self.stack.setCurrentIndex(1)

    def _scan_failed(self, message: str) -> None:
        self.error_label.setText(f"{message}\n\nControlla la cartella e che ffprobe sia disponibile, poi riprova.")
        self.stack.setCurrentIndex(2)

    def _worker_finished(self) -> None:
        self._set_busy(False)
        if self.worker:
            self.worker.deleteLater()
            self.worker = None

    def _update_stat_cards(self) -> None:
        values = {
            "all": (self.summary["total"], f"{format_bytes(self.summary['total_bytes'])} analizzati"),
            "convert": (self.summary["to_convert"], f"{format_bytes(self.summary['to_convert_bytes'])} da elaborare"),
            "compatible": (self.summary["compatible"], "H.265 oppure AV1"),
            "error": (self.summary["errors"], "Da controllare"),
        }
        labels = {"all": "Video presenti", "convert": "Da convertire", "compatible": "Già compatibili", "error": "Non leggibili"}
        for key, card in self.stat_buttons.items():
            value, detail = values[key]
            card.setText(f"{format_int(value)} · {labels[key]}\n{detail}")
            card.setAccessibleName(f"{labels[key]} {value}. {detail}")

    def set_filter(self, category: str) -> None:
        self.proxy.set_category(category)
        for key, button in self.stat_buttons.items():
            button.setChecked(key == category)
        for key, button in self.filter_buttons.items():
            button.setChecked(key == category)
        self._update_visible_count()
        self._update_actions()

    def _search_changed(self, text: str) -> None:
        self.proxy.set_query(text)
        self._update_visible_count()

    def _update_visible_count(self) -> None:
        if not hasattr(self, "visible_count"):
            return
        count = self.proxy.rowCount()
        self.visible_count.setText(f"{format_int(count)} video in questa vista")

    def _selected_convertible(self):
        if not self.table.selectionModel():
            return []
        selected = []
        for proxy_index in self.table.selectionModel().selectedRows():
            source_index = self.proxy.mapToSource(proxy_index)
            video = self.model.video(source_index.row())
            if video.category == "convert":
                selected.append(video)
        return selected

    def _update_actions(self) -> None:
        videos = self._selected_convertible()
        self.search_online_button.setEnabled(len(videos) == 1)
        self.convert_button.setEnabled(bool(videos))
        if videos:
            self.selected_label.setText(f"{format_int(len(videos))} video da convertire selezionati")
        else:
            self.selected_label.setText("Seleziona uno o più video da convertire per vedere le azioni")

    def open_h265_search(self) -> None:
        videos = self._selected_convertible()
        if len(videos) == 1:
            QDesktopServices.openUrl(QUrl(h265_search_url(videos[0].name)))

    def start_conversion(self) -> None:
        videos = self._selected_convertible()
        if not videos:
            return
        dialog = ConversionDialog(videos, self)
        if dialog.exec() != QDialog.Accepted:
            return
        progress = ConversionProgressDialog(
            videos,
            dialog.profile,
            dialog.destination_dir,
            dialog.container,
            replace_original=dialog.replace_original,
            parent=self,
        )
        progress.exec()

    def show_video_details(self, proxy_index) -> None:
        source_index = self.proxy.mapToSource(proxy_index)
        video = self.model.video(source_index.row())
        details = (
            f"Percorso: {video.path}\n\n"
            f"Codec: {codec_label(video.codec)}\n"
            f"Durata: {format_duration(video.duration)}\n"
            f"Risoluzione: {video.width or '—'} × {video.height or '—'}\n"
            f"Dimensione: {format_bytes(video.size)}\n"
            f"Stato: {self.model._status(video)}"
        )
        if video.error:
            details += f"\n\nErrore: {video.error}"
        QMessageBox.information(self, video.name, details)

    def export_videos(self) -> None:
        convertible = [video.path for video in self.videos if video.category == "convert"]
        if not convertible:
            self.statusBar().showMessage("Nessun video da convertire", 4000)
            return
        default_path = str(Path.home() / "fotogramma-da-convertire.txt")
        filename, _selected_filter = QFileDialog.getSaveFileName(
            self,
            "Esporta i video da convertire",
            default_path,
            "File di testo (*.txt)",
        )
        if not filename:
            return
        try:
            with open(filename, "w", encoding="utf-8", newline="\n") as output:
                output.write("\n".join(convertible) + "\n")
        except OSError as exc:
            QMessageBox.critical(self, "Esportazione non riuscita", str(exc))
            return
        self.statusBar().showMessage(f"Esportati {format_int(len(convertible))} percorsi in {filename}", 7000)

    def closeEvent(self, event: QCloseEvent) -> None:
        if self.worker and self.worker.isRunning():
            answer = QMessageBox.question(
                self,
                "Analisi in corso",
                "Vuoi interrompere l’analisi e chiudere Fotogramma?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No,
            )
            if answer != QMessageBox.Yes:
                event.ignore()
                return
            self.worker.cancel()
            if not self.worker.wait(4000):
                self.progress_label.setText("Attendi l’interruzione dei processi…")
                event.ignore()
                return
        self.settings.setValue("geometry", self.saveGeometry())
        event.accept()
