//! Editor chrome: the file-explorer pane, cell rail, symbol outline, editor
//! tab strip, tab reordering, and pending editor-history application. Methods on
//! the shared [`crate::ForgeApp`].

use crate::ui::theme::*;
use crate::*;
use eframe::egui;
use egui::RichText;

impl crate::ForgeApp {
    pub(crate) fn file_explorer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            use egui_phosphor_icons::icons;
            ui.label(RichText::new("FILES").size(10.0).strong().color(MUTED));
            if compact_icon_button(ui, icons::FOLDER_OPEN, "Open project…").clicked() {
                self.open_project();
            }
            if compact_icon_button(ui, icons::FILE_PLUS, "New file…").clicked() {
                self.create_new_file(None);
            }
            let selected_file = self.active().path.clone().filter(|path| {
                self.project
                    .as_ref()
                    .is_some_and(|project| path.starts_with(&project.root) && path.is_file())
            });
            if enabled_compact_icon_button(
                ui,
                selected_file.is_some(),
                icons::TRASH,
                "Delete the selected file",
            )
            .clicked()
            {
                self.pending_delete = selected_file;
            }
            if compact_icon_button(ui, icons::ARROWS_CLOCKWISE, "Refresh the file tree").clicked() {
                if let Some(project) = &mut self.project {
                    let _ = project.refresh();
                }
            }
        });
        ui.add_space(5.0);
        let selected = self.active().path.clone();
        let action = self.project.as_ref().and_then(|project| {
            ui.label(
                RichText::new(
                    project
                        .root
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("project"),
                )
                .strong()
                .color(TEXT),
            );
            egui::ScrollArea::vertical()
                .id_salt("project_file_tree")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    draw_file_nodes(ui, &project.files, selected.as_deref())
                })
                .inner
        });
        if let Some(action) = action {
            match action {
                ExplorerAction::Open(path) => self.open_file(path),
                ExplorerAction::NewFile(directory) => self.create_new_file(Some(directory)),
                ExplorerAction::Delete(path) => self.pending_delete = Some(path),
            }
        }
        if self.project.is_none() {
            crate::ui::theme::empty_state(
                ui,
                egui_phosphor_icons::icons::FOLDER_OPEN,
                "No project open",
                "Open a Cargo project to browse and edit its files.",
            );
            ui.vertical_centered(|ui| {
                if ui.button("Open project…").clicked() {
                    self.open_project();
                }
                if !self.recent_projects.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Recent").size(10.0).color(MUTED));
                    for path in self.recent_projects.clone().into_iter().take(5) {
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("project")
                            .to_owned();
                        if ui
                            .small_button(name)
                            .on_hover_text(path.display().to_string())
                            .clicked()
                        {
                            self.request_open_project_path(path);
                        }
                    }
                }
            });
        }
    }

    /// The notebook cell rail: status-marked cell list plus session stats.
    pub(crate) fn cell_rail(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("NOTEBOOK CELLS")
                .size(10.0)
                .strong()
                .color(MUTED),
        );
        ui.horizontal(|ui| {
            use egui_phosphor_icons::icons;
            if compact_icon_button(ui, icons::PLUS, "Insert cell after").clicked() {
                self.insert_cell_after();
            }
            if compact_icon_button(ui, icons::ARROW_UP, "Move cell up").clicked() {
                self.move_selected_cell(-1);
            }
            if compact_icon_button(ui, icons::ARROW_DOWN, "Move cell down").clicked() {
                self.move_selected_cell(1);
            }
            if compact_icon_button(ui, icons::TRASH, "Delete selected cell").clicked() {
                self.delete_selected_cell();
            }
            if compact_icon_button(ui, icons::BROOM, "Clear cell outputs").clicked() {
                self.cell_records.clear();
                self.console = "Cell outputs cleared.".to_owned();
            }
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("notebook_cell_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                for (index, (title, _)) in self.cells().iter().enumerate() {
                    let state = self
                        .cell_records
                        .get(&index)
                        .and_then(|r| r.state)
                        .unwrap_or(CellState::Idle);
                    let (mark, color) = match state {
                        CellState::Queued => ("[q]", MUTED),
                        CellState::Running => ("[>]", EMBER),
                        CellState::Passed => ("[ok]", GREEN),
                        CellState::Failed => ("[!]", RED),
                        CellState::Idle => ("[ ]", MUTED),
                    };
                    if ui
                        .selectable_label(
                            index == self.selected_cell,
                            RichText::new(format!("{mark}  {:02}  {title}", index + 1))
                                .monospace()
                                .color(if index == self.selected_cell {
                                    accent()
                                } else {
                                    color
                                }),
                        )
                        .clicked()
                    {
                        self.selected_cell = index;
                    }
                }
            });
        ui.add_space(14.0);
        ui.label(RichText::new("SESSION").size(10.0).strong().color(MUTED));
        status_row(ui, "Engine", "Evcxr", GREEN);
        status_row(ui, "Runs", &self.execution_count.to_string(), accent());
    }

    pub(crate) fn outline(&mut self, ui: &mut egui::Ui) {
        let title = self.active().title.clone();
        let path = self.active().path.clone();
        let content = self.active().content.clone();
        ui.label(RichText::new(title).strong().color(TEXT));
        let mut selected_line = None;
        for (line_no, line) in content.lines().enumerate() {
            let line = line.trim();
            let symbol = ["fn ", "struct ", "enum ", "trait ", "impl ", "mod "]
                .iter()
                .find_map(|prefix| {
                    line.strip_prefix(prefix).map(|rest| {
                        format!(
                            "{}{}",
                            prefix.trim(),
                            rest.split(['(', '{', '<', ' ']).next().unwrap_or("")
                        )
                    })
                });
            if let Some(symbol) = symbol {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("-  {symbol}  :{}", line_no + 1))
                                .monospace()
                                .size(11.0)
                                .color(accent()),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Go to symbol")
                    .clicked()
                {
                    selected_line = Some(line_no);
                }
            }
        }
        if let Some(line) = selected_line {
            if let Some(path) = path {
                self.navigate_to(path, line, 0);
            } else {
                let offset = content
                    .split_inclusive('\n')
                    .take(line)
                    .map(str::chars)
                    .map(Iterator::count)
                    .sum();
                self.pending_editor_selection = Some((offset, offset));
            }
        }
        ui.add_space(8.0);
        ui.separator();
    }

    pub(crate) fn editor_tabs(&mut self, ui: &mut egui::Ui) {
        let mut select = None;
        let mut close = None;
        let mut reorder: Option<(usize, usize)> = None;
        egui::ScrollArea::horizontal()
            .id_salt("editor_tab_strip")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, tab) in self.tabs.iter().enumerate() {
                        let title = if tab.external_change_pending {
                            format!("! {}", tab.title)
                        } else if tab.dirty {
                            format!("* {}", tab.title)
                        } else {
                            tab.title.clone()
                        };
                        let color = if tab.external_change_pending {
                            RED
                        } else if tab.dirty {
                            EMBER
                        } else {
                            TEXT
                        };
                        let selected = index == self.active_tab;
                        // A single click-and-drag widget: clicks switch tabs,
                        // middle-clicks close, drags reorder. (Wrapping in
                        // dnd_drag_source occluded the label's click sense, so
                        // selecting a tab stopped working with multiple tabs.)
                        let label = ui
                            .selectable_label(selected, RichText::new(title).color(color))
                            .interact(egui::Sense::click_and_drag())
                            .on_hover_text("Click to open · drag to reorder · middle-click to close");
                        if label.clicked() {
                            select = Some(index);
                        }
                        if label.middle_clicked() {
                            close = Some(index);
                        }
                        if label.dragged() {
                            egui::DragAndDrop::set_payload(ui.ctx(), index);
                        }
                        if let Some(from) = label.dnd_release_payload::<usize>() {
                            reorder = Some((*from, index));
                        }
                        if selected
                            && compact_icon_button(
                                ui,
                                egui_phosphor_icons::icons::X,
                                "Close editor tab",
                            )
                            .clicked()
                        {
                            close = Some(index);
                        }
                        ui.separator();
                    }
                })
            });
        if let Some(index) = select {
            self.active_tab = index;
            self.selected_cell = 0;
            self.cell_records.clear();
        }
        if let Some((from, to)) = reorder {
            self.move_tab(from, to);
        }
        if let Some(index) = close {
            self.close_tab(index);
        }
    }

    /// Move an editor tab from one position to another (drag-to-reorder),
    /// keeping `active_tab` pointed at the same logical tab.
    fn move_tab(&mut self, from: usize, to: usize) {
        if from == to || from >= self.tabs.len() || to >= self.tabs.len() {
            return;
        }
        let active = self.active_tab;
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active_tab =
            reordered_active_index(active, from, to).min(self.tabs.len().saturating_sub(1));
    }

    pub(crate) fn apply_pending_editor_history(&mut self, ui: &mut egui::Ui) {
        let Some(command) = self.pending_editor_history.take() else {
            return;
        };
        let id = ui.make_persistent_id(format!("editor_{}", self.active_tab));
        let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), id) else {
            return;
        };
        let cursor = state.cursor.char_range().unwrap_or_else(|| {
            egui::text::CCursorRange::one(egui::text::CCursor::new(self.cursor_offset))
        });
        let mut undoer = state.undoer();
        let current = (cursor, self.active().content.clone());
        let restored = match command {
            EditorHistoryCommand::Undo => undoer.undo(&current),
            EditorHistoryCommand::Redo => undoer.redo(&current),
        }
        .cloned();
        let Some((cursor, content)) = restored else {
            return;
        };
        state.set_undoer(undoer);
        state.cursor.set_char_range(Some(cursor));
        state.store(ui.ctx(), id);
        self.cursor_offset = cursor.primary.index.into();
        self.active_mut().content = content;
        self.active_mut().dirty = true;
        self.cell_records.clear();
        self.last_lsp_hash = 0;
    }
}
