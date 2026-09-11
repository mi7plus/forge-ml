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
                    ui.label(RichText::new("/").size(11.0).color(MUTED));
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
        self.go_to_line_window(ui.ctx());
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

        // Markdown / HTML files get Edit / Split / Preview modes.
        let preview_kind = self
            .active()
            .path
            .as_ref()
            .and_then(|path| crate::ui::preview::kind_for(path));
        if let Some(kind) = preview_kind {
            use crate::ui::preview::{PreviewKind, PreviewMode};
            // For HTML, offer a live Chromium preview in a dockable pane (the
            // out-of-process forge_cef helper). Needs the file saved on disk.
            let html_path = (kind == PreviewKind::Html)
                .then(|| self.active().path.clone())
                .flatten()
                .filter(|p| p.exists());
            let mut open_web = false;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.preview_mode, PreviewMode::Edit, "Edit");
                ui.selectable_value(&mut self.preview_mode, PreviewMode::Split, "Split");
                ui.selectable_value(&mut self.preview_mode, PreviewMode::Preview, "Preview");
                if kind == PreviewKind::Html {
                    ui.separator();
                    let label = format!(
                        "{}  Open in web view",
                        egui_phosphor_icons::icons::GLOBE.as_str()
                    );
                    if ui
                        .add_enabled(html_path.is_some(), egui::Button::new(label))
                        .on_hover_text("Live Chromium preview in a dockable pane")
                        .clicked()
                    {
                        open_web = true;
                    }
                }
            });
            if open_web {
                if let Some(path) = &html_path {
                    self.pending_open_web_preview =
                        Some(crate::ui::cef_preview::file_url(path));
                }
            }
            let source = self.active().content.clone();
            let path = self.active().path.clone();
            match self.preview_mode {
                PreviewMode::Preview => {
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            crate::ui::preview::render(ui, kind, &source, path.as_deref());
                        });
                    return;
                }
                PreviewMode::Split => {
                    ui.add_space(4.0);
                    let full = ui.available_rect_before_wrap();
                    let gap = 12.0;
                    let half = ((full.width() - gap) / 2.0).max(160.0);
                    let mid = full.min.x + half + gap / 2.0;
                    let left = egui::Rect::from_min_max(
                        full.min,
                        egui::pos2(full.min.x + half, full.max.y),
                    );
                    let right = egui::Rect::from_min_max(
                        egui::pos2(mid + gap / 2.0, full.min.y),
                        full.max,
                    );
                    let divider = ui.visuals().widgets.noninteractive.bg_stroke.color;
                    ui.painter()
                        .vline(mid, full.y_range(), egui::Stroke::new(1.0, divider));
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(left)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                        |ui| self.editor_body(ui),
                    );

                    // Linked scroll for Markdown: when the *editor* is scrolled,
                    // drive the preview to the same fraction through the document.
                    // Only while the editor moves — otherwise the preview scrolls
                    // freely on its own.
                    let content_key = egui::Id::new("split_preview_content_h");
                    let last_editor_key = egui::Id::new("split_editor_last_offset");
                    let mut linked_offset = None;
                    if kind == crate::ui::preview::PreviewKind::Markdown {
                        let row_h = ui
                            .ctx()
                            .fonts_mut(|f| {
                                f.row_height(&egui::FontId::monospace(self.editor_font_size))
                            })
                            .max(10.0);
                        let scroll_id = ui.make_persistent_id(egui::IdSalt::new(format!(
                            "editor_{}_outer_scroll",
                            self.active_tab
                        )));
                        let editor_offset = egui::scroll_area::State::load(ui.ctx(), scroll_id)
                            .map(|state| state.offset.y)
                            .unwrap_or(0.0);
                        let last = ui
                            .ctx()
                            .data(|d| d.get_temp::<f32>(last_editor_key))
                            .unwrap_or(editor_offset);
                        ui.ctx()
                            .data_mut(|d| d.insert_temp(last_editor_key, editor_offset));
                        if (editor_offset - last).abs() > 0.5 {
                            let lines = source.lines().count().max(1) as f32;
                            let fraction = (editor_offset
                                / (lines * row_h - full.height()).max(1.0))
                            .clamp(0.0, 1.0);
                            let preview_h = ui
                                .ctx()
                                .data(|d| d.get_temp::<f32>(content_key))
                                .unwrap_or(0.0);
                            linked_offset = Some(fraction * (preview_h - full.height()).max(0.0));
                        }
                    }

                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(right)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                        |ui| {
                            let mut area = egui::ScrollArea::vertical()
                                .id_salt("preview_split")
                                .auto_shrink([false, false]);
                            if let Some(offset) = linked_offset {
                                area = area.vertical_scroll_offset(offset);
                            }
                            let output = area.show(ui, |ui| {
                                crate::ui::preview::render(ui, kind, &source, path.as_deref());
                            });
                            ui.ctx()
                                .data_mut(|d| d.insert_temp(content_key, output.content_size.y));
                        },
                    );
                    return;
                }
                PreviewMode::Edit => {}
            }
        } else {
            self.preview_mode = crate::ui::preview::PreviewMode::Edit;
        }
        self.editor_body(ui);
    }

    /// The editor body — find bar, code editor, LSP diagnostics/popups, and the
    /// status strip. Extracted so it renders on its own (Edit) or beside the
    /// preview (Split).
    fn editor_body(&mut self, ui: &mut egui::Ui) {
        if self.find_visible {
            let mut next = false;
            let mut replace = false;
            let mut replace_all = false;
            ui.horizontal(|ui| {
                ui.label("Find");
                let response =
                    ui.add(egui::TextEdit::singleline(&mut self.find_query).desired_width(150.0));
                ui.label("Replace");
                ui.add(egui::TextEdit::singleline(&mut self.replace_query).desired_width(150.0));
                next = ui.button("Next").clicked()
                    || (response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter)));
                replace = ui.button("Replace").clicked();
                replace_all = ui.button("All").clicked();
                if compact_icon_button(ui, egui_phosphor_icons::icons::X, "Close find and replace")
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
        let editor_rows = (((ui.available_height() - editor_status_h) / editor_row_h).floor()
            as i64)
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
            let before = self
                .cursor_offset
                .checked_sub(1)
                .and_then(|i| self.tabs[self.active_tab].content.chars().nth(i));
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
            let (a, b) = (range.primary.index.0, range.secondary.index.0);
            self.editor_selection = (a.min(b), a.max(b));
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
                let lang = if self.active_is_rust() {
                    "Rust"
                } else {
                    "Plain text"
                };
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
                paint_navigable_word(ui, &output, &self.tabs[self.active_tab].content, offset);
            }
        }
        // Ctrl+click jumps to definition at the pointer even if the async
        // probe hasn't underlined the word yet (avoids a timing race).
        if ctrl_held && output.response.clicked_by(egui::PointerButton::Primary) {
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
                let popup_position = output.galley_pos + egui::vec2(caret.min.x, caret.max.y + 3.0);
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
                                                RichText::new(&label).monospace().size(11.0),
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
            // Must match how `CodeEditor`'s outer `ScrollArea` derives its id:
            // it passes the salt through `IdSalt::new` before `Ui::make_persistent_id`,
            // so we have to wrap the salt the same way. Passing a bare `String`
            // here hashes differently, `State::load` returns `None`, and the
            // scroll silently never happens.
            let scroll_id = ui.make_persistent_id(egui::IdSalt::new(format!(
                "editor_{}_outer_scroll",
                self.active_tab
            )));
            if let Some(mut scroll_state) = egui::scroll_area::State::load(ui.ctx(), scroll_id) {
                // `ui.available_height()` here is the sliver left below the editor,
                // not the editor's own viewport, so fall back to the pane height.
                let viewport_height = ui.available_height().max(ui.clip_rect().height());
                scroll_state.offset.y = (caret.center().y - viewport_height * 0.45).max(0.0);
                scroll_state.store(ui.ctx(), scroll_id);
            }
            output.response.request_focus();
            ui.ctx().request_repaint();
        }
    }
}
