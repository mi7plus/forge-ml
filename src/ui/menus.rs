//! Menu bar, top toolbar, and the modal windows (settings, welcome, rename,
//! code actions). Methods on the shared [`crate::ForgeApp`].

use crate::ui::theme::*;
use crate::*;
use eframe::egui;
use egui::RichText;

impl crate::ForgeApp {
    pub(crate) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        // MenuBar lays the top menus out horizontally, but egui 0.36 only opens a
        // top-level menu on click. `top!` adds the familiar menu-bar behavior:
        // once any menu is open, moving the pointer onto a sibling opens it
        // (egui keeps a single popup open, so this switches without a click).
        egui::MenuBar::new().ui(ui, |ui| {
            macro_rules! top {
                ($label:expr, |$ui:ident| $body:block) => {{
                    let resp = ui.menu_button($label, |$ui| $body).response;
                    let pid = resp.id.with("popup");
                    let ctx = resp.ctx.clone();
                    if resp.contains_pointer()
                        && egui::Popup::is_any_open(&ctx)
                        && !egui::Popup::is_id_open(&ctx, pid)
                    {
                        egui::Popup::open_id(&ctx, pid);
                    }
                }};
            }
            ui.label(RichText::new("FORGE ML").strong().color(RED));
            ui.separator();
            top!("File", |ui| {
                if ui.button("New file...   Ctrl+N").clicked() {
                    self.create_new_file(None);
                    ui.close();
                }
                if ui.button("Open project...").clicked() {
                    self.open_project();
                    ui.close();
                }
                ui.separator();
                if ui
                    .button("Save workspace as...")
                    .on_hover_text(
                        "Save the root folder, open files, dock layout, theme, keymap, and \
                         connections to a shareable file",
                    )
                    .clicked()
                {
                    self.save_workspace_as();
                    ui.close();
                }
                if ui
                    .button("Open workspace...")
                    .on_hover_text("Load a saved workspace file")
                    .clicked()
                {
                    self.open_workspace(ui.ctx());
                    ui.close();
                }
                ui.separator();
                if ui.button("Import Jupyter notebook...").clicked() {
                    self.import_ipynb();
                    ui.close();
                }
                if ui.button("Export as .ipynb...").clicked() {
                    self.export_ipynb();
                    ui.close();
                }
                if ui.button("Export notebook as Markdown...").clicked() {
                    self.export_notebook_document("md");
                    ui.close();
                }
                if ui.button("Export notebook as HTML...").clicked() {
                    self.export_notebook_document("html");
                    ui.close();
                }
                if ui.button("Export reproducible project bundle...").clicked() {
                    self.export_project_bundle();
                    ui.close();
                }
                let recent = self.recent_projects.clone();
                ui.menu_button("Open recent", |ui| {
                    if recent.is_empty() {
                        ui.label("No recent projects");
                    }
                    for path in recent {
                        if ui.button(path.display().to_string()).clicked() {
                            self.request_open_project_path(path);
                            ui.close();
                        }
                    }
                    if !self.recent_projects.is_empty() {
                        ui.separator();
                        if ui.button("Clear recent projects").clicked() {
                            self.recent_projects.clear();
                            ui.close();
                        }
                    }
                });
                if ui.button("Save   Ctrl+S").clicked() {
                    self.save_active();
                    ui.close();
                }
                if ui.button("Close editor tab").clicked() {
                    self.close_tab(self.active_tab);
                    ui.close();
                }
            });
            top!("Edit", |ui| {
                if ui.button("Undo   Ctrl+Z").clicked() {
                    self.pending_editor_history = Some(EditorHistoryCommand::Undo);
                    ui.close();
                }
                if ui.button("Redo   Ctrl+Y / Ctrl+Shift+Z").clicked() {
                    self.pending_editor_history = Some(EditorHistoryCommand::Redo);
                    ui.close();
                }
                ui.separator();
                if ui.button("Format document (rustfmt)").clicked() {
                    self.format_document();
                    ui.close();
                }
            });
            top!("Search", |ui| {
                if ui.button("Find in files   Ctrl+Shift+F").clicked() {
                    self.inspector_tab = InspectorTab::Search;
                    ui.close();
                }
            });
            top!("Source", |ui| {
                if ui.button("Find references").clicked() {
                    self.request_lsp("references");
                    ui.close();
                }
                if ui.button("Rename symbol…").clicked() {
                    self.rename_open = true;
                    self.rename_input.clear();
                    ui.close();
                }
                if ui.button("Code actions / quick fixes").clicked() {
                    self.request_lsp("codeactions");
                    ui.close();
                }
                ui.separator();
                if ui.button("Run code analysis (cargo check)").clicked() {
                    self.run_diagnostics();
                    ui.close();
                }
                if ui.button("Run clippy").clicked() {
                    self.run_clippy();
                    ui.close();
                }
                if ui.button("Format document (rustfmt)").clicked() {
                    self.format_document();
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Cargo", |ui| {
                    for (label, args) in [
                        ("Build", "build"),
                        ("Test", "test"),
                        ("Run", "run"),
                        ("Run (release)", "run --release"),
                        ("Bench", "bench"),
                        ("Clean", "clean"),
                    ] {
                        if ui.button(label).clicked() {
                            self.run_cargo_task(args);
                            ui.close();
                        }
                    }
                });
            });
            top!("Run", |ui| {
                if ui.button("Run cell   Shift+Enter").clicked() {
                    self.enqueue_cells([self.selected_cell]);
                    ui.close();
                }
                if ui.button("Run cells above").clicked() {
                    self.enqueue_cells(0..=self.selected_cell);
                    ui.close();
                }
                if ui.button("Run all   Ctrl+Shift+Enter").clicked() {
                    self.enqueue_cells(0..self.cells().len());
                    ui.close();
                }
                if ui.button("Restart and run all").clicked() {
                    self.restart_and_run_all();
                    ui.close();
                }
                if ui
                    .add_enabled(
                        matches!(self.run_state, RunState::Running(_)),
                        egui::Button::new("Stop execution"),
                    )
                    .clicked()
                {
                    self.stop_execution();
                    ui.close();
                }
            });
            top!("Debug", |ui| {
                if ui.button("Run code analysis (cargo check)").clicked() {
                    self.run_diagnostics();
                    self.inspector_tab = InspectorTab::Problems;
                    ui.close();
                }
                if ui.button("Show Problems pane").clicked() {
                    self.inspector_tab = InspectorTab::Problems;
                    ui.close();
                }
                if ui.button("Inspect variables").clicked() {
                    self.inspector_tab = InspectorTab::Variables;
                    ui.close();
                }
                if ui.button("Restart Rust console").clicked() {
                    let _ = self.runtime.reset();
                    self.run_state = RunState::Booting;
                    ui.close();
                }
                ui.separator();
                ui.label(
                    RichText::new("Step debugging is not available yet.")
                        .size(10.0)
                        .color(MUTED),
                );
            });
            top!("Tools", |ui| {
                if ui
                    .add_enabled(
                        self.integration_pending == 0,
                        egui::Button::new("Import dataset..."),
                    )
                    .clicked()
                {
                    self.import_dataset();
                    ui.close();
                }
                if ui
                    .add_enabled(
                        self.integration_pending == 0,
                        egui::Button::new("Import via Millwright..."),
                    )
                    .clicked()
                {
                    self.import_millwright_dataset();
                    ui.close();
                }
                if ui.button("Git workbench").clicked() {
                    self.inspector_tab = InspectorTab::Git;
                    ui.close();
                }
                if ui.button("Rust packages").clicked() {
                    self.inspector_tab = InspectorTab::Packages;
                    ui.close();
                }
                if ui.button("Discover Jupyter kernels").clicked() {
                    self.discover_jupyter();
                    ui.close();
                }
                if ui.button("Install Evcxr Jupyter kernel").clicked() {
                    self.jupyter_output = jupyter::install_evcxr().unwrap_or_else(|e| e);
                    self.hover_text = self.jupyter_output.clone();
                    self.inspector_tab = InspectorTab::Help;
                    ui.close();
                }
                if ui.button("Settings...").clicked() {
                    self.settings_open = true;
                    ui.close();
                }
                if ui.button("Restart Rust console").clicked() {
                    let _ = self.runtime.reset();
                    self.run_state = RunState::Booting;
                    ui.close();
                }
            });
            top!("View", |ui| {
                let label = if self.dark_mode {
                    "Use light theme"
                } else {
                    "Use dark theme"
                };
                if ui.button(label).clicked() {
                    self.dark_mode = !self.dark_mode;
                    self.active_theme = None;
                    self.apply_theme(ui.ctx());
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Panes", |ui| {
                    ui.label(
                        RichText::new("Show or hide dock panes")
                            .size(10.0)
                            .color(MUTED),
                    );
                    // Fixed panes plus any live terminals and Rust kernels.
                    let mut kinds = expected_panes();
                    if let Some(tree) = self.dock_tree.as_ref() {
                        let mut dynamic: Vec<PaneKind> = tree
                            .tiles
                            .iter()
                            .filter_map(|(_, tile)| match tile {
                                Tile::Pane(k @ (PaneKind::Terminal(_) | PaneKind::RustConsole(_))) => {
                                    Some(*k)
                                }
                                _ => None,
                            })
                            .collect();
                        dynamic.sort_by_key(|k| match k {
                            PaneKind::Terminal(id) => (0, *id),
                            PaneKind::RustConsole(id) => (1, *id),
                            _ => (2, 0),
                        });
                        kinds.extend(dynamic);
                    }
                    for kind in kinds {
                        let Some((mut visible, id)) = self.dock_tree.as_ref().and_then(|tree| {
                            Self::dock_tile_of(tree, kind).map(|id| (tree.tiles.is_visible(id), id))
                        }) else {
                            continue;
                        };
                        let label = match kind {
                            PaneKind::Terminal(n) => format!("Terminal {n}"),
                            PaneKind::RustConsole(n) => format!("Rust {n}"),
                            other => other.title().to_owned(),
                        };
                        if ui.checkbox(&mut visible, label).changed() {
                            if let Some(tree) = self.dock_tree.as_mut() {
                                tree.tiles.set_visible(id, visible);
                            }
                        }
                    }
                });
                if ui.button("New terminal").clicked() {
                    self.pending_new_terminal = Some(None);
                    ui.close();
                }
                if ui.button("New Rust kernel").clicked() {
                    self.pending_new_kernel = Some(None);
                    ui.close();
                }
                if ui.button("Reset layout to default").clicked() {
                    self.dock_tree = Some(build_dock_tree());
                    ui.close();
                }
                ui.label(
                    RichText::new("Drag a pane's tab to split, re-dock, or reorder it.")
                        .size(10.0)
                        .color(MUTED),
                );
            });
            top!("Help", |ui| {
                if ui.button("Welcome / start screen").clicked() {
                    self.welcome_open = true;
                    ui.close();
                }
                ui.separator();
                ui.label(format!(
                    "Forge ML {APP_VERSION} - interactive Rust scientific environment"
                ));
            });
        });
    }

    pub(crate) fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            use egui_phosphor_icons::icons;
            if toolbar_icon_button(ui, icons::FOLDER_OPEN, "Open project").clicked() {
                self.open_project();
            }
            if toolbar_icon_button(ui, icons::FLOPPY_DISK, "Save file (Ctrl+S)").clicked() {
                self.save_active();
            }
            if toolbar_icon_button(
                ui,
                icons::ARROW_COUNTER_CLOCKWISE,
                "Undo editor change (Ctrl+Z)",
            )
            .clicked()
            {
                self.pending_editor_history = Some(EditorHistoryCommand::Undo);
            }
            if toolbar_icon_button(
                ui,
                icons::ARROW_CLOCKWISE,
                "Redo editor change (Ctrl+Y / Ctrl+Shift+Z)",
            )
            .clicked()
            {
                self.pending_editor_history = Some(EditorHistoryCommand::Redo);
            }
            ui.separator();
            if toolbar_icon_button(ui, icons::CHECK_CIRCLE, "Run code analysis").clicked() {
                self.run_diagnostics();
            }
            if toolbar_icon_button(
                ui,
                icons::MAGIC_WAND,
                "Request rust-analyzer completions at the cursor",
            )
            .clicked()
            {
                self.request_lsp("complete");
            }
            if toolbar_icon_button(ui, icons::INFO, "Show type and documentation at the cursor")
                .clicked()
            {
                self.request_lsp("hover");
            }
            if toolbar_icon_button(
                ui,
                icons::ARROW_SQUARE_OUT,
                "Open the definition at the cursor",
            )
            .clicked()
            {
                self.request_lsp("definition");
            }
            ui.separator();
            let ready = !matches!(self.run_state, RunState::Running(_) | RunState::Booting);
            if enabled_toolbar_icon_button(
                ui,
                ready,
                icons::PLAY,
                "Run selected cell (Shift+Enter)",
            )
            .clicked()
            {
                self.enqueue_cells([self.selected_cell]);
            }
            if enabled_toolbar_icon_button(ui, ready, icons::ARROW_LINE_UP, "Run cells above")
                .clicked()
            {
                self.enqueue_cells(0..=self.selected_cell);
            }
            if enabled_toolbar_icon_button(ui, ready, icons::PLAYLIST, "Run all cells").clicked() {
                self.enqueue_cells(0..self.cells().len());
            }
            if toolbar_icon_button(ui, icons::ARROWS_CLOCKWISE, "Reset Rust runtime").clicked() {
                let _ = self.runtime.reset();
                self.run_state = RunState::Booting;
            }
            if enabled_toolbar_icon_button(
                ui,
                matches!(self.run_state, RunState::Running(_)),
                icons::STOP,
                "Stop execution and restart the Rust runtime",
            )
            .clicked()
            {
                self.stop_execution();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(14.0);
                ui.label(
                    RichText::new(
                        self.project
                            .as_ref()
                            .map(|p| p.root.display().to_string())
                            .unwrap_or_else(|| "No project".to_owned()),
                    )
                    .size(11.0)
                    .color(MUTED),
                );
                ui.separator();
                ui.label(
                    RichText::new(format!("Status: {}", self.status_announcement))
                        .size(10.0)
                        .color(MUTED),
                );
            });
        });
    }

    /// The theme builder: pick a theme, edit the seven base colors with live
    /// preview, save/duplicate/delete custom themes, and export/import them.
    fn theme_builder(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.collapsing("Theme builder", |ui| {
            // Theme picker — built-in Dark/Light plus every custom theme.
            let current = self
                .active_theme
                .clone()
                .unwrap_or_else(|| if self.dark_mode { "Dark" } else { "Light" }.to_owned());
            enum Pick {
                Builtin(bool),
                Custom(String),
            }
            let mut pick: Option<Pick> = None;
            egui::ComboBox::from_id_salt("forge_theme_select")
                .selected_text(&current)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.active_theme.is_none() && !self.dark_mode, "Light")
                        .clicked()
                    {
                        pick = Some(Pick::Builtin(false));
                    }
                    if ui
                        .selectable_label(self.active_theme.is_none() && self.dark_mode, "Dark")
                        .clicked()
                    {
                        pick = Some(Pick::Builtin(true));
                    }
                    for theme in &self.custom_themes {
                        let active = self.active_theme.as_deref() == Some(theme.name.as_str());
                        if ui.selectable_label(active, &theme.name).clicked() {
                            pick = Some(Pick::Custom(theme.name.clone()));
                        }
                    }
                });
            if let Some(pick) = pick {
                match pick {
                    Pick::Builtin(dark) => {
                        self.dark_mode = dark;
                        self.active_theme = None;
                    }
                    Pick::Custom(name) => self.active_theme = Some(name),
                }
                self.theme_draft =
                    resolve_palette(&self.active_theme, &self.custom_themes, self.dark_mode);
                self.apply_theme(ui.ctx());
            }

            ui.add_space(6.0);
            // Color slots — edit the draft palette with a live preview.
            let mut changed = false;
            egui::Grid::new("forge_theme_slots")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    for (label, rgb) in self.theme_draft.slots() {
                        ui.label(RichText::new(label).size(11.0));
                        ui.horizontal(|ui| {
                            if egui::color_picker::color_edit_button_srgb(ui, rgb).changed() {
                                changed = true;
                            }
                            ui.label(
                                RichText::new(crate::ui::theme::Palette::to_hex(*rgb))
                                    .monospace()
                                    .size(10.0)
                                    .color(MUTED),
                            );
                        });
                        ui.end_row();
                    }
                });
            if ui
                .checkbox(&mut self.theme_draft.dark, "Dark widget base")
                .on_hover_text("Whether egui's built-in widgets use their dark or light baseline")
                .changed()
            {
                changed = true;
            }
            if changed {
                configure_style(ui.ctx(), &self.theme_draft, self.high_contrast);
                // Keep an active custom theme's stored palette in sync with edits.
                if let Some(name) = self.active_theme.clone() {
                    if let Some(theme) =
                        self.custom_themes.iter_mut().find(|theme| theme.name == name)
                    {
                        theme.palette = self.theme_draft.clone();
                    }
                }
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.theme_new_name)
                        .desired_width(150.0)
                        .hint_text("new theme name"),
                );
                let name = self.theme_new_name.trim().to_owned();
                if ui
                    .add_enabled(!name.is_empty(), egui::Button::new("Save as"))
                    .clicked()
                {
                    self.custom_themes.retain(|theme| theme.name != name);
                    self.custom_themes.push(NamedTheme {
                        name: name.clone(),
                        palette: self.theme_draft.clone(),
                    });
                    self.active_theme = Some(name);
                    self.theme_new_name.clear();
                    self.apply_theme(ui.ctx());
                }
            });
            ui.horizontal(|ui| {
                if ui
                    .button("Reset draft")
                    .on_hover_text("Discard edits and reload the active theme")
                    .clicked()
                {
                    self.theme_draft =
                        resolve_palette(&self.active_theme, &self.custom_themes, self.dark_mode);
                    self.apply_theme(ui.ctx());
                }
                if let Some(name) = self.active_theme.clone() {
                    if ui
                        .button("Delete")
                        .on_hover_text(format!("Delete the '{name}' theme"))
                        .clicked()
                    {
                        self.custom_themes.retain(|theme| theme.name != name);
                        self.active_theme = None;
                        self.theme_draft = resolve_palette(
                            &self.active_theme,
                            &self.custom_themes,
                            self.dark_mode,
                        );
                        self.apply_theme(ui.ctx());
                    }
                }
                if ui.button("Export…").clicked() {
                    self.export_theme();
                }
                if ui.button("Import…").clicked() {
                    self.import_theme(ui.ctx());
                }
            });
        });
    }

    /// Write the current draft palette to a `.json` theme file.
    fn export_theme(&mut self) {
        let name = self
            .active_theme
            .clone()
            .unwrap_or_else(|| if self.dark_mode { "dark" } else { "light" }.to_owned());
        let theme = NamedTheme {
            name: name.clone(),
            palette: self.theme_draft.clone(),
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Forge theme", &["json"])
            .set_file_name(format!("{name}.forge-theme.json"))
            .save_file()
        else {
            return;
        };
        self.console = match serde_json::to_string_pretty(&theme)
            .map_err(|e| e.to_string())
            .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()))
        {
            Ok(()) => format!("Exported theme to {}", path.display()),
            Err(error) => format!("Could not export theme: {error}"),
        };
    }

    /// Load a `.json` theme file, add it to the custom themes, and activate it.
    fn import_theme(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Forge theme", &["json"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str::<NamedTheme>(&text).map_err(|e| e.to_string()))
        {
            Ok(theme) => {
                let name = theme.name.clone();
                self.custom_themes.retain(|existing| existing.name != name);
                self.theme_draft = theme.palette.clone();
                self.custom_themes.push(theme);
                self.active_theme = Some(name);
                self.apply_theme(ctx);
                self.console = "Imported theme.".to_owned();
            }
            Err(error) => self.console = format!("Could not import theme: {error}"),
        }
    }

    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        egui::Window::new("Forge ML settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.heading("Appearance");
                let mut dark = self.dark_mode;
                ui.horizontal(|ui| {
                    ui.label("Color theme");
                    ui.selectable_value(&mut dark, false, "Light");
                    ui.selectable_value(&mut dark, true, "Dark");
                });
                if dark != self.dark_mode {
                    self.dark_mode = dark;
                    self.active_theme = None;
                    self.theme_draft =
                        resolve_palette(&self.active_theme, &self.custom_themes, self.dark_mode);
                    self.apply_theme(ctx);
                }
                self.theme_builder(ui);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Editor font size");
                    ui.add(
                        egui::Slider::new(&mut self.editor_font_size, 10.0..=24.0)
                            .suffix(" px")
                            .step_by(1.0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Interface scale")
                        .on_hover_text("Scale all text and controls across the whole IDE");
                    let changed = ui
                        .add(
                            egui::Slider::new(&mut self.ui_scale, 0.8..=1.6)
                                .step_by(0.05)
                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                        )
                        .changed();
                    if ui.small_button("Reset").clicked() {
                        self.ui_scale = 1.0;
                        ctx.set_zoom_factor(self.ui_scale);
                    } else if changed {
                        ctx.set_zoom_factor(self.ui_scale);
                    }
                });
                ui.checkbox(&mut self.caret_blink, "Blink editor caret");
                ui.checkbox(&mut self.format_on_save, "Format Rust files with rustfmt on save")
                    .on_hover_text("Requires rustfmt on PATH");
                ui.heading("Accessibility");
                let contrast_changed = ui
                    .checkbox(&mut self.high_contrast, "High-contrast interface")
                    .changed();
                ui.checkbox(
                    &mut self.reduced_motion,
                    "Reduce motion and disable blinking",
                );
                if contrast_changed {
                    self.apply_theme(ctx);
                }
                ui.heading("Privacy & diagnostics");
                let consent_changed = ui
                    .checkbox(
                        &mut self.diagnostics_opt_in,
                        "Record bounded local diagnostics for this project",
                    )
                    .changed();
                ui.label(
                    RichText::new("Off by default. Records event types and crash summaries only; no source, datasets, environment variables, or automatic upload.")
                        .size(10.0)
                        .color(MUTED),
                );
                if consent_changed {
                    let root = self.project_root();
                    privacy_diagnostics::configure(self.diagnostics_opt_in, root.as_deref());
                    if self.diagnostics_opt_in {
                        let _ = privacy_diagnostics::record("diagnostics_enabled");
                    }
                    self.status_announcement = if self.diagnostics_opt_in {
                        "Local diagnostics enabled".into()
                    } else {
                        "Local diagnostics disabled".into()
                    };
                }
                if ui.button("Export reviewable diagnostics ZIP…").clicked() {
                    self.console = match (
                        self.project_root(),
                        rfd::FileDialog::new()
                            .set_file_name("forge-diagnostics.zip")
                            .save_file(),
                    ) {
                        (Some(root), Some(path)) => privacy_diagnostics::export_bundle(&root, &path)
                            .map(|()| format!("Exported diagnostics to {}. Nothing was uploaded.", path.display()))
                            .unwrap_or_else(|e| format!("Diagnostics export failed: {e}")),
                        (None, _) => "Open a project before exporting diagnostics.".into(),
                        (_, None) => "Diagnostics export cancelled.".into(),
                    };
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Changes apply immediately and are saved for the next session.")
                        .size(10.0)
                        .color(MUTED),
                );
                if ui.button("Restore appearance defaults").clicked() {
                    self.dark_mode = false;
                    self.editor_font_size = default_editor_font_size();
                    self.caret_blink = default_true();
                    self.high_contrast = false;
                    self.reduced_motion = false;
                    self.apply_theme(ctx);
                }

                ui.separator();
                ui.heading("Keyboard shortcuts");
                // Capture a new chord while a rebind is in progress.
                if let Some(action) = self.rebinding {
                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.rebinding = None;
                    } else if let Some(shortcut) = keymap::capture(ctx) {
                        if shortcut.logical_key != egui::Key::Escape {
                            match self.keymap.conflict(shortcut, action) {
                                Some(other) => {
                                    self.status_announcement =
                                        format!("{} already uses that shortcut", other.label());
                                }
                                None => self.keymap.set(action, shortcut),
                            }
                            self.rebinding = None;
                        }
                    }
                }
                egui::ScrollArea::vertical()
                    .id_salt("keymap_scroll")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        egui::Grid::new("keymap_grid")
                            .num_columns(3)
                            .striped(true)
                            .min_col_width(120.0)
                            .show(ui, |ui| {
                                for action in keymap::KeyAction::ALL {
                                    ui.label(action.label());
                                    if self.rebinding == Some(action) {
                                        ui.label(
                                            RichText::new("press keys… (Esc to cancel)")
                                                .italics()
                                                .color(accent()),
                                        );
                                    } else {
                                        ui.label(
                                            RichText::new(self.keymap.display(action)).monospace(),
                                        );
                                    }
                                    ui.horizontal(|ui| {
                                        if ui.small_button("Rebind").clicked() {
                                            self.rebinding = Some(action);
                                        }
                                        if ui.small_button("Reset").clicked() {
                                            self.keymap.reset(action);
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });
                if ui.button("Restore all shortcut defaults").clicked() {
                    self.keymap.reset_all();
                    self.rebinding = None;
                }
                ui.label(
                    RichText::new("The numeric Ctrl+1…9 pane jumps are fixed.")
                        .size(10.0)
                        .color(MUTED),
                );
            });
        self.settings_open = open;
    }

    /// A start screen with quick actions, recent projects, and language-server
    /// status. Closing it (or acting) hides it for future launches; reopen from
    /// Help → Welcome.
    pub(crate) fn welcome_window(&mut self, ctx: &egui::Context) {
        if !self.welcome_open {
            return;
        }
        let mut open = true;
        let mut close = false;
        egui::Window::new("Welcome to Forge ML")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("An interactive Rust IDE for machine learning.").color(MUTED),
                );
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Open project…").clicked() {
                        self.open_project();
                        close = true;
                    }
                    if ui.button("New file").clicked() {
                        self.create_new_file(None);
                        close = true;
                    }
                    if ui.button("Sample notebook").clicked() {
                        self.tabs.push(welcome_tab());
                        self.active_tab = self.tabs.len() - 1;
                        self.selected_cell = 0;
                        close = true;
                    }
                    if ui.button("Import dataset…").clicked() {
                        self.import_dataset();
                        close = true;
                    }
                });
                if !self.recent_projects.is_empty() {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("RECENT PROJECTS")
                            .size(10.0)
                            .strong()
                            .color(MUTED),
                    );
                    for path in self.recent_projects.clone().into_iter().take(6) {
                        if ui
                            .add(egui::Button::new(path.display().to_string()).frame(false))
                            .clicked()
                        {
                            self.request_open_project_path(path);
                            close = true;
                        }
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("rust-analyzer").strong());
                    ui.label(RichText::new(&self.lsp_status).size(11.0).color(MUTED));
                });
                if ui
                    .small_button("Install or repair language support")
                    .clicked()
                {
                    self.lsp.install();
                    self.lsp_status = "Installing rust-analyzer and rust-src...".to_owned();
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Closing this hides it on future launches — reopen from Help → Welcome.")
                        .size(10.0)
                        .color(MUTED),
                );
            });
        self.welcome_open = open && !close;
    }

    /// Prompt for a new name and dispatch a rust-analyzer rename.
    pub(crate) fn rename_window(&mut self, ctx: &egui::Context) {
        if !self.rename_open {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Rename symbol")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("New name for the symbol at the cursor:");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.rename_input)
                        .desired_width(260.0)
                        .hint_text("new_name"),
                );
                response.request_focus();
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }
                ui.horizontal(|ui| {
                    apply |= ui.button("Rename").clicked();
                    cancel |= ui.button("Cancel").clicked();
                });
            });
        if apply && !self.rename_input.trim().is_empty() {
            let name = self.rename_input.trim().to_owned();
            self.send_rename(name);
            self.rename_open = false;
        } else if cancel || !open {
            self.rename_open = false;
        }
    }

    /// Show available code actions; applying one runs its workspace edit.
    pub(crate) fn code_actions_window(&mut self, ctx: &egui::Context) {
        if self.code_actions.is_empty() {
            return;
        }
        let mut open = true;
        let mut chosen = None;
        egui::Window::new("Code actions")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                for (index, action) in self.code_actions.iter().enumerate() {
                    if ui
                        .add(egui::Button::new(&action.title).frame(false))
                        .clicked()
                    {
                        chosen = Some(index);
                    }
                }
            });
        if let Some(index) = chosen {
            let edits = self.code_actions[index].edits.clone();
            self.apply_file_edits(edits);
            self.code_actions.clear();
        } else if !open {
            self.code_actions.clear();
        }
    }
}
