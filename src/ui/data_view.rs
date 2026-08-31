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
                    ui.horizontal(|ui| {
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
            ui.label(
                RichText::new("No dataset open. Open one from the Data pane.").color(MUTED),
            );
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
