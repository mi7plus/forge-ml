//! User-customizable keyboard shortcuts.
//!
//! A single [`Keymap`] owns the binding for every rebindable [`KeyAction`], so
//! shortcut handling lives in one place instead of being scattered as hard-coded
//! `ui.input(...)` checks. Bindings are edited in Settings and persisted as a
//! small serializable DTO, defaulting to the built-in bindings.

use eframe::egui;
use egui::{Key, KeyboardShortcut, Modifiers};
use serde::{Deserialize, Serialize};

/// A command that can be triggered by a customizable shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyAction {
    Save,
    NewFile,
    FindInFile,
    FindInProject,
    FormatDocument,
    RunCell,
    RunAll,
    RequestCompletion,
    CommandPalette,
    CyclePane,
    GoToDefinition,
    FindReferences,
    NewTerminal,
    CloseTab,
    StopExecution,
    OpenSettings,
}

impl KeyAction {
    pub const ALL: [KeyAction; 16] = [
        KeyAction::Save,
        KeyAction::NewFile,
        KeyAction::FindInFile,
        KeyAction::FindInProject,
        KeyAction::FormatDocument,
        KeyAction::RunCell,
        KeyAction::RunAll,
        KeyAction::RequestCompletion,
        KeyAction::CommandPalette,
        KeyAction::CyclePane,
        KeyAction::GoToDefinition,
        KeyAction::FindReferences,
        KeyAction::NewTerminal,
        KeyAction::CloseTab,
        KeyAction::StopExecution,
        KeyAction::OpenSettings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            KeyAction::Save => "Save file",
            KeyAction::NewFile => "New file",
            KeyAction::FindInFile => "Find in file",
            KeyAction::FindInProject => "Find in project",
            KeyAction::FormatDocument => "Format document",
            KeyAction::RunCell => "Run current cell",
            KeyAction::RunAll => "Run all cells",
            KeyAction::RequestCompletion => "Request completion",
            KeyAction::CommandPalette => "Command palette",
            KeyAction::CyclePane => "Cycle inspector panes",
            KeyAction::GoToDefinition => "Go to definition",
            KeyAction::FindReferences => "Find references",
            KeyAction::NewTerminal => "New terminal",
            KeyAction::CloseTab => "Close editor tab",
            KeyAction::StopExecution => "Stop execution",
            KeyAction::OpenSettings => "Open settings",
        }
    }

    fn id(self) -> &'static str {
        match self {
            KeyAction::Save => "save",
            KeyAction::NewFile => "new_file",
            KeyAction::FindInFile => "find_in_file",
            KeyAction::FindInProject => "find_in_project",
            KeyAction::FormatDocument => "format_document",
            KeyAction::RunCell => "run_cell",
            KeyAction::RunAll => "run_all",
            KeyAction::RequestCompletion => "request_completion",
            KeyAction::CommandPalette => "command_palette",
            KeyAction::CyclePane => "cycle_pane",
            KeyAction::GoToDefinition => "go_to_definition",
            KeyAction::FindReferences => "find_references",
            KeyAction::NewTerminal => "new_terminal",
            KeyAction::CloseTab => "close_tab",
            KeyAction::StopExecution => "stop_execution",
            KeyAction::OpenSettings => "open_settings",
        }
    }

    fn from_id(id: &str) -> Option<KeyAction> {
        KeyAction::ALL.into_iter().find(|a| a.id() == id)
    }

    fn default_shortcut(self) -> KeyboardShortcut {
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND.plus(Modifiers::SHIFT);
        let shift = Modifiers::SHIFT;
        let alt_shift = Modifiers::ALT.plus(Modifiers::SHIFT);
        let none = Modifiers::NONE;
        let (mods, key) = match self {
            KeyAction::Save => (cmd, Key::S),
            KeyAction::NewFile => (cmd, Key::N),
            KeyAction::FindInFile => (cmd, Key::F),
            KeyAction::FindInProject => (cmd_shift, Key::F),
            KeyAction::FormatDocument => (alt_shift, Key::F),
            KeyAction::RunCell => (shift, Key::Enter),
            KeyAction::RunAll => (cmd_shift, Key::Enter),
            KeyAction::RequestCompletion => (cmd, Key::Space),
            KeyAction::CommandPalette => (cmd_shift, Key::P),
            KeyAction::CyclePane => (none, Key::F6),
            KeyAction::GoToDefinition => (none, Key::F12),
            KeyAction::FindReferences => (shift, Key::F12),
            KeyAction::NewTerminal => (cmd, Key::Backtick),
            KeyAction::CloseTab => (cmd, Key::W),
            KeyAction::StopExecution => (cmd, Key::Period),
            KeyAction::OpenSettings => (cmd, Key::Comma),
        };
        KeyboardShortcut::new(mods, key)
    }
}

/// A serializable binding override for one action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChordDto {
    action: String,
    command: bool,
    shift: bool,
    alt: bool,
    key: String,
}

/// The active set of shortcut bindings.
#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<(KeyAction, KeyboardShortcut)>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: KeyAction::ALL
                .into_iter()
                .map(|a| (a, a.default_shortcut()))
                .collect(),
        }
    }
}

impl Keymap {
    pub fn shortcut(&self, action: KeyAction) -> Option<KeyboardShortcut> {
        self.bindings
            .iter()
            .find(|(a, _)| *a == action)
            .map(|(_, sc)| *sc)
    }

    pub fn set(&mut self, action: KeyAction, shortcut: KeyboardShortcut) {
        if let Some(slot) = self.bindings.iter_mut().find(|(a, _)| *a == action) {
            slot.1 = shortcut;
        } else {
            self.bindings.push((action, shortcut));
        }
    }

    pub fn reset(&mut self, action: KeyAction) {
        self.set(action, action.default_shortcut());
    }

    pub fn reset_all(&mut self) {
        *self = Keymap::default();
    }

    /// Another action already bound to the same chord, if any.
    pub fn conflict(&self, shortcut: KeyboardShortcut, except: KeyAction) -> Option<KeyAction> {
        self.bindings.iter().find_map(|(a, sc)| {
            (*a != except && sc.modifiers == shortcut.modifiers && sc.logical_key == shortcut.logical_key)
                .then_some(*a)
        })
    }

    /// Human-readable chord, e.g. `Ctrl+Shift+P`.
    pub fn display(&self, action: KeyAction) -> String {
        self.shortcut(action)
            .map(|sc| sc.format(&egui::ModifierNames::NAMES, cfg!(target_os = "macos")))
            .unwrap_or_else(|| "unset".to_owned())
    }

    /// True if the action's shortcut was pressed this frame; consumes the event.
    pub fn triggered(&self, action: KeyAction, ctx: &egui::Context) -> bool {
        match self.shortcut(action) {
            Some(sc) => ctx.input_mut(|input| input.consume_shortcut(&sc)),
            None => false,
        }
    }

    pub fn to_dto(&self) -> Vec<ChordDto> {
        self.bindings
            .iter()
            .map(|(action, sc)| ChordDto {
                action: action.id().to_owned(),
                command: sc.modifiers.command || sc.modifiers.ctrl || sc.modifiers.mac_cmd,
                shift: sc.modifiers.shift,
                alt: sc.modifiers.alt,
                key: sc.logical_key.name().to_owned(),
            })
            .collect()
    }

    /// Start from the defaults and apply any saved overrides that still parse.
    pub fn from_dto(dtos: &[ChordDto]) -> Self {
        let mut map = Keymap::default();
        for dto in dtos {
            if let (Some(action), Some(key)) =
                (KeyAction::from_id(&dto.action), Key::from_name(&dto.key))
            {
                let mut mods = Modifiers::NONE;
                mods.command = dto.command;
                mods.shift = dto.shift;
                mods.alt = dto.alt;
                map.set(action, KeyboardShortcut::new(mods, key));
            }
        }
        map
    }
}

/// Build a shortcut from the currently-pressed keys during a rebind capture.
/// Returns the first non-modifier key pressed, with its modifiers.
pub fn capture(ctx: &egui::Context) -> Option<KeyboardShortcut> {
    ctx.input(|input| {
        let m = input.modifiers;
        for event in &input.events {
            if let egui::Event::Key {
                key,
                pressed: true,
                ..
            } = event
            {
                let mut mods = Modifiers::NONE;
                mods.command = m.command || m.ctrl || m.mac_cmd;
                mods.shift = m.shift;
                mods.alt = m.alt;
                return Some(KeyboardShortcut::new(mods, *key));
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_every_action() {
        let map = Keymap::default();
        for action in KeyAction::ALL {
            assert!(map.shortcut(action).is_some(), "{action:?} unbound");
        }
    }

    #[test]
    fn rebind_conflict_and_reset() {
        let mut map = Keymap::default();
        let save = map.shortcut(KeyAction::Save).unwrap();
        // Binding NewFile to Save's chord is a conflict with Save.
        assert_eq!(map.conflict(save, KeyAction::NewFile), Some(KeyAction::Save));
        // Rebinding then resetting restores the default.
        let custom = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::J);
        map.set(KeyAction::Save, custom);
        assert_eq!(map.shortcut(KeyAction::Save), Some(custom));
        map.reset(KeyAction::Save);
        assert_eq!(map.shortcut(KeyAction::Save), Some(save));
    }

    #[test]
    fn dto_round_trips_overrides() {
        let mut map = Keymap::default();
        let custom = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::K);
        map.set(KeyAction::CommandPalette, custom);
        let restored = Keymap::from_dto(&map.to_dto());
        assert_eq!(restored.shortcut(KeyAction::CommandPalette), Some(custom));
        // Untouched actions keep their defaults.
        assert_eq!(
            restored.shortcut(KeyAction::Save),
            Some(KeyAction::Save.default_shortcut())
        );
    }

    #[test]
    fn from_dto_ignores_unknown_entries() {
        let bogus = ChordDto {
            action: "does_not_exist".into(),
            command: true,
            shift: false,
            alt: false,
            key: "Z".into(),
        };
        // Falls back to defaults without panicking.
        let map = Keymap::from_dto(&[bogus]);
        assert_eq!(map.shortcut(KeyAction::Save), Some(KeyAction::Save.default_shortcut()));
    }
}
