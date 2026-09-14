//! The two framework trait impls for [`crate::ForgeApp`], split out of `main.rs`
//! to keep that file focused on the app struct and its inherent methods:
//!
//!   * [`egui_tiles::Behavior`] — how each dockable pane is titled and painted,
//!     and the tab context-menu (hide / undock) plumbing; and
//!   * [`eframe::App`] — the per-frame `ui` entry point and the `save` hook that
//!     persists the session.
//!
//! Both are pure view/lifecycle glue; the dock-tree builders and helpers they
//! call (`build_dock_tree`, `dock_tile_of`, …) stay in `main.rs`.

use crate::ui::theme::*;
use crate::*;
use eframe::egui;
use egui::RichText;

impl egui_tiles::Behavior<PaneKind> for ForgeApp {
    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: TileId,
        pane: &mut PaneKind,
    ) -> egui_tiles::UiResponse {
        ui.add_space(2.0);
        self.dock_pane_body(*pane, ui);
        egui_tiles::UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &PaneKind) -> egui::WidgetText {
        // Terminal tabs reflect the shell's live OSC title, or a numbered
        // fallback so multiple terminals stay distinguishable.
        if let PaneKind::Terminal(id) = *pane {
            let live = self
                .terminals
                .get(&id)
                .map(|t| t.title())
                .unwrap_or("Terminal");
            let label = if live == "Terminal" {
                format!("Terminal {id}")
            } else {
                live.to_string()
            };
            return format!("{}  {}", pane.icon(), label).into();
        }
        if let PaneKind::RustConsole(id) = *pane {
            return format!("{}  Rust {id}", pane.icon()).into();
        }
        pane.tab_label().into()
    }

    /// Right-click a tab for hide / undock actions. The tree isn't available
    /// here, so the chosen action is recorded and applied after layout.
    fn on_tab_button(
        &mut self,
        tiles: &mut Tiles<PaneKind>,
        tile_id: TileId,
        button_response: egui::Response,
    ) -> egui::Response {
        let kind = tiles.get_pane(&tile_id).copied();
        button_response.context_menu(|ui| {
            ui.label(
                RichText::new(kind.map(|k| k.title()).unwrap_or("Pane"))
                    .strong()
                    .color(MUTED),
            );
            if kind == Some(PaneKind::DataViewer) {
                // The data viewer has its own floating window mechanism.
                let label = if self.dataset_viewer_docked {
                    "Undock to a floating window"
                } else {
                    "Dock data viewer"
                };
                if ui.button(label).clicked() {
                    self.dataset_viewer_docked = !self.dataset_viewer_docked;
                    ui.close();
                }
            } else if ui
                .button("Undock to a floating window")
                .on_hover_text("Pop this pane out into a movable window")
                .clicked()
            {
                self.pending_dock_action = Some((tile_id, DockAction::Undock));
                ui.close();
            }
            if ui
                .button("Hide pane")
                .on_hover_text("Bring it back from View -> Panes")
                .clicked()
            {
                self.pending_dock_action = Some((tile_id, DockAction::Hide));
                ui.close();
            }
            if matches!(kind, Some(PaneKind::Terminal(_))) {
                ui.separator();
                if ui
                    .button("New terminal")
                    .on_hover_text("Open another terminal beside this one")
                    .clicked()
                {
                    self.pending_new_terminal = Some(Some(tile_id));
                    ui.close();
                }
            }
            if matches!(kind, Some(PaneKind::RustConsole(_))) {
                ui.separator();
                if ui
                    .button("New Rust kernel")
                    .on_hover_text("Open another independent Rust kernel beside this one")
                    .clicked()
                {
                    self.pending_new_kernel = Some(Some(tile_id));
                    ui.close();
                }
            }
        });
        button_response
    }

    /// Terminal and Rust-kernel tabs get a close button; the others stay put and
    /// are hidden via the View menu instead.
    fn is_tab_closable(&self, tiles: &Tiles<PaneKind>, tile_id: TileId) -> bool {
        matches!(
            tiles.get_pane(&tile_id),
            Some(PaneKind::Terminal(_)) | Some(PaneKind::RustConsole(_))
        )
    }

    fn on_tab_close(&mut self, tiles: &mut Tiles<PaneKind>, tile_id: TileId) -> bool {
        match tiles.get_pane(&tile_id) {
            // Kill the backing session before the tile is removed.
            Some(PaneKind::Terminal(id)) => {
                self.terminals.remove(id);
            }
            Some(PaneKind::RustConsole(id)) => {
                self.kernels.remove(id);
            }
            Some(PaneKind::WebPreview) => {
                // Dropping the session shuts the forge_cef helper down.
                self.web_preview = None;
            }
            _ => {}
        }
        true
    }

    fn simplification_options(&self) -> SimplificationOptions {
        SimplificationOptions {
            // Keep emptied tab groups from vanishing so a hidden pane can be
            // brought back; still allow single-child pruning for tidy splits.
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }
}

impl eframe::App for ForgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.theme_dirty {
            self.apply_theme(ui.ctx());
            self.theme_dirty = false;
        }
        // Startup splash while the Rust runtime boots; keep background work
        // ticking underneath so it progresses to Ready.
        if self.splash_active(ui.ctx()) {
            self.poll_background(ui.ctx());
            // Kick rust-analyzer so it starts indexing behind the splash.
            self.sync_lsp();
            self.draw_splash(ui);
            ui.ctx().request_repaint();
            return;
        }
        self.splash_start = None;
        self.accessibility_shortcuts(ui.ctx());
        self.command_palette(ui.ctx());
        self.poll_background(ui.ctx());
        // Legacy navigation still assigns `inspector_tab`; when it changes, bring
        // the matching dock pane to the front of its tab group.
        if self.inspector_tab != self.last_inspector_tab {
            self.last_inspector_tab = self.inspector_tab;
            self.dock_focus = Some(PaneKind::Inspector(self.inspector_tab));
        }
        // Shortcut handling runs through the customizable keymap, and is paused
        // while the user is capturing a new binding in Settings.
        use keymap::KeyAction;
        let ctx = ui.ctx().clone();
        let paused = self.rebinding.is_some();
        let save = !paused && self.keymap.triggered(KeyAction::Save, &ctx);
        let new_file = !paused && self.keymap.triggered(KeyAction::NewFile, &ctx);
        let find = !paused && self.keymap.triggered(KeyAction::FindInFile, &ctx);
        let find_in_files = !paused && self.keymap.triggered(KeyAction::FindInProject, &ctx);
        let complete = !paused && self.keymap.triggered(KeyAction::RequestCompletion, &ctx);
        let run = !paused && self.keymap.triggered(KeyAction::RunCell, &ctx);
        let run_all = !paused && self.keymap.triggered(KeyAction::RunAll, &ctx);
        let format_doc = !paused && self.keymap.triggered(KeyAction::FormatDocument, &ctx);
        if save {
            self.save_active();
        }
        if new_file {
            self.create_new_file(None);
        }
        if format_doc {
            self.format_document();
        }
        if find_in_files {
            self.inspector_tab = InspectorTab::Search;
        } else if find {
            self.find_visible = true;
        }
        if complete {
            self.request_lsp("complete");
            self.lsp_status = "Requesting completions...".to_owned();
        }
        if self.completion_popup_open && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.completion_popup_open = false;
        }
        if run_all {
            self.enqueue_cells(0..self.cells().len());
        } else if run {
            self.enqueue_cells([self.selected_cell]);
        }
        Panel::top("menu_bar")
            .resizable(false)
            .default_size(28.0)
            .frame(compact_panel_frame(
                theme_colors(self.dark_mode).menu,
                self.dark_mode,
            ))
            .show(ui, |ui| self.menu_bar(ui));
        Panel::top("command_bar")
            .resizable(false)
            .default_size(42.0)
            .frame(compact_panel_frame(
                theme_colors(self.dark_mode).surface,
                self.dark_mode,
            ))
            .show(ui, |ui| self.top_bar(ui));
        Panel::bottom("status_bar")
            .resizable(false)
            .default_size(24.0)
            .frame(compact_panel_frame(
                theme_colors(self.dark_mode).surface,
                self.dark_mode,
            ))
            .show(ui, |ui| self.status_bar(ui));
        let dock_frame = panel_frame(theme_colors(self.dark_mode).background, self.dark_mode);
        egui::CentralPanel::default()
            .frame(dock_frame)
            .show(ui, |ui| {
                // Take the tree out so both it and `self` (the Behavior) can be
                // borrowed mutably during layout; restore it immediately after.
                let mut tree = self.dock_tree.take().unwrap_or_else(build_dock_tree);
                if let Some(kind) = self.dock_focus.take() {
                    if let Some(id) = Self::dock_tile_of(&tree, kind) {
                        tree.tiles.set_visible(id, true);
                        tree.make_active(|_, tile| matches!(tile, Tile::Pane(p) if *p == kind));
                    }
                }
                tree.ui(self, ui);
                // Apply a tab context-menu action now that the full tree is in hand.
                if let Some((tile, action)) = self.pending_dock_action.take() {
                    match action {
                        DockAction::Hide => tree.tiles.set_visible(tile, false),
                        DockAction::Undock => {
                            // Float: hide the tile and render the pane in a window.
                            if let Some(kind) = tree.tiles.get_pane(&tile).copied() {
                                tree.tiles.set_visible(tile, false);
                                if !self.floating_panes.contains(&kind) {
                                    self.floating_panes.push(kind);
                                }
                            }
                        }
                    }
                }
                if let Some(anchor) = self.pending_new_terminal.take() {
                    let kind = Self::create_terminal(&mut tree, anchor);
                    self.dock_focus = Some(kind);
                }
                if let Some(anchor) = self.pending_new_kernel.take() {
                    let kind = Self::create_kernel(&mut tree, anchor);
                    self.dock_focus = Some(kind);
                }
                if let Some(url) = self.pending_open_web_preview.take() {
                    let kind = Self::ensure_web_preview_tile(&mut tree);
                    // Reuse a running helper (just navigate) or spawn a new one.
                    match self.web_preview.as_mut() {
                        Some(preview) => preview.navigate(&url),
                        None => match ui::cef_preview::CefPreview::spawn(&url, (960, 720)) {
                            Ok(preview) => self.web_preview = Some(preview),
                            Err(error) => self.console = format!("Web preview: {error}"),
                        },
                    }
                    self.dock_focus = Some(kind);
                }
                self.dock_tree = Some(tree);
            });
        self.after_editor(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let recovery = WorkspaceRecovery {
            open_files: self
                .tabs
                .iter()
                .filter_map(|tab| tab.path.clone())
                .collect(),
            active_file: self.active().path.clone(),
            explorer_height: Some(self.explorer_height),
            dataset_pane_height: Some(self.dataset_pane_height),
            dataset_viewer_docked: Some(self.dataset_viewer_docked),
        };
        if let Some(store) = &self.workspace_store {
            if let Err(error) = store.save_recovery(&recovery) {
                self.console = format!("Could not save workspace recovery state: {error}");
            }
        }
        let state = SessionState {
            project_root: self.project.as_ref().map(|p| p.root.clone()),
            open_files: self
                .tabs
                .iter()
                .filter_map(|tab| tab.path.clone())
                .collect(),
            active_file: self.active().path.clone(),
            dark_mode: self.dark_mode,
            explorer_height: self.explorer_height,
            recent_projects: self.recent_projects.clone(),
            editor_font_size: self.editor_font_size,
            caret_blink: self.caret_blink,
            format_on_save: self.format_on_save,
            show_welcome: self.welcome_open,
            keymap: self.keymap.to_dto(),
            high_contrast: self.high_contrast,
            lsp_enabled: self.lsp_enabled,
            reduced_motion: self.reduced_motion,
            diagnostics_opt_in: self.diagnostics_opt_in,
            saved_runs: self.saved_runs.clone(),
            experiment_name: self.experiment_name.clone(),
            comparison_metric: self.comparison_metric.clone(),
            dataset_viewer_docked: self.dataset_viewer_docked,
            dataset_pane_height: self.dataset_pane_height,
            selected_python: self.selected_python.clone(),
            selected_jupyter_kernel: self.selected_jupyter_kernel.clone(),
            python_environment_fingerprint: self.python_environment_fingerprint.clone(),
            structured_plots: session::bounded_plots(&self.structured_plots),
            native_regression_artifact: self.native_burn_artifact.clone(),
            native_inference_feature: self.native_burn_inference_feature,
            drift_mean_shift_threshold: self.drift_mean_shift_threshold,
            drift_scale_ratio_lower: self.drift_scale_ratio_lower,
            drift_scale_ratio_upper: self.drift_scale_ratio_upper,
            native_training_backend: self.deep_backend,
            compute_device: self.compute_device,
            native_training_epochs: self.burn_training_epochs,
            native_training_learning_rate: self.burn_training_learning_rate,
            native_training_validation_fraction: self.burn_training_validation_fraction,
            native_training_patience: self.early_stopping_patience,
            native_training_use_dataset: self.burn_training_use_dataset,
            native_training_feature: self.burn_training_feature.clone(),
            native_training_target: self.burn_training_target.clone(),
            dock_layout: self
                .dock_tree
                .as_ref()
                .and_then(|tree| serde_json::to_string(tree).ok()),
            active_theme: self.active_theme.clone(),
            custom_themes: self.custom_themes.clone(),
            ui_scale: self.ui_scale,
        };
        eframe::set_value(storage, STORAGE_KEY, &state);
    }
}
