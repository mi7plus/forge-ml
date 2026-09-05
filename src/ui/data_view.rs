//! The Data inspector and dataset-viewer plumbing: the data pane, the docked
//! and floating dataset viewers, and applying edits back to a dataset. Methods
//! on the shared [`crate::ForgeApp`].

use crate::ui::theme::*;
use crate::*;
use eframe::egui;
use egui::RichText;

impl crate::ForgeApp {
    pub(crate) fn data_inspector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.integration_pending == 0,
                    egui::Button::new("Import file…"),
                )
                .on_hover_text("Import CSV, TSV, JSON Lines, Parquet, or Arrow IPC")
                .clicked()
            {
                self.import_dataset();
            }
            if self.integration_pending > 0 {
                ui.spinner();
                ui.label(
                    RichText::new("Data operation running")
                        .size(9.0)
                        .color(MUTED),
                );
            }
        });
        ui.separator();
        if self.data.vectors.is_empty() && self.data.tables.is_empty() {
            ui.label(
                RichText::new(
                    "Import a supported file, or emit `forge_vector:name=1,2,3` or `forge_table:name={...}`.",
                )
                .monospace()
                .size(10.0)
                .color(MUTED),
            );
            return;
        }
        if ui
            .small_button("Clear datasets")
            .on_hover_text("Remove all live datasets and their vector plots")
            .clicked()
        {
            self.data.vectors.clear();
            self.data.tables.clear();
            self.open_dataset = None;
            self.console = "Cleared all live datasets.".to_owned();
            return;
        }
        let mut dataset_to_delete: Option<(bool, String)> = None;
        let mut export_request: Option<(String, export::DataFormat, &'static str)> = None;
        let mut report_request: Option<String> = None;
        let mut pdf_report_request: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt("data_inspector_vectors")
            .show(ui, |ui| {
                let mut table_names = self.data.tables.keys().cloned().collect::<Vec<_>>();
                table_names.sort();
                for name in table_names {
                    let data = &self.data.tables[&name];
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(RichText::new(&name).strong().color(accent()))
                            .on_hover_text("Open in the data viewer")
                            .clicked()
                        {
                            self.open_dataset = Some(format!("table:{name}"));
                        }
                        ui.label(
                            RichText::new(format!(
                                "{} rows × {} columns",
                                data.rows.len(),
                                data.columns.len()
                            ))
                            .size(10.0)
                            .color(MUTED),
                        );
                        ui.add_enabled_ui(self.integration_pending == 0, |ui| {
                            ui.menu_button("Export", |ui| {
                                for (label, format, extension) in [
                                    ("CSV", export::DataFormat::Csv, "csv"),
                                    ("TSV", export::DataFormat::Tsv, "tsv"),
                                    ("JSON Lines", export::DataFormat::JsonLines, "jsonl"),
                                    ("Parquet", export::DataFormat::Parquet, "parquet"),
                                    ("Arrow IPC", export::DataFormat::Arrow, "arrow"),
                                ] {
                                    if ui.button(label).clicked() {
                                        export_request = Some((name.clone(), format, extension));
                                        ui.close();
                                    }
                                }
                                if ui.button("EDA HTML report").clicked() {
                                    report_request = Some(name.clone());
                                    ui.close();
                                }
                                if ui.button("EDA PDF report").clicked() {
                                    pdf_report_request = Some(name.clone());
                                    ui.close();
                                }
                            });
                        });
                        if compact_icon_button(
                            ui,
                            egui_phosphor_icons::icons::TRASH,
                            "Delete this dataset",
                        )
                        .clicked()
                        {
                            dataset_to_delete = Some((true, name.clone()));
                        }
                    });
                    if let Some(source) = &data.source {
                        ui.label(
                            RichText::new(format!(
                                "Source: {source} · {} Arrow batch(es) · {} rows",
                                data.batches.len(),
                                data.arrow_rows()
                            ))
                            .size(9.0)
                            .color(MUTED),
                        );
                    }
                    egui::Grid::new(format!("table_preview_{name}"))
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("#").strong().color(MUTED));
                            for column in &data.columns {
                                ui.label(RichText::new(column).strong());
                            }
                            ui.end_row();
                            for (index, row) in data.rows.iter().take(5).enumerate() {
                                ui.label(RichText::new(index.to_string()).monospace().color(MUTED));
                                for value in row {
                                    ui.label(RichText::new(value).monospace().size(10.0));
                                }
                                ui.end_row();
                            }
                        });
                    if data.rows.len() > 5 {
                        ui.label(
                            RichText::new("Open the dataset to view and filter all rows.")
                                .size(9.0)
                                .color(MUTED),
                        );
                    }
                    egui::CollapsingHeader::new("Column profile").show(ui, |ui| {
                        egui::Grid::new(format!("profile_{name}"))
                            .striped(true)
                            .show(ui, |ui| {
                                for label in [
                                    "Column", "Missing", "Numeric", "Unique", "Min", "Max", "Mean",
                                    "Std dev",
                                ] {
                                    ui.strong(label);
                                }
                                ui.end_row();
                                for profile in data.profile() {
                                    ui.label(&profile.name);
                                    ui.label(format!(
                                        "{} ({:.1}%)",
                                        profile.missing, profile.missing_percent
                                    ));
                                    ui.label(format!(
                                        "{}/{}",
                                        profile.numeric_count,
                                        data.rows.len().saturating_sub(profile.missing)
                                    ));
                                    ui.label(profile.unique.to_string());
                                    ui.label(
                                        profile
                                            .min
                                            .map(|v| format!("{v:.4}"))
                                            .unwrap_or_else(|| "—".into()),
                                    );
                                    ui.label(
                                        profile
                                            .max
                                            .map(|v| format!("{v:.4}"))
                                            .unwrap_or_else(|| "—".into()),
                                    );
                                    ui.label(
                                        profile
                                            .mean
                                            .map(|v| format!("{v:.4}"))
                                            .unwrap_or_else(|| "—".into()),
                                    );
                                    ui.label(
                                        profile
                                            .std_dev
                                            .map(|v| format!("{v:.4}"))
                                            .unwrap_or_else(|| "—".into()),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
                    egui::CollapsingHeader::new("Quality alerts").show(ui, |ui| {
                        let quality = data.quality();
                        ui.label(format!("{} alert(s)", quality.alerts.len()));
                        if quality.alerts.is_empty() {
                            ui.label("No missingness, constant-column, or mixed-type alerts.");
                        } else {
                            for alert in &quality.alerts {
                                ui.label(RichText::new(format!("• {alert}")).color(EMBER));
                            }
                        }
                    });
                    egui::CollapsingHeader::new("Numeric correlations").show(ui, |ui| {
                        let quality = data.quality();
                        ui.label(
                            RichText::new(format!(
                                "{} pair(s); Pearson correlation over at most {} rows and {} numeric columns.",
                                quality.correlations.len(),
                                quality.correlation_rows, quality.correlation_columns
                            ))
                            .size(9.0)
                            .color(MUTED),
                        );
                        egui::Grid::new(format!("correlations_{name}"))
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Columns");
                                ui.strong("r");
                                ui.end_row();
                                for correlation in quality.correlations.iter().take(20) {
                                    ui.label(format!(
                                        "{} ↔ {}",
                                        correlation.left, correlation.right
                                    ));
                                    ui.label(format!("{:.4}", correlation.coefficient));
                                    ui.end_row();
                                }
                            });
                    });
                    ui.separator();
                }
                for (name, values) in &self.data.vectors {
                    let min = values.iter().copied().reduce(f64::min).unwrap_or(0.0);
                    let max = values.iter().copied().reduce(f64::max).unwrap_or(0.0);
                    let mean = values.iter().sum::<f64>() / values.len().max(1) as f64;
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new(name).strong().color(accent()))
                            .on_hover_text("Open in the data viewer")
                            .clicked()
                        {
                            self.open_dataset = Some(format!("vector:{name}"));
                        }
                        if compact_icon_button(
                            ui,
                            egui_phosphor_icons::icons::TRASH,
                            "Delete this dataset and its plot",
                        )
                        .clicked()
                        {
                            dataset_to_delete = Some((false, name.clone()));
                        }
                    });
                    egui::Grid::new(format!("data_summary_{name}"))
                        .num_columns(4)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Shape");
                            ui.label(format!("[{}]", values.len()));
                            ui.label("dtype");
                            ui.label("f64");
                            ui.end_row();
                            ui.label("Min");
                            ui.label(format!("{min:.5}"));
                            ui.label("Max");
                            ui.label(format!("{max:.5}"));
                            ui.end_row();
                            ui.label("Mean");
                            ui.label(format!("{mean:.5}"));
                            ui.label("Count");
                            ui.label(values.len().to_string());
                            ui.end_row();
                        });
                    ui.label(RichText::new("Values").size(10.0).color(MUTED));
                    egui::ScrollArea::horizontal()
                        .id_salt(format!("data_values_{name}"))
                        .show(ui, |ui| {
                            egui::Grid::new(format!("data_grid_{name}"))
                                .striped(true)
                                .show(ui, |ui| {
                                    for (index, _) in values.iter().enumerate().take(128) {
                                        ui.label(
                                            RichText::new(index.to_string()).monospace().size(9.0),
                                        );
                                    }
                                    ui.end_row();
                                    for value in values.iter().take(128) {
                                        ui.label(
                                            RichText::new(format!("{value:.5}"))
                                                .monospace()
                                                .size(10.0),
                                        );
                                    }
                                    ui.end_row();
                                });
                        });
                    if values.len() > 128 {
                        ui.label(
                            RichText::new(format!("Showing 128 of {} values", values.len()))
                                .size(9.0)
                                .color(MUTED),
                        );
                    }
                    ui.separator();
                }
            });
        if let Some((is_table, name)) = dataset_to_delete {
            if is_table {
                self.data.tables.remove(&name);
            } else {
                self.data.vectors.remove(&name);
            }
            self.console = format!("Deleted dataset `{name}`.");
        }
        if let Some((name, format, extension)) = export_request {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(format!("{name}.{extension}"))
                .save_file()
            {
                let request = self
                    .data
                    .tables
                    .get(&name)
                    .ok_or_else(|| "Dataset no longer exists".to_owned())
                    .and_then(|dataset| {
                        self.integration_worker
                            .submit(IntegrationRequest::DataExport {
                                name: name.clone(),
                                batches: dataset.batches.clone(),
                                path: path.clone(),
                                format,
                            })
                    });
                match request {
                    Ok(()) => {
                        self.integration_pending += 1;
                        self.console = format!("Exporting `{name}` in the background…");
                    }
                    Err(error) => self.console = format!("Dataset export failed: {error}"),
                }
            }
        }
        if let Some(name) = report_request {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(format!("{}-eda.html", safe_file_stem(&name)))
                .save_file()
            {
                self.console = self
                    .data
                    .tables
                    .get(&name)
                    .map(|dataset| std::fs::write(&path, export::dataset_report(&name, dataset)))
                    .transpose()
                    .map(|_| format!("Exported EDA report to {}", path.display()))
                    .unwrap_or_else(|e| format!("EDA report failed: {e}"));
            }
        }
        if let Some(name) = pdf_report_request {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(format!("{}-eda.pdf", safe_file_stem(&name)))
                .save_file()
            {
                self.console = self
                    .data
                    .tables
                    .get(&name)
                    .ok_or_else(|| "Dataset no longer exists".to_owned())
                    .and_then(|dataset| export::dataset_pdf(&name, dataset, &path))
                    .map(|()| format!("Exported EDA PDF to {}", path.display()))
                    .unwrap_or_else(|error| format!("EDA PDF failed: {error}"));
            }
        }
    }

    pub(crate) fn selected_dataset_info(&self) -> Option<(String, bool)> {
        let selection = self.open_dataset.as_ref()?;
        let (kind, name) = selection.split_once(':').unwrap_or(("", selection));
        let exists = match kind {
            "table" => self.data.tables.contains_key(name),
            "vector" => self.data.vectors.contains_key(name),
            _ => false,
        };
        exists.then(|| (name.to_owned(), kind == "table"))
    }

    pub(crate) fn draw_selected_dataset(
        &mut self,
        ui: &mut egui::Ui,
        name: &str,
        editable: bool,
        id_salt: &str,
    ) -> Option<DatasetViewResult> {
        if editable {
            let dataset = self.data.tables.get_mut(name)?;
            let state = self.dataset_views.entry(name.to_owned()).or_default();
            return Some(draw_dataset_table(
                ui,
                &dataset.table,
                state,
                true,
                id_salt,
                Some(dataset.revision),
            ));
        }
        let values = self.data.vectors.get(name)?;
        let table = std::sync::Arc::new(TableData {
            columns: vec!["value".to_owned()],
            rows: values.iter().map(|value| vec![value.to_string()]).collect(),
        });
        Some(draw_dataset_table(
            ui,
            &table,
            self.dataset_views.entry(name.to_owned()).or_default(),
            false,
            id_salt,
            None,
        ))
    }

    fn docked_dataset_viewer(&mut self, ui: &mut egui::Ui) {
        let Some((name, editable)) = self.selected_dataset_info() else {
            self.open_dataset = None;
            return;
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("DATA VIEWER")
                    .size(10.0)
                    .strong()
                    .color(MUTED),
            );
            if ui.small_button("Close").clicked() {
                self.open_dataset = None;
            }
            ui.label(RichText::new(&name).strong().color(accent()));
            if ui
                .small_button("Undock")
                .on_hover_text("Move this viewer to a floating window")
                .clicked()
            {
                self.dataset_viewer_docked = false;
            }
        });
        if let Some(result) = self.draw_selected_dataset(ui, &name, editable, "docked") {
            self.apply_dataset_view_result(&name, result);
        }
    }

    /// Body of the dockable [`PaneKind::DataViewer`] tile. Shows the docked
    /// dataset when one is open, or a hint pointing at the floating window /
    /// the Data pane otherwise.
    pub(crate) fn dock_data_viewer(&mut self, ui: &mut egui::Ui) {
        if self.open_dataset.is_none() {
            ui.add_space(6.0);
            ui.label(RichText::new("No dataset open. Open one from the Data pane.").color(MUTED));
            return;
        }
        if !self.dataset_viewer_docked {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Dataset is in a floating window.").color(MUTED));
                if ui.small_button("Dock here").clicked() {
                    self.dataset_viewer_docked = true;
                }
            });
            return;
        }
        self.docked_dataset_viewer(ui);
    }
}

// ── Dataset table renderer (moved from main.rs) ───────────────────────────

fn sort_header_text(label: &str, sort: Option<bool>) -> egui::WidgetText {
    use egui::text::{LayoutJob, TextFormat};
    let color = accent();
    let mut job = LayoutJob::default();
    job.append(
        label,
        0.0,
        TextFormat {
            font_id: egui::FontId::proportional(12.5),
            color,
            ..Default::default()
        },
    );
    if let Some(descending) = sort {
        let caret = if descending {
            egui_phosphor_icons::icons::CARET_DOWN
        } else {
            egui_phosphor_icons::icons::CARET_UP
        };
        job.append(
            &format!(" {}", caret.as_str()),
            0.0,
            TextFormat {
                font_id: egui::FontId::new(12.5, egui::FontFamily::Name("phosphor-regular".into())),
                color,
                ..Default::default()
            },
        );
    }
    job.into()
}

fn draw_dataset_table(
    ui: &mut egui::Ui,
    data: &std::sync::Arc<TableData>,
    state: &mut DatasetViewState,
    editable: bool,
    id_salt: &str,
    revision: Option<u64>,
) -> DatasetViewResult {
    let mut result = DatasetViewResult::default();
    let immutable_data = data.clone();
    let data = data.as_ref();
    let column_count = data.columns.len();
    state.visible.resize(column_count, true);
    state.pinned.resize(column_count, false);
    state.widths.resize(column_count, 120.0);
    // `horizontal_wrapped` so the filter controls flow onto the next line on a
    // narrow pane instead of overflowing off the right edge (matching the
    // selection/export row below).
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "{} rows × {} columns",
            data.rows.len(),
            data.columns.len()
        ));
        ui.separator();
        ui.label("Filter");
        egui::ComboBox::from_id_salt(("filter_column", id_salt))
            .selected_text(
                state
                    .filter
                    .column
                    .and_then(|index| data.columns.get(index))
                    .map(String::as_str)
                    .unwrap_or("All columns"),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.filter.column, None, "All columns");
                for (index, column) in data.columns.iter().enumerate() {
                    ui.selectable_value(&mut state.filter.column, Some(index), column);
                }
            });
        egui::ComboBox::from_id_salt(("filter_mode", id_salt))
            .selected_text(state.filter.mode.label())
            .show_ui(ui, |ui| {
                for mode in FilterMode::ALL {
                    ui.selectable_value(&mut state.filter.mode, mode, mode.label());
                }
            });
        ui.add(
            egui::TextEdit::singleline(&mut state.filter.text)
                .desired_width(160.0)
                .hint_text(if state.filter.mode.is_numeric() {
                    "Number..."
                } else {
                    "Search values..."
                }),
        );
        if ui.small_button("Clear").clicked() {
            state.filter = RowFilter::default();
        }
        ui.menu_button("Columns", |ui| {
            for (index, column) in data.columns.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.visible[index], column);
                    ui.checkbox(&mut state.pinned[index], "Pin");
                });
            }
        });
        if editable && state.edit_draft.is_none() && ui.button("Edit cells").clicked() {
            state.edit_draft = Some(data.clone());
        }
        if state.edit_draft.is_some() {
            if ui.button("Save edits").clicked() {
                result.committed = state.edit_draft.take();
                state.row_index_cache = None;
            }
            if ui.button("Cancel edits").clicked() {
                state.edit_draft = None;
                state.row_index_cache = None;
                result.message = Some("Discarded dataset edits.".into());
            }
        }
    });
    ui.collapsing("Column widths", |ui| {
        for (index, column) in data
            .columns
            .iter()
            .enumerate()
            .filter(|(i, _)| state.visible[*i])
        {
            ui.horizontal(|ui| {
                ui.label(column);
                ui.add(egui::Slider::new(&mut state.widths[index], 60.0..=360.0).suffix(" px"));
            });
        }
    });
    let mut ordered_columns = (0..column_count)
        .filter(|index| state.visible[*index])
        .collect::<Vec<_>>();
    ordered_columns.sort_by_key(|index| !state.pinned[*index]);
    let editing = state.edit_draft.is_some();
    let DatasetViewState {
        filter,
        sort_column,
        sort_descending,
        selected_rows,
        widths,
        edit_draft,
        linked_x,
        linked_y,
        row_index_cache,
        row_index_pending,
        row_index_worker,
        ..
    } = state;
    let display = edit_draft.as_ref().unwrap_or(data);
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("{} selected", selected_rows.len()));
        if ui.button("Export selection CSV…").clicked() {
            let table = selected_table(display, selected_rows, &ordered_columns);
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("dataset-selection.csv")
                .save_file()
            {
                result.message = Some(
                    data::Dataset::from_table(table, Some("selection".into()))
                        .and_then(|dataset| {
                            export::dataset(&dataset, &path, export::DataFormat::Csv)
                        })
                        .map(|()| format!("Exported selected data to {}", path.display()))
                        .unwrap_or_else(|e| format!("Selection export failed: {e}")),
                );
            }
        }
        egui::ComboBox::from_id_salt(("linked_x", id_salt))
            .selected_text(
                display
                    .columns
                    .get(*linked_x)
                    .map(String::as_str)
                    .unwrap_or("X column"),
            )
            .show_ui(ui, |ui| {
                for index in &ordered_columns {
                    ui.selectable_value(
                        linked_x,
                        *index,
                        format!("X: {}", display.columns[*index]),
                    );
                }
            });
        egui::ComboBox::from_id_salt(("linked_y", id_salt))
            .selected_text(
                display
                    .columns
                    .get(*linked_y)
                    .map(String::as_str)
                    .unwrap_or("Y column"),
            )
            .show_ui(ui, |ui| {
                for index in &ordered_columns {
                    ui.selectable_value(
                        linked_y,
                        *index,
                        format!("Y: {}", display.columns[*index]),
                    );
                }
            });
        if ui.button("Linked scatter").clicked() {
            let points = display
                .rows
                .iter()
                .enumerate()
                .filter(|(index, _)| selected_rows.is_empty() || selected_rows.contains(index))
                .filter_map(|(_, row)| {
                    Some([
                        row.get(*linked_x)?.parse().ok()?,
                        row.get(*linked_y)?.parse().ok()?,
                    ])
                })
                .collect::<Vec<_>>();
            if points.is_empty() {
                result.message =
                    Some("Linked plots require numeric X/Y values in the selected rows.".into());
            } else {
                result.linked_plot = Some(PlotSpec {
                    version: plot::PLOT_SPEC_VERSION,
                    name: format!(
                        "{} vs {}",
                        display.columns[*linked_y], display.columns[*linked_x]
                    ),
                    kind: PlotKind::Scatter,
                    x_label: display.columns[*linked_x].clone(),
                    y_label: display.columns[*linked_y].clone(),
                    series: vec![plot::PlotSeries {
                        name: "selection".into(),
                        points,
                        values: Vec::new(),
                        visible: true,
                    }],
                    matrix: Vec::new(),
                    x_log: false,
                    y_log: false,
                });
            }
        }
    });
    ui.separator();
    let uncached_rows;
    let pending_rows = Vec::new();
    let matching_rows: &[usize] = if let Some(revision) = revision.filter(|_| !editing) {
        let key = RowIndexKey {
            revision,
            filter: filter.clone(),
            sort_column: *sort_column,
            sort_descending: *sort_descending,
        };
        while let Ok(cache) = row_index_worker.receiver.try_recv() {
            let cache_key = RowIndexKey {
                revision: cache.revision,
                filter: cache.filter.clone(),
                sort_column: cache.sort_column,
                sort_descending: cache.sort_descending,
            };
            if cache_key == key {
                *row_index_cache = Some(cache);
                *row_index_pending = None;
            }
        }
        let cache_matches = row_index_cache.as_ref().is_some_and(|cache| {
            cache.revision == revision
                && cache.filter == *filter
                && cache.sort_column == *sort_column
                && cache.sort_descending == *sort_descending
        });
        if !cache_matches
            && row_index_pending.as_ref() != Some(&key)
            && row_index_worker.submit(key.clone(), immutable_data).is_ok()
        {
            *row_index_pending = Some(key);
        }
        if cache_matches {
            &row_index_cache.as_ref().expect("cache matched").rows
        } else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Filtering and sorting dataset in the background…");
            });
            ui.ctx().request_repaint_after(Duration::from_millis(50));
            &pending_rows
        }
    } else {
        uncached_rows = build_row_index(display, filter, *sort_column, *sort_descending);
        &uncached_rows
    };
    let scroll_id = ui.make_persistent_id(("dataset_table_scroll", id_salt));
    let horizontal_offset = egui::scroll_area::State::load(ui.ctx(), scroll_id)
        .map(|state| state.offset.x)
        .unwrap_or_default();
    let column_window = visible_column_window(
        &ordered_columns,
        widths,
        (horizontal_offset - 48.0).max(0.0),
        ui.available_width().max(120.0),
    );
    let rendered_columns = &ordered_columns[column_window.start..column_window.end];
    let display_columns = display.columns.clone();
    egui::ScrollArea::both()
        .id_salt(("dataset_table_scroll", id_salt))
        .auto_shrink([false, false])
        .show_rows(ui, 22.0, matching_rows.len() + 1, |ui, range| {
            egui::Grid::new(("dataset_viewer_grid", id_salt))
                .striped(true)
                .min_col_width(90.0)
                .show(ui, |ui| {
                    if range.start == 0 {
                        let all_selected = !matching_rows.is_empty()
                            && matching_rows
                                .iter()
                                .all(|index| selected_rows.contains(index));
                        ui.horizontal(|ui| {
                            let mut select_all = all_selected;
                            if ui
                                .checkbox(&mut select_all, "")
                                .on_hover_text("Select all matching rows")
                                .changed()
                            {
                                if select_all {
                                    selected_rows.extend(matching_rows.iter().copied());
                                } else {
                                    for index in matching_rows {
                                        selected_rows.remove(index);
                                    }
                                }
                            }
                            let index_sort = (*sort_column == Some(INDEX_SORT_COLUMN))
                                .then_some(*sort_descending);
                            if ui
                                .add(egui::Button::new(sort_header_text("#", index_sort)))
                                .on_hover_text("Sort by original row order")
                                .clicked()
                            {
                                if *sort_column == Some(INDEX_SORT_COLUMN) {
                                    *sort_descending = !*sort_descending;
                                } else {
                                    *sort_column = Some(INDEX_SORT_COLUMN);
                                    *sort_descending = false;
                                }
                            }
                        });
                        if column_window.leading > 0.0 {
                            ui.add_space(column_window.leading);
                        }
                        for index in rendered_columns {
                            let column = &display_columns[*index];
                            let sort = (*sort_column == Some(*index)).then_some(*sort_descending);
                            if ui
                                .add_sized(
                                    [widths[*index], 20.0],
                                    egui::Button::new(sort_header_text(column, sort)),
                                )
                                .clicked()
                            {
                                if *sort_column == Some(*index) {
                                    *sort_descending = !*sort_descending;
                                } else {
                                    *sort_column = Some(*index);
                                    *sort_descending = false;
                                }
                            }
                        }
                        if column_window.trailing > 0.0 {
                            ui.add_space(column_window.trailing);
                        }
                        ui.end_row();
                    }
                    for index in matching_rows
                        .iter()
                        .skip(range.start.saturating_sub(1))
                        .take(range.len())
                    {
                        let mut selected = selected_rows.contains(index);
                        if ui.checkbox(&mut selected, index.to_string()).changed() {
                            if selected {
                                selected_rows.insert(*index);
                            } else {
                                selected_rows.remove(index);
                            }
                        }
                        if column_window.leading > 0.0 {
                            ui.add_space(column_window.leading);
                        }
                        for column in rendered_columns {
                            if editing {
                                ui.add_sized(
                                    [widths[*column], 20.0],
                                    egui::TextEdit::singleline(
                                        &mut edit_draft
                                            .as_mut()
                                            .expect("editing requires a draft")
                                            .rows[*index][*column],
                                    )
                                    .font(egui::TextStyle::Monospace),
                                );
                            } else {
                                ui.add_sized(
                                    [widths[*column], 20.0],
                                    egui::Label::new(
                                        RichText::new(&data.rows[*index][*column])
                                            .monospace()
                                            .size(10.0),
                                    )
                                    .truncate(),
                                );
                            }
                        }
                        if column_window.trailing > 0.0 {
                            ui.add_space(column_window.trailing);
                        }
                        ui.end_row();
                    }
                });
        });
    ui.label(
        RichText::new(format!(
            "{} matching rows · {} of {} visible columns rendered",
            matching_rows.len(),
            rendered_columns.len(),
            ordered_columns.len()
        ))
        .size(9.0)
        .color(MUTED),
    );
    result
}
