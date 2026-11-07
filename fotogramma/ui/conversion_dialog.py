"""Native profile and progress dialogs for direct FFmpeg conversion."""

from __future__ import annotations

from pathlib import Path
import shlex

from PySide6.QtCore import QUrl, Qt
from PySide6.QtGui import QDesktopServices
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QFileDialog,
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPlainTextEdit,
    QProgressBar,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
)

from fotogramma.core.conversion import PROFILES, allocate_output_path, allocate_partial_path, build_ffmpeg_args, custom_profile
from fotogramma.ui.workers import ConversionWorker


class ConversionDialog(QDialog):
    def __init__(self, videos, parent=None) -> None:
        super().__init__(parent)
        self.videos = videos
        self.profile = PROFILES["balanced"]
        self.container = "mkv"
        self.destination_dir = str(Path(videos[0].path).parent / "Fotogramma-H265")
        self.setWindowTitle("Converti in H.265")
        self.setMinimumWidth(780)
        layout = QVBoxLayout(self)
        layout.setSpacing(14)

        self.count_label = QLabel(f"{len(videos)} file selezionati. Gli originali restano invariati.")
        self.count_label.setStyleSheet("font-weight: 650;")
        layout.addWidget(self.count_label)

        form = QFormLayout()
        self.profile_combo = QComboBox()
        for key, profile in PROFILES.items():
            self.profile_combo.addItem(profile.label, key)
        self.profile_combo.addItem("Personalizzato", "custom")
        self.profile_combo.currentIndexChanged.connect(self._profile_changed)
        form.addRow("Profilo", self.profile_combo)

        self.profile_description = QLabel()
        self.profile_description.setObjectName("muted")
        self.profile_description.setWordWrap(True)
        form.addRow("", self.profile_description)

        self.crf_spin = QSpinBox()
        self.crf_spin.setRange(16, 35)
        self.crf_spin.setValue(22)
        form.addRow("CRF · qualità", self.crf_spin)

        self.preset_combo = QComboBox()
        self.preset_combo.addItems(["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"])
        form.addRow("Preset · velocità", self.preset_combo)

        self.pixel_combo = QComboBox()
        self.pixel_combo.addItem("10-bit · banding ridotto", "yuv420p10le")
        self.pixel_combo.addItem("8-bit · massima compatibilità", "yuv420p")
        form.addRow("Pixel format", self.pixel_combo)

        self.tune_combo = QComboBox()
        self.tune_combo.addItem("Nessuno", "")
        self.tune_combo.addItem("Animation", "animation")
        self.tune_combo.addItem("Film", "film")
        self.tune_combo.addItem("Grain", "grain")
        form.addRow("Tuning contenuto", self.tune_combo)

        self.aq_combo = QComboBox()
        self.aq_combo.addItem("Automatico", None)
        for value in range(4):
            self.aq_combo.addItem(f"AQ mode {value}", value)
        form.addRow("Adaptive quantization", self.aq_combo)

        self.audio_combo = QComboBox()
        self.audio_combo.addItem("Originale · nessuna ricompressione", "copy")
        self.audio_combo.addItem("AAC 128 kbps", 128)
        self.audio_combo.addItem("AAC 160 kbps", 160)
        self.audio_combo.addItem("AAC 256 kbps", 256)
        form.addRow("Audio", self.audio_combo)

        self.container_combo = QComboBox()
        self.container_combo.addItem("MKV · consigliato", "mkv")
        self.container_combo.addItem("MP4 · faststart", "mp4")
        form.addRow("Contenitore", self.container_combo)
        layout.addLayout(form)

        self.custom_fields = [
            self.crf_spin,
            self.preset_combo,
            self.pixel_combo,
            self.tune_combo,
            self.aq_combo,
        ]

        destination_group = QGroupBox("Destinazione · mai sovrascrivere")
        destination_layout = QHBoxLayout(destination_group)
        self.destination_edit = QLineEdit(self.destination_dir)
        destination_layout.addWidget(self.destination_edit, 1)
        self.destination_browse = QPushButton("Scegli…")
        self.destination_browse.clicked.connect(self._browse_destination)
        destination_layout.addWidget(self.destination_browse)
        layout.addWidget(destination_group)
        self.destination_group = destination_group

        self.replace_checkbox = QCheckBox("Sostituisci gli originali solo dopo verifica completa")
        self.replace_checkbox.setToolTip("Scrive prima un file temporaneo, esegue tutti i test e solo dopo sostituisce l’originale con backup di sicurezza")
        self.replace_checkbox.toggled.connect(self._replacement_changed)
        layout.addWidget(self.replace_checkbox)

        preview_label = QLabel("Anteprima del comando FFmpeg")
        preview_label.setStyleSheet("font-weight: 650;")
        layout.addWidget(preview_label)
        self.preview = QPlainTextEdit()
        self.preview.setReadOnly(True)
        self.preview.setMaximumHeight(125)
        self.preview.setStyleSheet("font-family: 'IBM Plex Mono'; font-size: 10px;")
        layout.addWidget(self.preview)

        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.button(QDialogButtonBox.Ok).setText("Avvia conversione")
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

        self.profile_combo.currentIndexChanged.connect(self._update_preview)
        for widget in (self.preset_combo, self.pixel_combo, self.tune_combo, self.aq_combo, self.audio_combo, self.container_combo):
            widget.currentIndexChanged.connect(self._update_preview)
        self.crf_spin.valueChanged.connect(self._update_preview)
        self.destination_edit.textChanged.connect(self._update_preview)
        self._profile_changed()

    def _profile_changed(self) -> None:
        key = self.profile_combo.currentData()
        is_custom = key == "custom"
        for widget in self.custom_fields:
            widget.setEnabled(is_custom)
        if not is_custom:
            profile = PROFILES[key]
            self.profile_description.setText(profile.description)
            self.crf_spin.setValue(profile.crf)
            self.preset_combo.setCurrentText(profile.preset)
            self.pixel_combo.setCurrentIndex(self.pixel_combo.findData(profile.pixel_format))
            self.tune_combo.setCurrentIndex(self.tune_combo.findData(profile.tune or ""))
            self.aq_combo.setCurrentIndex(self.aq_combo.findData(profile.aq_mode))
        else:
            self.profile_description.setText("Configura manualmente i parametri video. Audio e contenitore restano modificabili in ogni profilo.")
        self._update_preview()

    def _update_preview(self) -> None:
        profile = self.current_profile()
        self.profile = profile
        self.container = self.container_combo.currentData()
        self.destination_dir = self.destination_edit.text()
        replacing = self.replace_checkbox.isChecked()
        self.replace_original = replacing
        if replacing and Path(self.videos[0].path).suffix.lower() not in {".mkv", ".mp4"}:
            self.preview.setPlainText("La sostituzione dell’originale è disponibile solo per MKV e MP4.")
            return
        if replacing:
            self.container = Path(self.videos[0].path).suffix.lower().lstrip(".")
            destination = allocate_partial_path(self.videos[0].path)
        else:
            if not self.destination_dir.strip():
                self.preview.setPlainText("Scegli una cartella di destinazione.")
                return
            destination = allocate_output_path(self.videos[0].path, self.destination_dir, self.container)
        args = build_ffmpeg_args(profile, self.videos[0].path, str(destination))
        self.preview.setPlainText(shlex.join(["ffmpeg", *args]))

    def current_profile(self):
        key = self.profile_combo.currentData()
        if key != "custom":
            return PROFILES[key]
        audio_data = self.audio_combo.currentData()
        return custom_profile(
            self.crf_spin.value(),
            self.preset_combo.currentText(),
            self.pixel_combo.currentData(),
            self.tune_combo.currentData(),
            self.aq_combo.currentData(),
            "copy" if audio_data == "copy" else "aac",
            audio_data if audio_data != "copy" else 160,
        )

    def _replacement_changed(self, checked: bool) -> None:
        self.count_label.setText(
            f"{len(self.videos)} file. Gli originali saranno sostituiti solo dopo test completi."
            if checked
            else f"{len(self.videos)} file selezionati. Gli originali restano invariati."
        )
        self.destination_edit.setDisabled(checked)
        self.destination_browse.setDisabled(checked)
        self.container_combo.setDisabled(checked)
        if checked:
            container = Path(self.videos[0].path).suffix.lower().lstrip(".")
            self.container_combo.setCurrentIndex(self.container_combo.findData(container))
        self.destination_group.setTitle(
            "Sostituzione solo dopo test completi" if checked else "Destinazione · mai sovrascrivere"
        )
        self._update_preview()

    def _browse_destination(self) -> None:
        selected = QFileDialog.getExistingDirectory(self, "Cartella di destinazione", self.destination_edit.text())
        if selected:
            self.destination_edit.setText(selected)


class ConversionProgressDialog(QDialog):
    def __init__(self, videos, profile, destination_dir: str, container: str, replace_original: bool = False, parent=None) -> None:
        super().__init__(parent)
        self.setWindowTitle("Conversione in corso")
        self.setMinimumWidth(680)
        self.destination_dir = destination_dir
        self.worker = ConversionWorker(videos, profile, destination_dir, container, replace_original=replace_original, parent=self)
        self.finished = False
        self.results = []

        layout = QVBoxLayout(self)
        self.label = QLabel("Preparazione…")
        layout.addWidget(self.label)
        self.progress_bar = QProgressBar()
        self.progress_bar.setRange(0, 100)
        self.progress_bar.setValue(0)
        layout.addWidget(self.progress_bar)
        self.log = QPlainTextEdit()
        self.log.setReadOnly(True)
        self.log.setStyleSheet("font-family: 'IBM Plex Mono'; font-size: 10px;")
        layout.addWidget(self.log, 1)

        buttons = QHBoxLayout()
        self.open_button = QPushButton("Apri cartella")
        self.open_button.clicked.connect(self.open_destination)
        self.open_button.setVisible(False)
        buttons.addWidget(self.open_button)
        buttons.addStretch(1)
        self.cancel_button = QPushButton("Interrompi")
        self.cancel_button.clicked.connect(self.cancel)
        buttons.addWidget(self.cancel_button)
        self.done_button = QPushButton("Chiudi")
        self.done_button.setObjectName("primaryButton")
        self.done_button.clicked.connect(self.accept)
        self.done_button.setEnabled(False)
        buttons.addWidget(self.done_button)
        layout.addLayout(buttons)

        self.worker.fileStarted.connect(self._file_started)
        self.worker.progress.connect(self._progress)
        self.worker.verificationStarted.connect(self._verification_started)
        self.worker.fileCompleted.connect(self._file_completed)
        self.worker.conversionCompleted.connect(self._completed)
        self.worker.failed.connect(self._failed)
        self.worker.finished.connect(self.worker.deleteLater)
        self.worker.start()

    def _file_started(self, index: int, total: int, source: str, destination: str) -> None:
        self.label.setText(f"{index} / {total} · {Path(source).name}")
        self.log.appendPlainText(f"Avvio: {source}\nOutput: {destination}")
        self.progress_bar.setValue(0)

    def _progress(self, _index: int, _total: int, percent: int, current_file: str) -> None:
        self.progress_bar.setValue(percent)
        self.label.setText(f"{Path(current_file).name} · {percent}%")

    def _verification_started(self, _index: int, current_file: str) -> None:
        self.label.setText(f"Verifica completa: {Path(current_file).name}")
        self.log.appendPlainText("Verifica completa prima della sostituzione…")

    def _file_completed(self, result) -> None:
        self.results.append(result)
        if result.verified:
            self.log.appendPlainText("Verifica superata ✓ · originale sostituito")
        elif result.success:
            self.log.appendPlainText("Completato ✓")
        else:
            self.log.appendPlainText(f"Errore: {result.message}")

    def _completed(self, results) -> None:
        self.finished = True
        successes = sum(result.success for result in results)
        errors = len(results) - successes
        replacements = sum(result.replaced for result in results)
        summary = f"Conversione terminata: {successes} riuscite, {errors} errori"
        if replacements:
            summary += f" · {replacements} originali sostituiti dopo verifica"
        self.label.setText(summary)
        self.progress_bar.setValue(100)
        self.cancel_button.setVisible(False)
        self.done_button.setEnabled(True)
        self.open_button.setVisible(successes > 0)

    def _failed(self, message: str) -> None:
        self.log.appendPlainText(message)
        self.label.setText("Conversione non riuscita")
        self.cancel_button.setVisible(False)
        self.done_button.setEnabled(True)
        self.finished = True

    def cancel(self) -> None:
        self.cancel_button.setDisabled(True)
        self.label.setText("Interruzione in corso…")
        self.worker.cancel()

    def open_destination(self) -> None:
        QDesktopServices.openUrl(QUrl.fromLocalFile(self.destination_dir))

    def closeEvent(self, event) -> None:
        if not self.finished and self.worker.isRunning():
            answer = self.cancel_button.text()
            if answer:
                self.worker.cancel()
                if not self.worker.wait(4000):
                    event.ignore()
                    return
        event.accept()
