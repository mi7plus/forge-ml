//! Command dispatch and keyboard entry points: command→keymap mapping,
//! command execution, the remote-input and command-palette windows, and global
//! accessibility shortcuts. Methods on the shared [`crate::ForgeApp`].

use crate::ui::theme::*;
use crate::*;
use eframe::egui;
use egui::RichText;

impl crate::ForgeApp {
    /// The customizable keymap action a palette command maps to, if any, so the
    /// palette can show each command's live shortcut.
    fn command_key_action(command: commands::Command) -> Option<keymap::KeyAction> {
        use commands::Command as C;
        use keymap::KeyAction as K;
        Some(match command {
            C::Save => K::Save,
            C::NewFile => K::NewFile,
            C::Find => K::FindInFile,
            C::FindProject => K::FindInProject,
            C::FormatDocument => K::FormatDocument,
            C::RunCell => K::RunCell,
            C::RunAll => K::RunAll,
            C::FindReferences => K::FindReferences,
            C::NewTerminal => K::NewTerminal,
            C::Stop => K::StopExecution,
            C::Settings => K::OpenSettings,
            _ => return None,
        })
    }

    pub(crate) fn execute_command(&mut self, command: commands::Command) {
        use commands::Command::*;
        // Track for the palette's recent list (newest first, capped).
        self.recent_commands.retain(|c| *c != command);
        self.recent_commands.insert(0, command);
        self.recent_commands.truncate(8);
        match command {
            NewFile => self.create_new_file(None),
            Save => self.save_active(),
            RunCell => self.enqueue_cells([self.selected_cell]),
            RunAll => self.enqueue_cells(0..self.cells().len()),
            Stop => self.stop_execution(),
            Find => self.find_visible = true,
            FindProject => self.inspector_tab = InspectorTab::Search,
            FindReferences => self.request_lsp("references"),
            RenameSymbol => {
                self.rename_open = true;
                self.rename_input.clear();
            }
            CodeActions => self.request_lsp("codeactions"),
            FormatDocument => self.format_document(),
            Clippy => self.run_clippy(),
            CargoBuild => self.run_cargo_task("build"),
            CargoTest => self.run_cargo_task("test"),
            CargoRun => self.run_cargo_task("run"),
            NewTerminal => self.pending_new_terminal = Some(None),
            NewKernel => self.pending_new_kernel = Some(None),
            ImportData => self.import_dataset(),
            ToggleTheme => {
                self.dark_mode = !self.dark_mode;
                self.active_theme = None;
                self.theme_dirty = true;
            }
            Settings => self.settings_open = true,
            Variables => self.inspector_tab = InspectorTab::Variables,
            Data => self.inspector_tab = InspectorTab::Data,
            Plots => self.inspector_tab = InspectorTab::Charts,
            Runs => self.inspector_tab = InspectorTab::Experiments,
            Problems => self.inspector_tab = InspectorTab::Problems,
            Git => self.inspector_tab = InspectorTab::Git,
            Packages => self.inspector_tab = InspectorTab::Packages,
            GitHub => self.inspector_tab = InspectorTab::GitHub,
            Studio => self.inspector_tab = InspectorTab::Studio,
            Sql => self.inspector_tab = InspectorTab::Database,
            Deep => self.inspector_tab = InspectorTab::DeepLearning,
            Deploy => self.inspector_tab = InspectorTab::Deploy,
            Storage => self.inspector_tab = InspectorTab::Storage,
        }
        self.status_announcement = format!("Command completed: {:?}", command);
    }

    pub(crate) fn remote_input_window(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.remote_input_prompt.clone() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new("Remote kernel input")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_width(380.0);
                ui.label(prompt);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.remote_input_response)
                        .password(self.remote_input_password)
                        .desired_width(f32::INFINITY)
                        .hint_text(if self.remote_input_password {
                            "Password input"
                        } else {
                            "Reply to remote kernel"
                        }),
                );
                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }
                ui.horizontal(|ui| {
                    submit |= ui.button("Send").clicked();
                    cancel |= ui.button("Cancel execution").clicked();
                });
                ui.small("Input is sent only to the active Jupyter kernel and is not persisted.");
            });
        if submit {
            if self.remote_input_response.len() > 64 * 1024 {
                self.console = "Remote input is limited to 64 KiB.".into();
                return;
            }
            let reply = std::mem::take(&mut self.remote_input_response);
            match self
                .remote_input_sender
                .as_ref()
                .ok_or_else(|| "Remote input channel is no longer available.".to_owned())
                .and_then(|sender| sender.send(reply).map_err(|error| error.to_string()))
            {
                Ok(()) => {
                    self.remote_input_prompt = None;
                    self.remote_input_password = false;
                }
                Err(error) => {
                    self.remote_input_sender = None;
                    self.remote_input_prompt = None;
                    self.remote_input_password = false;
                    self.console = error;
                }
            }
        } else if cancel {
            self.remote_input_sender = None;
            self.remote_input_prompt = None;
            self.remote_input_response.clear();
            self.remote_input_password = false;
            self.stop_execution();
        }
    }

    pub(crate) fn accessibility_shortcuts(&mut self, ctx: &egui::Context) {
        if self.rebinding.is_some() {
            return; // capturing a new binding; don't fire normal shortcuts
        }
        if self
            .keymap
            .triggered(keymap::KeyAction::CommandPalette, ctx)
        {
            self.command_palette_open = true;
            self.command_query.clear();
            self.command_selection = 0;
        }
        if self.keymap.triggered(keymap::KeyAction::CyclePane, ctx) {
            let tabs = InspectorTab::ALL;
            let index = tabs
                .iter()
                .position(|tab| *tab == self.inspector_tab)
                .unwrap_or(0);
            self.inspector_tab = tabs[(index + 1) % tabs.len()];
            self.status_announcement = "Moved to next inspector pane".into();
        }
        use keymap::KeyAction as K;
        if self.keymap.triggered(K::GoToDefinition, ctx) {
            self.request_lsp("definition");
        }
        if self.keymap.triggered(K::FindReferences, ctx) {
            self.request_lsp("references");
        }
        if self.keymap.triggered(K::NewTerminal, ctx) {
            self.pending_new_terminal = Some(None);
        }
        if self.keymap.triggered(K::CloseTab, ctx) {
            self.close_tab(self.active_tab);
        }
        if self.keymap.triggered(K::StopExecution, ctx) {
            self.stop_execution();
        }
        if self.keymap.triggered(K::OpenSettings, ctx) {
            self.settings_open = true;
        }
        // Ctrl+1..=9 jump straight to the first nine inspector panes.
        const NUM_KEYS: [egui::Key; 9] = [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
            egui::Key::Num7,
            egui::Key::Num8,
            egui::Key::Num9,
        ];
        for (index, key) in NUM_KEYS.iter().enumerate() {
            if ctx.input(|i| i.modifiers.command && i.key_pressed(*key)) {
                let tab = InspectorTab::ALL[index];
                self.inspector_tab = tab;
                self.status_announcement = format!("{} pane selected", tab.label());
            }
        }
    }

    pub(crate) fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.command_palette_open {
            return;
        }
        let mut matches = commands::matches(&self.command_query);
        // With an empty query, surface recently-used commands first.
        if self.command_query.trim().is_empty() && !self.recent_commands.is_empty() {
            let mut ordered = Vec::new();
            for recent in &self.recent_commands {
                if let Some(pos) = matches.iter().position(|(c, _, _)| c == recent) {
                    ordered.push(matches.remove(pos));
                }
            }
            ordered.extend(matches);
            matches = ordered;
        }
        if !matches.is_empty() {
            self.command_selection = self.command_selection.min(matches.len() - 1);
        } else {
            self.command_selection = 0;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) && !matches.is_empty() {
            self.command_selection = (self.command_selection + 1) % matches.len();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) && !matches.is_empty() {
            self.command_selection = (self.command_selection + matches.len() - 1) % matches.len();
        }
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let mut chosen = enter
            .then(|| matches.get(self.command_selection).map(|item| item.0))
            .flatten();
        let mut open = self.command_palette_open;
        let title = format!(
            "Command palette — {}",
            self.keymap.display(keymap::KeyAction::CommandPalette)
        );
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.command_query)
                        .hint_text("Type a command or shortcut…")
                        .desired_width(f32::INFINITY),
                );
                response.request_focus();
                let hint =
                    if self.command_query.trim().is_empty() && !self.recent_commands.is_empty() {
                        "Recent commands · ↑/↓ select · Enter run · Esc close"
                    } else {
                        "↑/↓ select · Enter run · Esc close"
                    };
                ui.label(RichText::new(hint).size(10.0).color(MUTED));
                ui.separator();
                for (index, (command, label, shortcut)) in matches.iter().enumerate().take(12) {
                    // Prefer the live keymap binding over the static hint.
                    let binding =
                        Self::command_key_action(*command).map(|a| self.keymap.display(a));
                    let shortcut = binding.as_deref().unwrap_or(shortcut);
                    let selected = index == self.command_selection;
                    let response = ui.selectable_label(selected, {
                        let mut job = egui::text::LayoutJob::default();
                        job.append(label, 0.0, egui::TextFormat::default());
                        if !shortcut.is_empty() {
                            job.append(
                                &format!("    {shortcut}"),
                                0.0,
                                egui::TextFormat {
                                    color: MUTED,
                                    ..Default::default()
                                },
                            );
                        }
                        job
                    });
                    if response.clicked() {
                        chosen = Some(*command);
                    }
                }
            });
        self.command_palette_open = open && !escape;
        if let Some(command) = chosen {
            self.command_palette_open = false;
            self.execute_command(command);
            self.apply_theme(ctx);
        }
    }
}
