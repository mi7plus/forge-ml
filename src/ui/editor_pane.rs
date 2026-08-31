//! The composite editor workspace: the editor pane layout (explorer, tabs,
//! editor body, inspector) and the shared post-editor work. Methods on the
//! shared [`crate::ForgeApp`].

use crate::ui::theme::*;
use crate::*;
use eframe::egui;
use egui::RichText;

impl crate::ForgeApp {
    /// Post-editor work shared by the legacy layout and the docked workspace:
    /// LSP sync, deferred definition probes, and the modal windows.
    /// A `project › folder › file` breadcrumb under the editor tab strip.
    fn editor_breadcrumb(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.active().path.clone() else {
            return;
        };
        let root = self.project.as_ref().map(|project| project.root.clone());
        let mut parts: Vec<String> = Vec::new();
        if let Some(root) = &root {
            if let Some(name) = root.file_name().and_then(|n| n.to_str()) {
                parts.push(name.to_owned());
            }
        }
        let tail = root
            .as_ref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path.as_path());
        for component in tail.components() {
            if let std::path::Component::Normal(segment) = component {
                if let Some(segment) = segment.to_str() {
                    parts.push(segment.to_owned());
                }
            }
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (index, part) in parts.iter().enumerate() {
                if index > 0 {
                    ui.label(RichText::new("›").size(11.0).color(MUTED));
                }
                let last = index + 1 == parts.len();
                ui.label(
                    RichText::new(part)
                        .size(11.0)
                        .color(if last { TEXT } else { MUTED }),
                );
            }
        });
    }

    pub(crate) fn after_editor(&mut self, ui: &mut egui::Ui) {
        self.sync_lsp();
        if let Some(offset) = self.dock_pending_definition_probe.take() {
            self.definition_probe_pending = true;
            self.probe_definition(offset);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(40));
        }
        if std::mem::take(&mut self.dock_pending_ctrl_definition) {
            self.request_lsp("definition");
            self.lsp_status = "Looking up definition...".to_owned();
        }
        self.delete_confirmation(ui.ctx());
        self.unsaved_confirmation(ui.ctx());
        self.settings_window(ui.ctx());
        self.welcome_window(ui.ctx());
        self.rename_window(ui.ctx());
        self.code_actions_window(ui.ctx());
        self.dataset_window(ui.ctx());
        self.dock_floating_windows(ui.ctx());
        self.remote_input_window(ui.ctx());
    }

    /// Render the editor surface: tabs, find bar, code editor, inline
    /// diagnostics, caret, hover/definition probing, and the completion popup.
    /// Shared by the central editor panel and [`PaneKind::Editor`].
    pub(crate) fn editor_pane(&mut self, ui: &mut egui::Ui) {
                self.editor_tabs(ui);
                self.editor_breadcrumb(ui);
                self.external_change_banner(ui);
                self.apply_pending_editor_history(ui);
                if self.find_visible {
                    let mut next = false;
                    let mut replace = false;
                    let mut replace_all = false;
                    ui.horizontal(|ui| {
                        ui.label("Find");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.find_query).desired_width(150.0),
                        );
                        ui.label("Replace");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.replace_query)
                                .desired_width(150.0),
                        );
                        next = ui.button("Next").clicked()
                            || (response.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter)));
                        replace = ui.button("Replace").clicked();
                        replace_all = ui.button("All").clicked();
                        if compact_icon_button(
                            ui,
                            egui_phosphor_icons::icons::X,
                            "Close find and replace",
                        )
                        .clicked()
                        {
                            self.find_visible = false;
                        }
                    });
                    if next {
                        self.find_next();
                    }
                    if replace {
                        self.replace_current();
                    }
                    if replace_all {
                        self.replace_all();
                    }
                }
                ui.add_space(5.0);
                // Grow the editor to fill the pane down to a one-line status
                // strip at the bottom, instead of a fixed 32-row box.
                let editor_status_h = self.editor_font_size + 10.0;
                let editor_row_h = ui
                    .ctx()
                    .fonts_mut(|f| f.row_height(&egui::FontId::monospace(self.editor_font_size)))
                    .max(10.0);
                let editor_rows = (((ui.available_height() - editor_status_h) / editor_row_h)
                    .floor() as i64)
                    .max(3) as usize;
                let output = CodeEditor::default()
                    .id_source(format!("editor_{}", self.active_tab))
                    .with_rows(editor_rows)
                    .with_fontsize(self.editor_font_size)
                    .with_theme(crate::ui::theme::editor_color_theme(
                        &crate::ui::theme::active_palette(),
                    ))
                    .with_numlines(true)
                    .show(ui, &mut self.tabs[self.active_tab].content, &Syntax::rust());
                if self.editor_needs_initial_focus {
                    output.response.request_focus();
                    self.editor_needs_initial_focus = false;
                    ui.ctx().request_repaint();
                }
                if output.response.changed() {
                    self.tabs[self.active_tab].dirty = true;
                    self.cell_records.clear();
                    // Signature help: request after '(' or ',', dismiss on ')'.
                    let before = self.cursor_offset.checked_sub(1).and_then(|i| {
                        self.tabs[self.active_tab].content.chars().nth(i)
                    });
                    match before {
                        Some('(') | Some(',') => self.request_lsp("signature"),
                        Some(')') => self.lsp_signature.clear(),
                        _ => {}
                    }
                }
                if let Some(diagnostics) = self
                    .active()
                    .path
                    .as_ref()
                    .and_then(|path| self.lsp_diagnostics.get(path))
                {
                    paint_inline_diagnostics(
                        ui,
                        &output,
                        &self.tabs[self.active_tab].content,
                        diagnostics,
                    );
                }
                if let Some(range) = output.cursor_range {
                    self.cursor_offset = range.primary.index.0;
                    self.select_cell_from_caret();
                    if output.response.has_focus() {
                        paint_editor_caret(
                            ui,
                            &output,
                            range.primary,
                            self.dark_mode,
                            self.caret_blink && !self.reduced_motion,
                        );
                    }
                }
                // Editor status strip pinned under the editor (Ln/Col + language).
                let (caret_line, caret_col) =
                    line_column(&self.tabs[self.active_tab].content, self.cursor_offset);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Ln {caret_line}, Col {caret_col}"))
                            .monospace()
                            .size(11.0)
                            .color(MUTED),
                    );
                    ui.separator();
                    let chars = self.tabs[self.active_tab].content.chars().count();
                    ui.label(
                        RichText::new(format!("{chars} chars"))
                            .size(11.0)
                            .color(MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let lang = if self.active_is_rust() { "Rust" } else { "Plain text" };
                        ui.label(RichText::new(lang).size(11.0).color(MUTED));
                    });
                });
                let ctrl_held = ui.input(|input| input.modifiers.ctrl);
                let hovered_offset = if output.response.hovered() {
                    ui.ctx().pointer_hover_pos().and_then(|pointer| {
                        let raw_offset = output
                            .galley
                            .cursor_from_pos(pointer - output.galley_pos)
                            .index
                            .0;
                        word_start_at(&self.tabs[self.active_tab].content, raw_offset)
                    })
                } else {
                    None
                };
                if hovered_offset != self.hover_probe_offset {
                    self.hover_probe_offset = hovered_offset;
                    self.navigable_hover_offset = None;
                    self.dock_pending_definition_probe = hovered_offset;
                }
                if let Some(offset) = hovered_offset {
                    if self.navigable_hover_offset == Some(offset) {
                        if ctrl_held {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        paint_navigable_word(
                            ui,
                            &output,
                            &self.tabs[self.active_tab].content,
                            offset,
                        );
                    }
                }
                // Ctrl+click jumps to definition at the pointer even if the async
                // probe hasn't underlined the word yet (avoids a timing race).
                if ctrl_held
                    && output.response.clicked_by(egui::PointerButton::Primary)
                {
                    if let Some(offset) = hovered_offset {
                        self.cursor_offset = offset;
                        self.dock_pending_ctrl_definition = true;
                    }
                }
                // Right-click moves the caret to the pointer, then offers the
                // source-navigation actions there.
                if output.response.secondary_clicked() {
                    if let Some(offset) = hovered_offset {
                        self.cursor_offset = offset;
                    }
                }
                output.response.context_menu(|ui| {
                    if ui.button("Go to definition   (Ctrl+click)").clicked() {
                        self.dock_pending_ctrl_definition = true;
                        ui.close();
                    }
                    if ui.button("Find references").clicked() {
                        self.request_lsp("references");
                        ui.close();
                    }
                    if ui.button("Rename symbol…").clicked() {
                        self.rename_open = true;
                        self.rename_input.clear();
                        ui.close();
                    }
                });
                // Signature help popup above the caret.
                if !self.lsp_signature.is_empty() {
                    if let Some(range) = output.cursor_range {
                        let caret = output.galley.pos_from_cursor(range.primary);
                        let pos = output.galley_pos + egui::vec2(caret.min.x, caret.min.y - 24.0);
                        egui::Area::new(egui::Id::new("editor_signature_popup"))
                            .order(egui::Order::Foreground)
                            .fixed_pos(pos)
                            .show(ui.ctx(), |ui| {
                                egui::Frame::popup(ui.style()).show(ui, |ui| {
                                    ui.label(
                                        RichText::new(&self.lsp_signature)
                                            .monospace()
                                            .size(11.0)
                                            .color(accent()),
                                    );
                                });
                            });
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            self.lsp_signature.clear();
                        }
                    }
                }
                if self.completion_popup_open {
                    if let Some(range) = output.cursor_range {
                        let caret = output.galley.pos_from_cursor(range.primary);
                        let popup_position =
                            output.galley_pos + egui::vec2(caret.min.x, caret.max.y + 3.0);
                        let completions = self
                            .completions
                            .iter()
                            .take(12)
                            .cloned()
                            .collect::<Vec<_>>();
                        let mut selected = None;
                        let popup = egui::Area::new(egui::Id::new("editor_completion_popup"))
                            .order(egui::Order::Foreground)
                            .fixed_pos(popup_position)
                            .show(ui.ctx(), |ui| {
                                egui::Frame::popup(ui.style()).show(ui, |ui| {
                                    ui.set_min_width(240.0);
                                    ui.label(
                                        RichText::new("RUST-ANALYZER COMPLETIONS")
                                            .size(9.0)
                                            .strong()
                                            .color(MUTED),
                                    );
                                    egui::ScrollArea::vertical()
                                        .id_salt("editor_completion_popup_list")
                                        .max_height(240.0)
                                        .show(ui, |ui| {
                                            for (label, insert) in completions {
                                                if ui
                                                    .selectable_label(
                                                        false,
                                                        RichText::new(&label)
                                                            .monospace()
                                                            .size(11.0),
                                                    )
                                                    .clicked()
                                                {
                                                    selected = Some(insert);
                                                }
                                            }
                                        });
                                });
                            });
                        if let Some(completion) = selected {
                            self.apply_completion(&completion);
                        } else if ui.input(|input| input.pointer.any_pressed())
                            && !popup.response.contains_pointer()
                            && !output.response.contains_pointer()
                        {
                            self.completion_popup_open = false;
                        }
                    }
                }
                if let Some((start, end)) = self.pending_editor_selection.take() {
                    let mut state = output.state.clone();
                    let target_cursor = egui::text::CCursor::new(start);
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            target_cursor,
                            egui::text::CCursor::new(end),
                        )));
                    state.store(ui.ctx(), output.response.id);
                    let caret = output.galley.pos_from_cursor(target_cursor);
                    let scroll_id =
                        ui.make_persistent_id(format!("editor_{}_outer_scroll", self.active_tab));
                    if let Some(mut scroll_state) =
                        egui::scroll_area::State::load(ui.ctx(), scroll_id)
                    {
                        let viewport_height = ui.available_height().max(120.0);
                        scroll_state.offset.y =
                            (caret.center().y - viewport_height * 0.45).max(0.0);
                        scroll_state.store(ui.ctx(), scroll_id);
                    }
                    output.response.request_focus();
                    ui.ctx().request_repaint();
                }
    }
}
