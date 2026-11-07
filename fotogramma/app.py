"""Application entry point for the native desktop app."""

from __future__ import annotations

import argparse
import sys

from PySide6.QtCore import QTimer
from PySide6.QtGui import QFontDatabase, QIcon
from PySide6.QtWidgets import QApplication

from runtime import resource_path
from fotogramma.ui.main_window import MainWindow
from fotogramma.ui.theme import QSS


def load_fonts() -> None:
    for relative in ("static/fonts/Sora-Variable.ttf", "static/fonts/IBMPlexMono-Regular.ttf"):
        path = resource_path(relative)
        if path.is_file():
            QFontDatabase.addApplicationFont(str(path))


def parse_args(argv=None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Fotogramma Desktop")
    parser.add_argument("--directory", help="Cartella da precompilare nel campo libreria")
    parser.add_argument("--scan-on-start", action="store_true", help="Avvia subito la scansione della cartella indicata")
    parser.add_argument("--smoke-test", action="store_true", help="Chiudi automaticamente dopo la verifica iniziale")
    return parser.parse_args(argv)


def main(argv=None) -> int:
    args = parse_args(argv)
    app = QApplication(sys.argv)
    app.setApplicationName("Fotogramma")
    app.setOrganizationName("Fotogramma")
    app.setApplicationDisplayName("Fotogramma — H.265 cinema library")
    app.setWindowIcon(QIcon(str(resource_path("static/icons/fotogramma.png"))))
    load_fonts()
    app.setStyleSheet(QSS)

    window = MainWindow()
    if args.directory:
        window.directory_edit.setText(args.directory)
    if args.scan_on_start:
        QTimer.singleShot(0, window.start_scan)
    window.show()
    if args.smoke_test and args.scan_on_start:
        def quit_when_finished():
            if window.worker is None and window.stack.currentIndex() == 1:
                app.quit()
            else:
                QTimer.singleShot(100, quit_when_finished)

        QTimer.singleShot(100, quit_when_finished)
        QTimer.singleShot(30000, app.quit)
    elif args.smoke_test:
        QTimer.singleShot(2000, app.quit)
    return app.exec()
