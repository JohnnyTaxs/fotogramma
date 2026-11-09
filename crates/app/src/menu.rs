//! The application menu, defined once and drawn twice: natively in the menu bar
//! on macOS (`native_menu.rs`), and as a bar at the top of the window on
//! Windows and Linux (`show_bar` below). Only what the app really does is in
//! it; every entry is a [`MenuAction`] that the application carries out.

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use fotogramma_core::i18n::tr_str;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuAction {
    ChooseFolder,
    Scan,
    Export,
    ConvertSelected,
    SelectAll,
    ClearSelection,
    FocusSearch,
    FilterAll,
    FilterConvert,
    FilterCompatible,
    FilterErrors,
    Settings,
    CheckUpdates,
    About,
    Minimize,
    ToggleMaximize,
    Quit,
}

pub const ALL_ACTIONS: [MenuAction; 17] = [
    MenuAction::ChooseFolder,
    MenuAction::Scan,
    MenuAction::Export,
    MenuAction::ConvertSelected,
    MenuAction::SelectAll,
    MenuAction::ClearSelection,
    MenuAction::FocusSearch,
    MenuAction::FilterAll,
    MenuAction::FilterConvert,
    MenuAction::FilterCompatible,
    MenuAction::FilterErrors,
    MenuAction::Settings,
    MenuAction::CheckUpdates,
    MenuAction::About,
    MenuAction::Minimize,
    MenuAction::ToggleMaximize,
    MenuAction::Quit,
];

/// A shortcut is always Command (Cmd on macOS, Ctrl elsewhere) plus a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub key: Key,
    pub shift: bool,
}

impl Shortcut {
    pub fn egui(self) -> KeyboardShortcut {
        let modifiers = if self.shift {
            Modifiers::COMMAND | Modifiers::SHIFT
        } else {
            Modifiers::COMMAND
        };
        KeyboardShortcut::new(modifiers, self.key)
    }

    /// `Ctrl+Shift+A` as shown in the in-window menu.
    pub fn text(self) -> String {
        let key = match self.key {
            Key::Comma => ",".to_string(),
            Key::Enter => "Enter".to_string(),
            Key::Num1 => "1".to_string(),
            Key::Num2 => "2".to_string(),
            Key::Num3 => "3".to_string(),
            Key::Num4 => "4".to_string(),
            other => format!("{other:?}"),
        };
        if self.shift {
            format!("Ctrl+Shift+{key}")
        } else {
            format!("Ctrl+{key}")
        }
    }

    /// The accelerator in the form `muda` reads: `CmdOrCtrl+Shift+A`.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn accelerator(self) -> String {
        let key = match self.key {
            Key::Comma => ",".to_string(),
            Key::Enter => "Enter".to_string(),
            Key::Num1 => "1".to_string(),
            Key::Num2 => "2".to_string(),
            Key::Num3 => "3".to_string(),
            Key::Num4 => "4".to_string(),
            other => format!("{other:?}"),
        };
        if self.shift {
            format!("CmdOrCtrl+Shift+{key}")
        } else {
            format!("CmdOrCtrl+{key}")
        }
    }
}

const fn plain(key: Key) -> Option<Shortcut> {
    Some(Shortcut { key, shift: false })
}

const fn shifted(key: Key) -> Option<Shortcut> {
    Some(Shortcut { key, shift: true })
}

impl MenuAction {
    pub fn label(self) -> &'static str {
        match self {
            MenuAction::ChooseFolder => tr_str("Scegli cartella…", "Choose Folder…"),
            MenuAction::Scan => tr_str("Analizza libreria", "Scan Library"),
            MenuAction::Export => tr_str("Esporta da convertire…", "Export To Convert…"),
            MenuAction::ConvertSelected => tr_str("Converti selezionati…", "Convert Selected…"),
            MenuAction::SelectAll => {
                tr_str("Seleziona tutti da convertire", "Select All To Convert")
            }
            MenuAction::ClearSelection => tr_str("Deseleziona", "Clear Selection"),
            MenuAction::FocusSearch => tr_str("Cerca nella libreria", "Search the Library"),
            MenuAction::FilterAll => tr_str("Tutti i video", "All Videos"),
            MenuAction::FilterConvert => tr_str("Da convertire", "To Convert"),
            MenuAction::FilterCompatible => tr_str("Compatibili", "Compatible"),
            MenuAction::FilterErrors => tr_str("Errori", "Errors"),
            MenuAction::Settings => tr_str("Impostazioni…", "Settings…"),
            MenuAction::CheckUpdates => tr_str("Cerca aggiornamenti…", "Check for Updates…"),
            MenuAction::About => tr_str("Informazioni su Fotogramma", "About Fotogramma"),
            MenuAction::Minimize => tr_str("Riduci a icona", "Minimize"),
            MenuAction::ToggleMaximize => tr_str("Ingrandisci", "Zoom"),
            MenuAction::Quit => tr_str("Esci da Fotogramma", "Quit Fotogramma"),
        }
    }

    pub fn shortcut(self) -> Option<Shortcut> {
        match self {
            MenuAction::ChooseFolder => plain(Key::O),
            MenuAction::Scan => plain(Key::R),
            MenuAction::Export => plain(Key::E),
            MenuAction::ConvertSelected => plain(Key::Enter),
            MenuAction::SelectAll => shifted(Key::A),
            MenuAction::ClearSelection => shifted(Key::D),
            MenuAction::FocusSearch => plain(Key::F),
            MenuAction::FilterAll => plain(Key::Num1),
            MenuAction::FilterConvert => plain(Key::Num2),
            MenuAction::FilterCompatible => plain(Key::Num3),
            MenuAction::FilterErrors => plain(Key::Num4),
            MenuAction::Settings => plain(Key::Comma),
            MenuAction::Minimize => plain(Key::M),
            MenuAction::Quit => plain(Key::Q),
            MenuAction::CheckUpdates | MenuAction::About | MenuAction::ToggleMaximize => None,
        }
    }

    /// A stable text for the native menu to hand back when an item is chosen.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn id(self) -> String {
        format!("{self:?}")
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn from_id(id: &str) -> Option<MenuAction> {
        ALL_ACTIONS.into_iter().find(|action| action.id() == id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Action(MenuAction),
    Separator,
}

pub struct Menu {
    pub title: &'static str,
    pub entries: Vec<Entry>,
}

use Entry::{Action, Separator};

/// The menus, in order. On macOS Settings, About and Quit live in the
/// application menu (`app_entries`); elsewhere they are in the menus below.
pub fn menus(native_app_menu: bool) -> Vec<Menu> {
    let mut file = vec![
        Action(MenuAction::ChooseFolder),
        Action(MenuAction::Scan),
        Separator,
        Action(MenuAction::Export),
        Action(MenuAction::ConvertSelected),
    ];
    let mut edit = vec![
        Action(MenuAction::SelectAll),
        Action(MenuAction::ClearSelection),
        Separator,
        Action(MenuAction::FocusSearch),
    ];
    // On macOS Check for Updates is in the application menu and Help is left
    // to the system, which adds its search field.
    let mut help = Vec::new();
    if !native_app_menu {
        file.push(Separator);
        file.push(Action(MenuAction::Quit));
        edit.push(Separator);
        edit.push(Action(MenuAction::Settings));
        help.push(Action(MenuAction::CheckUpdates));
        help.push(Separator);
        help.push(Action(MenuAction::About));
    }
    vec![
        Menu {
            title: tr_str("File", "File"),
            entries: file,
        },
        Menu {
            title: tr_str("Modifica", "Edit"),
            entries: edit,
        },
        Menu {
            title: tr_str("Vista", "View"),
            entries: vec![
                Action(MenuAction::FilterAll),
                Action(MenuAction::FilterConvert),
                Action(MenuAction::FilterCompatible),
                Action(MenuAction::FilterErrors),
            ],
        },
        Menu {
            title: tr_str("Finestra", "Window"),
            entries: vec![
                Action(MenuAction::Minimize),
                Action(MenuAction::ToggleMaximize),
            ],
        },
        Menu {
            title: tr_str("Aiuto", "Help"),
            entries: help,
        },
    ]
}

/// The macOS application menu, between the app name and "File".
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn app_entries() -> Vec<Entry> {
    vec![
        Action(MenuAction::About),
        Separator,
        Action(MenuAction::Settings),
        Action(MenuAction::CheckUpdates),
        Separator,
        Action(MenuAction::Quit),
    ]
}

/// What the application can do right now, so that entries can be disabled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuState {
    /// A scan or conversion is not running and no window is open.
    pub idle: bool,
    /// The results page is showing.
    pub results: bool,
    /// There are videos left to convert.
    pub can_export: bool,
    /// Videos to convert are selected.
    pub can_convert: bool,
}

impl MenuState {
    pub fn enabled(&self, action: MenuAction) -> bool {
        match action {
            MenuAction::ChooseFolder | MenuAction::Scan => self.idle,
            MenuAction::Export => self.idle && self.can_export,
            MenuAction::ConvertSelected => self.idle && self.can_convert,
            MenuAction::SelectAll
            | MenuAction::ClearSelection
            | MenuAction::FocusSearch
            | MenuAction::FilterAll
            | MenuAction::FilterConvert
            | MenuAction::FilterCompatible
            | MenuAction::FilterErrors => self.idle && self.results,
            MenuAction::Settings | MenuAction::CheckUpdates => self.idle,
            MenuAction::About
            | MenuAction::Minimize
            | MenuAction::ToggleMaximize
            | MenuAction::Quit => true,
        }
    }
}

/// The in-window menu bar (Windows and Linux). Returns what was chosen.
pub fn show_bar(ui: &mut egui::Ui, state: &MenuState) -> Option<MenuAction> {
    let mut chosen = None;
    egui::MenuBar::new().ui(ui, |ui| {
        for menu in menus(false) {
            ui.menu_button(menu.title, |ui| {
                for entry in &menu.entries {
                    match entry {
                        Separator => {
                            ui.separator();
                        }
                        Action(action) => {
                            let mut button = egui::Button::new(action.label());
                            if let Some(shortcut) = action.shortcut() {
                                button = button.shortcut_text(shortcut.text());
                            }
                            if ui.add_enabled(state.enabled(*action), button).clicked() {
                                chosen = Some(*action);
                                ui.close();
                            }
                        }
                    }
                }
            });
        }
    });
    chosen
}

/// A shortcut pressed in the window (Windows and Linux; on macOS the native
/// menu takes the key before the window sees it).
pub fn shortcut_pressed(ctx: &egui::Context) -> Option<MenuAction> {
    ALL_ACTIONS.into_iter().find(|action| {
        action
            .shortcut()
            .is_some_and(|shortcut| ctx.input_mut(|input| input.consume_shortcut(&shortcut.egui())))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_action_has_a_label() {
        for action in ALL_ACTIONS {
            assert!(!action.label().is_empty(), "{action:?}");
        }
    }

    fn press(modifiers: Modifiers, key: Key) -> Option<MenuAction> {
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        ctx.begin_pass(raw);
        let found = shortcut_pressed(&ctx);
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
        found
    }

    #[test]
    fn a_pressed_shortcut_finds_its_action() {
        assert_eq!(
            press(Modifiers::COMMAND, Key::Comma),
            Some(MenuAction::Settings)
        );
        assert_eq!(
            press(Modifiers::COMMAND, Key::O),
            Some(MenuAction::ChooseFolder)
        );
        assert_eq!(
            press(Modifiers::COMMAND, Key::Num3),
            Some(MenuAction::FilterCompatible)
        );
        assert_eq!(
            press(Modifiers::COMMAND | Modifiers::SHIFT, Key::A),
            Some(MenuAction::SelectAll)
        );
    }

    #[test]
    fn keys_that_are_not_shortcuts_find_nothing() {
        assert_eq!(
            press(Modifiers::NONE, Key::O),
            None,
            "a plain letter is typing"
        );
        assert_eq!(
            press(Modifiers::COMMAND, Key::A),
            None,
            "Cmd+A belongs to the text field"
        );
        assert_eq!(press(Modifiers::COMMAND, Key::C), None);
        assert_eq!(press(Modifiers::COMMAND, Key::V), None);
        assert_eq!(press(Modifiers::ALT, Key::O), None);
    }

    #[test]
    fn shortcuts_are_unique() {
        let mut seen = HashSet::new();
        for action in ALL_ACTIONS {
            if let Some(shortcut) = action.shortcut() {
                assert!(
                    seen.insert((format!("{:?}", shortcut.key), shortcut.shift)),
                    "{action:?} repeats a shortcut"
                );
            }
        }
    }

    #[test]
    fn shortcuts_never_take_the_text_editing_keys() {
        // Cmd+A, Cmd+C, Cmd+V, Cmd+X, Cmd+Z belong to the text fields.
        for action in ALL_ACTIONS {
            if let Some(shortcut) = action.shortcut() {
                if !shortcut.shift {
                    assert!(
                        !matches!(shortcut.key, Key::A | Key::C | Key::V | Key::X | Key::Z),
                        "{action:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn ids_round_trip() {
        for action in ALL_ACTIONS {
            assert_eq!(MenuAction::from_id(&action.id()), Some(action));
        }
        assert_eq!(MenuAction::from_id("nonsense"), None);
    }

    #[test]
    fn every_action_is_in_exactly_one_place_in_each_layout() {
        for native in [true, false] {
            let mut count = std::collections::HashMap::new();
            let all = menus(native)
                .into_iter()
                .flat_map(|menu| menu.entries)
                .chain(if native { app_entries() } else { vec![] });
            for entry in all {
                if let Entry::Action(action) = entry {
                    *count.entry(action).or_insert(0) += 1;
                }
            }
            for action in ALL_ACTIONS {
                assert_eq!(
                    count.get(&action),
                    Some(&1),
                    "{action:?} (native: {native})"
                );
            }
        }
    }

    #[test]
    fn the_text_of_a_shortcut_reads_like_a_normal_app() {
        assert_eq!(
            MenuAction::ChooseFolder.shortcut().unwrap().text(),
            "Ctrl+O"
        );
        assert_eq!(
            MenuAction::SelectAll.shortcut().unwrap().text(),
            "Ctrl+Shift+A"
        );
        assert_eq!(MenuAction::Settings.shortcut().unwrap().text(), "Ctrl+,");
        assert_eq!(
            MenuAction::SelectAll.shortcut().unwrap().accelerator(),
            "CmdOrCtrl+Shift+A"
        );
        assert_eq!(
            MenuAction::FilterConvert.shortcut().unwrap().accelerator(),
            "CmdOrCtrl+2"
        );
        assert_eq!(
            MenuAction::ConvertSelected
                .shortcut()
                .unwrap()
                .accelerator(),
            "CmdOrCtrl+Enter"
        );
    }

    #[test]
    fn entries_are_enabled_only_when_they_can_work() {
        let busy = MenuState::default();
        assert!(!busy.enabled(MenuAction::Scan));
        assert!(!busy.enabled(MenuAction::Settings));
        assert!(
            busy.enabled(MenuAction::Quit),
            "quitting is always possible"
        );
        assert!(busy.enabled(MenuAction::About));

        let welcome = MenuState {
            idle: true,
            ..MenuState::default()
        };
        assert!(welcome.enabled(MenuAction::Scan));
        assert!(
            !welcome.enabled(MenuAction::FilterConvert),
            "no results yet"
        );
        assert!(!welcome.enabled(MenuAction::Export));

        let results = MenuState {
            idle: true,
            results: true,
            can_export: true,
            can_convert: false,
        };
        assert!(results.enabled(MenuAction::FilterConvert));
        assert!(results.enabled(MenuAction::Export));
        assert!(
            !results.enabled(MenuAction::ConvertSelected),
            "nothing selected"
        );
        let selected = MenuState {
            can_convert: true,
            ..results
        };
        assert!(selected.enabled(MenuAction::ConvertSelected));
    }
}
