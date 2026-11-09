//! The native menu bar on macOS, built with `muda` from the same definition as
//! the in-window menu (`menu.rs`).
//!
//! Edit has no Cut/Copy/Paste: a native item would grab Cmd+C and Cmd+V before
//! the text fields see them. Those keys keep working in the fields.

use crate::menu::{self, Entry, MenuAction, MenuState, ALL_ACTIONS};
use eframe::egui;
use muda::accelerator::Accelerator;
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;
use std::sync::Mutex;

/// Actions chosen in the menu bar, waiting for the next frame.
static PENDING: Mutex<Vec<MenuAction>> = Mutex::new(Vec::new());

pub struct NativeMenu {
    _menu: Menu,
    items: HashMap<MenuAction, MenuItem>,
    enabled: HashMap<MenuAction, bool>,
}

impl NativeMenu {
    /// Replace the menu bar of the application. Call it on the main thread,
    /// and again after the language changed.
    pub fn install(ctx: &egui::Context) -> Self {
        let handler_ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(action) = MenuAction::from_id(&event.id.0) {
                PENDING
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(action);
                handler_ctx.request_repaint();
            }
        }));

        let menu = Menu::new();
        let mut items: HashMap<MenuAction, MenuItem> = HashMap::new();
        let mut item_for = |action: MenuAction| -> MenuItem {
            let accelerator = action
                .shortcut()
                .and_then(|shortcut| shortcut.accelerator().parse::<Accelerator>().ok());
            let item = MenuItem::with_id(action.id(), action.label(), true, accelerator);
            items.insert(action, item.clone());
            item
        };

        // The application menu: macOS shows the name of the app as its title.
        let app = Submenu::new("Fotogramma", true);
        for entry in menu::app_entries() {
            match entry {
                Entry::Action(action) => {
                    let _ = app.append(&item_for(action));
                }
                Entry::Separator => {
                    let _ = app.append(&PredefinedMenuItem::separator());
                }
            }
            // Services and Hide sit between Check for Updates and Quit.
            if entry == Entry::Action(MenuAction::CheckUpdates) {
                let _ = app.append(&PredefinedMenuItem::separator());
                let _ = app.append(&PredefinedMenuItem::services(None));
                let _ = app.append(&PredefinedMenuItem::separator());
                let _ = app.append(&PredefinedMenuItem::hide(None));
                let _ = app.append(&PredefinedMenuItem::hide_others(None));
                let _ = app.append(&PredefinedMenuItem::show_all(None));
            }
        }
        let _ = menu.append(&app);

        let mut window_menu = None;
        let mut help_menu = None;
        let definitions = menu::menus(true);
        let last = definitions.len() - 1;
        for (index, definition) in definitions.into_iter().enumerate() {
            let submenu = Submenu::new(definition.title, true);
            for entry in &definition.entries {
                match entry {
                    Entry::Action(action) => {
                        let _ = submenu.append(&item_for(*action));
                    }
                    Entry::Separator => {
                        let _ = submenu.append(&PredefinedMenuItem::separator());
                    }
                }
            }
            let _ = menu.append(&submenu);
            if definition
                .entries
                .contains(&Entry::Action(MenuAction::Minimize))
            {
                window_menu = Some(submenu.clone());
            } else if index == last {
                help_menu = Some(submenu.clone());
            }
        }
        menu.init_for_nsapp();
        if let Some(window) = window_menu {
            window.set_as_windows_menu_for_nsapp();
        }
        if let Some(help) = help_menu {
            help.set_as_help_menu_for_nsapp();
        }

        Self {
            _menu: menu,
            items,
            enabled: HashMap::new(),
        }
    }

    /// Grey out what cannot work right now. Cheap: it only touches what changed.
    pub fn update(&mut self, state: &MenuState) {
        for action in ALL_ACTIONS {
            let wanted = state.enabled(action);
            if self.enabled.get(&action) != Some(&wanted) {
                if let Some(item) = self.items.get(&action) {
                    item.set_enabled(wanted);
                }
                self.enabled.insert(action, wanted);
            }
        }
    }

    /// What was chosen since the last frame.
    pub fn take_actions() -> Vec<MenuAction> {
        std::mem::take(&mut *PENDING.lock().unwrap_or_else(|error| error.into_inner()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shortcut_is_a_valid_accelerator() {
        for action in ALL_ACTIONS {
            if let Some(shortcut) = action.shortcut() {
                let text = shortcut.accelerator();
                assert!(
                    text.parse::<Accelerator>().is_ok(),
                    "{action:?}: muda cannot read “{text}”"
                );
            }
        }
    }
}
