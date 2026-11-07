"""Cinema Pop styling for native Qt widgets."""

COLORS = {
    "bg": "#28152f",
    "bg_deep": "#211127",
    "surface": "#351a40",
    "raised": "#48234f",
    "line": "#62406b",
    "text": "#fcecf6",
    "muted": "#c7a9c5",
    "coral": "#ff9482",
    "coral_ink": "#2b172f",
    "lavender": "#d4b4db",
    "mint": "#77ddbb",
    "error": "#ff8585",
}

QSS = """
* { color: #fcecf6; font-family: "Sora"; font-size: 13px; }
QWidget { background: transparent; }
QMainWindow, QDialog, QStackedWidget { background: #28152f; }
QLabel#brandTitle { font-size: 21px; font-weight: 650; letter-spacing: -0.04em; }
QLabel#brandSubtitle, QLabel#muted, QLabel#fieldHint { color: #c7a9c5; font-size: 11px; }
QLabel#pageTitle { font-size: 28px; font-weight: 650; letter-spacing: -0.045em; }
QLabel#heroTitle { font-size: 46px; font-weight: 650; letter-spacing: -0.055em; }
QLabel#heroSubtitle { color: #c7a9c5; font-size: 14px; line-height: 1.6; }
QLabel#mono, QLabel#engineStatus { font-family: "IBM Plex Mono"; font-size: 10px; }
QFrame#controlDeck, QFrame#heroPanel { background: #351a40; border: 1px solid #62406b; border-radius: 16px; }
QLineEdit, QComboBox { background: #28152f; border: 1px solid #70487a; border-radius: 8px; padding: 10px 12px; selection-background-color: #ff9482; selection-color: #2b172f; }
QLineEdit:focus, QComboBox:focus { border-color: #ff9482; }
QLineEdit:disabled { color: #806886; background: #24132e; }
QComboBox QAbstractItemView { background: #351a40; border: 1px solid #70487a; selection-background-color: #ff9482; selection-color: #2b172f; }
QPushButton { background: #48234f; border: 1px solid #70487a; border-radius: 8px; padding: 10px 14px; font-weight: 600; }
QPushButton:hover { background: #51295c; border-color: #7b5583; }
QPushButton:pressed { background: #ffab98; color: #2b172f; }
QPushButton:disabled { color: #806886; background: #302035; border-color: #503454; }
QPushButton#primaryButton { color: #2b172f; background: #ff9482; border-color: #ff9482; }
QPushButton#primaryButton:hover { background: #ffab98; }
QPushButton#statCard { background: #351a40; border: 1px solid #62406b; border-radius: 12px; padding: 16px; text-align: left; }
QPushButton#statCard:checked { border: 2px solid #ffab98; }
QPushButton#statCard[accent="convert"] { color: #2b172f; background: #ff9482; border-color: #ff9482; }
QPushButton#statCard[accent="convert"]:checked { border: 2px solid #2b172f; }
QPushButton#filterButton { border-radius: 18px; padding: 8px 13px; color: #d4b4db; }
QPushButton#filterButton:checked { color: #2b172f; background: #ff9482; border-color: #ff9482; }
QTableView { background: #351a40; alternate-background-color: #2f1838; border: 1px solid #62406b; border-radius: 14px; gridline-color: #4e2c58; selection-background-color: #67394e; selection-color: #fcecf6; }
QTableView::item { border-bottom: 1px solid #4e2c58; padding: 7px; }
QHeaderView::section { background: #2c1733; color: #d4b4db; border: 0; padding: 10px 8px; font-family: "IBM Plex Mono"; font-size: 10px; }
QHeaderView::section:first { border-top-left-radius: 12px; }
QHeaderView::section:last { border-top-right-radius: 12px; }
QProgressBar { background: #55305e; border: 0; border-radius: 4px; height: 8px; text-align: center; }
QProgressBar::chunk { background: #ff9482; border-radius: 4px; }
QScrollBar:vertical { background: #211127; width: 12px; margin: 2px; }
QScrollBar::handle:vertical { background: #70487a; border-radius: 5px; min-height: 28px; }
QScrollBar::handle:vertical:hover { background: #ff9482; }
QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical { height: 0; }
QScrollBar:horizontal { background: #211127; height: 12px; margin: 2px; }
QScrollBar::handle:horizontal { background: #70487a; border-radius: 5px; min-width: 28px; }
QScrollBar::add-line:horizontal, QScrollBar::sub-line:horizontal { width: 0; }
QToolTip { color: #fcecf6; background: #351a40; border: 1px solid #ff9482; padding: 6px; }
QMessageBox QLabel { min-width: 360px; }
"""
