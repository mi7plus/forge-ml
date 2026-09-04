//! Workspace panes: project search results, the experiments/runs board,
//! the charts and structured-plot viewers, and the console pane. Methods on the
//! shared [`crate::ForgeApp`].

use crate::ui::theme::*;
use crate::*;
use eframe::egui;
use egui::RichText;

impl crate::ForgeApp {
    pub(crate) fn project_search(&mut self, ui: &mut egui::Ui) {
        // rust-analyzer "Find references" results, when present.
        if !self.lsp_references.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("REFERENCES ({})", self.lsp_references.len()))
                        .size(10.0)
                        .strong()
                        .color(MUTED),
                );
                if ui.small_button("Clear").clicked() {
                    self.lsp_references.clear();
                }
            });
            let mut navigate = None;
            egui::ScrollArea::vertical()
                .id_salt("lsp_reference_list")
                .max_height(160.0)
                .show(ui, |ui| {
                    for reference in &self.lsp_references {
                        let label = format!(
                            "{}:{}:{}",
                            file_title(&reference.path),
                            reference.line + 1,
                            reference.column + 1
                        );
                        if ui
                            .add(
                                egui::Button::new(RichText::new(label).monospace().size(11.0))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            navigate =
                                Some((reference.path.clone(), reference.line, reference.column));
                        }
                    }
                });
            if let Some((path, line, column)) = navigate {
                self.navigate_to(path, line, column);
            }
            ui.separator();
        }
        let mut run_search = false;
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.project_search_query)
                    .desired_width(180.0)
                    .hint_text("Search project..."),
            );
            run_search = ui.button("Find").clicked()
                || (response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.project_search_case_sensitive, "Match case");
            if ui.button("Clear").clicked() {
                self.project_search_results.clear();
            }
        });
        if run_search {
            self.run_project_search();
        }
        ui.separator();
        ui.label(
            RichText::new(format!("{} result(s)", self.project_search_results.len()))
                .size(10.0)
                .color(MUTED),
        );
        let root = self.project.as_ref().map(|project| project.root.clone());
        let mut navigate = None;
        egui::ScrollArea::vertical()
            .id_salt("project_search_results")
            .show(ui, |ui| {
                for result in &self.project_search_results {
                    let shown_path = root
                        .as_ref()
                        .and_then(|root| result.path.strip_prefix(root).ok())
                        .unwrap_or(&result.path);
                    let label = format!(
                        "{}:{}:{}\n  {}",
                        shown_path.display(),
                        result.line + 1,
                        result.column + 1,
                        result.preview
                    );
                    if ui
                        .add(
                            egui::Button::new(RichText::new(label).monospace().size(10.0))
                                .frame(false)
                                .wrap(),
                        )
                        .on_hover_text("Open search result")
                        .clicked()
                    {
                        navigate = Some((result.path.clone(), result.line, result.column));
                    }
                    ui.separator();
                }
            });
        if let Some((path, line, column)) = navigate {
            self.navigate_to(path, line, column);
        }
    }

    pub(crate) fn experiments(&mut self, ui: &mut egui::Ui) {
        if self.saved_runs.is_empty() {
            crate::ui::theme::empty_state(
                ui,
                egui_phosphor_icons::icons::FLASK,
                "No saved runs",
                "Save a snapshot from the Plots pane to compare training runs here.",
            );
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Metric");
            egui::ComboBox::from_id_salt("comparison_metric")
                .selected_text(&self.comparison_metric)
                .show_ui(ui, |ui| {
                    let mut names = self
                        .saved_runs
                        .iter()
                        .flat_map(|run| run.metrics.keys().cloned())
                        .collect::<Vec<_>>();
                    names.sort();
                    names.dedup();
                    for name in names {
                        ui.selectable_value(&mut self.comparison_metric, name.clone(), name);
                    }
                });
            if ui.button("Export CSV").clicked() {
                self.export_telemetry_csv();
            }
            if ui.button("HTML report").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("experiment-comparison.html")
                    .save_file()
                {
                    self.console = std::fs::write(
                        &path,
                        export::experiment_report(&self.saved_runs, &self.comparison_metric),
                    )
                    .map(|()| format!("Exported experiment report to {}", path.display()))
                    .unwrap_or_else(|e| format!("Experiment report failed: {e}"));
                }
            }
            if ui.button("PDF report").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("experiment-comparison.pdf")
                    .save_file()
                {
                    self.console =
                        export::experiment_pdf(&self.saved_runs, &self.comparison_metric, &path)
                            .map(|()| format!("Exported experiment PDF to {}", path.display()))
                            .unwrap_or_else(|error| format!("Experiment PDF failed: {error}"));
                }
            }
        });
        let colors = [
            accent(),
            EMBER,
            GREEN,
            RED,
            Color32::from_rgb(150, 105, 210),
        ];
        Plot::new("experiment_comparison")
            .height(220.0)
            .allow_drag(false)
            .show(ui, |plot| {
                for (index, run) in self.saved_runs.iter().enumerate() {
                    if let Some(values) = run.metrics.get(&self.comparison_metric) {
                        let points: PlotPoints = values.clone().into();
                        plot.line(
                            Line::new(run.name.clone(), points)
                                .color(colors[index % colors.len()])
                                .width(2.0),
                        );
                    }
                }
            });
        let mut run_to_delete = None;
        let mut run_to_clone = None;
        let mut run_to_toggle_archive = None;
        let mut run_to_export = None;
        egui::ScrollArea::vertical()
            .id_salt("experiment_run_table")
            .show(ui, |ui| {
                egui::Grid::new("experiment_table")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Run").strong());
                        ui.label(RichText::new("Final").strong());
                        ui.label(RichText::new("Steps").strong());
                        ui.label(RichText::new("Execs").strong());
                        ui.label(RichText::new("Status").strong());
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                        for (index, run) in self.saved_runs.iter().enumerate() {
                            let values = run.metrics.get(&self.comparison_metric);
                            let final_value = values
                                .and_then(|values| values.last())
                                .map(|point| format!("{:.6}", point[1]))
                                .unwrap_or_else(|| "-".to_owned());
                            ui.label(&run.name);
                            ui.label(final_value);
                            ui.label(values.map_or(0, Vec::len).to_string());
                            ui.label(run.execution_count.to_string());
                            ui.label(if run.archived { "Archived" } else { "Active" });
                            if ui.small_button("Clone").clicked() {
                                run_to_clone = Some(index);
                            }
                            if ui
                                .small_button(if run.archived { "Restore" } else { "Archive" })
                                .clicked()
                            {
                                run_to_toggle_archive = Some(index);
                            }
                            if compact_icon_button(
                                ui,
                                egui_phosphor_icons::icons::TRASH,
                                "Delete this saved run",
                            )
                            .clicked()
                            {
                                run_to_delete = Some(index);
                            }
                            if ui
                                .small_button("Bundle")
                                .on_hover_text("Export run manifest and artifacts as ZIP")
                                .clicked()
                            {
                                run_to_export = Some(index);
                            }
                            ui.end_row();
                            ui.label("");
                            ui.label(
                                RichText::new(format!("tags: {}", run.tags.join(", ")))
                                    .size(9.0)
                                    .color(MUTED),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "git: {}{}",
                                    run.provenance
                                        .git_commit
                                        .chars()
                                        .take(10)
                                        .collect::<String>(),
                                    if run.provenance.git_dirty {
                                        " dirty"
                                    } else {
                                        ""
                                    }
                                ))
                                .size(9.0)
                                .color(MUTED),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "artifacts: {} · datasets: {} · {}",
                                    run.artifacts.len(),
                                    run.provenance.datasets.len(),
                                    if run.provenance.fingerprint_algorithm.is_empty() {
                                        "legacy fingerprints"
                                    } else {
                                        run.provenance.fingerprint_algorithm.as_str()
                                    }
                                ))
                                .size(9.0)
                                .color(MUTED),
                            );
                            ui.label(RichText::new(run.notes.clone()).size(9.0).color(MUTED));
                            ui.end_row();
                        }
                    });
            });
        if let Some(index) = run_to_clone {
            let child = self.saved_runs[index]
                .clone_as_child(format!("{}_clone", self.saved_runs[index].name));
            if let Some(store) = &self.workspace_store {
                let _ = store.save_experiment(&child.id, &child.name, &child);
            }
            self.saved_runs.push(child);
        }
        if let Some(index) = run_to_export {
            let run = &self.saved_runs[index];
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(format!("{}-{}.zip", run.name, run.id.as_str()))
                .save_file()
            {
                let artifact_root = self
                    .project_root()
                    .map(|root| root.join(".forge/artifacts"))
                    .unwrap_or_default();
                self.console = export::experiment_bundle(run, &artifact_root, &path)
                    .map(|()| format!("Exported run bundle {}", path.display()))
                    .unwrap_or_else(|e| format!("Run export failed: {e}"));
            }
        }
        if let Some(index) = run_to_toggle_archive {
            self.saved_runs[index].archived = !self.saved_runs[index].archived;
            if let Some(store) = &self.workspace_store {
                let run = &self.saved_runs[index];
                let _ = store.save_experiment(&run.id, &run.name, run);
            }
        }
        if let Some(index) = run_to_delete {
            let run = self.saved_runs.remove(index);
            if let Some(store) = &self.workspace_store {
                if let Err(error) = store.delete_experiment(&run.id) {
                    self.console =
                        format!("Deleted run from the view, but storage failed: {error}");
                    return;
                }
            }
            self.console = format!("Deleted saved run `{}`.", run.name);
        }
    }

    pub(crate) fn charts(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_name)
                    .desired_width(110.0)
                    .hint_text("Run name"),
            );
            if ui.button("Save run").clicked() {
                self.save_experiment_run();
            }
            if ui.button("Export").clicked() {
                self.export_telemetry_csv();
            }
            if ui.button("Import plot JSON").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Forge plot JSON", &["json"])
                    .pick_file()
                {
                    self.console = match std::fs::read(&path)
                        .map_err(|error| error.to_string())
                        .and_then(|bytes| plot::parse_json(&bytes))
                    {
                        Ok(plots) => {
                            let count = plots.len();
                            for spec in plots {
                                if let Some(existing) = self
                                    .structured_plots
                                    .iter_mut()
                                    .find(|existing| existing.name == spec.name)
                                {
                                    *existing = spec;
                                } else {
                                    self.structured_plots.push(spec);
                                }
                            }
                            format!("Imported {count} plot(s) from {}", path.display())
                        }
                        Err(error) => format!("Plot import failed: {error}"),
                    };
                }
            }
            if !self.structured_plots.is_empty() && ui.button("Export plot history").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forge-plot-history.json")
                    .save_file()
                {
                    self.console = plot::collection_json(&self.structured_plots)
                        .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()))
                        .map(|()| format!("Exported plot history to {}", path.display()))
                        .unwrap_or_else(|error| format!("Plot history export failed: {error}"));
                }
            }
            if (self.data.has_telemetry() || !self.structured_plots.is_empty())
                && ui
                    .button("Clear current")
                    .on_hover_text("Clear all current datasets and plots")
                    .clicked()
            {
                self.data.metrics.clear();
                self.data.vectors.clear();
                self.structured_plots.clear();
                self.console = "Cleared current datasets and plots.".to_owned();
            }
        });
        self.structured_plot_viewer(ui);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_tags)
                    .hint_text("tags, comma separated"),
            );
            ui.add(egui::TextEdit::singleline(&mut self.experiment_notes).hint_text("run notes"));
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_github_issue)
                    .hint_text("GitHub issue URL"),
            );
            ui.add(egui::TextEdit::singleline(&mut self.experiment_github_pr).hint_text("PR URL"));
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_github_action)
                    .hint_text("Actions run URL"),
            );
        });
        if !self.data.has_telemetry() {
            ui.label(
                RichText::new(
                    "Emit `forge_metric:loss=0.42` or `forge_vector:weights=1,2,3` from a cell.",
                )
                .monospace()
                .size(10.0)
                .color(MUTED),
            );
        }
        let mut metric_to_delete = None;
        for (name, values) in &self.data.metrics {
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).strong().color(EMBER));
                if compact_icon_button(
                    ui,
                    egui_phosphor_icons::icons::TRASH,
                    "Delete this metric plot",
                )
                .clicked()
                {
                    metric_to_delete = Some(name.clone());
                }
            });
            Plot::new(format!("metric_{name}"))
                .height(175.0)
                .allow_drag(false)
                .show(ui, |plot| plot.line(metric_line(name, values, EMBER)));
        }
        if let Some(name) = metric_to_delete {
            self.data.metrics.remove(&name);
            self.console = format!("Deleted plot `{name}`.");
        }
        let mut vector_to_delete = None;
        for (name, values) in &self.data.vectors {
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).strong().color(accent()));
                if compact_icon_button(
                    ui,
                    egui_phosphor_icons::icons::TRASH,
                    "Delete this dataset and its plot",
                )
                .clicked()
                {
                    vector_to_delete = Some(name.clone());
                }
            });
            Plot::new(format!("vector_{name}"))
                .height(175.0)
                .allow_drag(false)
                .show(ui, |plot| {
                    plot.bar_chart(vector_bars(name, values, accent()))
                });
        }
        if let Some(name) = vector_to_delete {
            self.data.vectors.remove(&name);
            self.console = format!("Deleted dataset and plot `{name}`.");
        }
    }

    fn structured_plot_viewer(&mut self, ui: &mut egui::Ui) {
        if self.structured_plots.is_empty() {
            return;
        }
        ui.separator();
        ui.heading("Structured plots");
        let mut delete = None;
        let mut move_up = None;
        let mut move_down = None;
        let mut duplicate = None;
        let mut status = None;
        let plot_count = self.structured_plots.len();
        for (index, spec) in self.structured_plots.iter_mut().enumerate() {
            egui::CollapsingHeader::new(format!("{} · {}", spec.name, spec.kind.label()))
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut spec.x_log, "log X");
                        ui.checkbox(&mut spec.y_log, "log Y");
                        if ui
                            .add_enabled(index > 0, egui::Button::new("↑"))
                            .on_hover_text("Move plot earlier")
                            .clicked()
                        {
                            move_up = Some(index);
                        }
                        if ui
                            .add_enabled(index + 1 < plot_count, egui::Button::new("↓"))
                            .on_hover_text("Move plot later")
                            .clicked()
                        {
                            move_down = Some(index);
                        }
                        if ui.button("Duplicate").clicked() {
                            duplicate = Some(spec.clone());
                        }
                        if ui.button("Export JSON").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name(format!("{}.plot.json", safe_file_stem(&spec.name)))
                                .save_file()
                            {
                                status = Some(
                                    std::fs::write(
                                        &path,
                                        serde_json::to_vec_pretty(spec).unwrap_or_default(),
                                    )
                                    .map(|()| format!("Exported {}", path.display()))
                                    .unwrap_or_else(|e| e.to_string()),
                                );
                            }
                        }
                        if ui.button("Export SVG").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name(format!("{}.svg", safe_file_stem(&spec.name)))
                                .save_file()
                            {
                                status = Some(
                                    plot::svg(spec, 960, 540)
                                        .and_then(|svg| {
                                            std::fs::write(&path, svg).map_err(|e| e.to_string())
                                        })
                                        .map(|()| format!("Exported {}", path.display()))
                                        .unwrap_or_else(|e| e),
                                );
                            }
                        }
                        if ui.button("Export PNG").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name(format!("{}.png", safe_file_stem(&spec.name)))
                                .save_file()
                            {
                                status = Some(
                                    plot::png(spec, 960, 540)
                                        .and_then(|png| {
                                            std::fs::write(&path, png).map_err(|e| e.to_string())
                                        })
                                        .map(|()| format!("Exported {}", path.display()))
                                        .unwrap_or_else(|e| e),
                                );
                            }
                        }
                        if ui.button("Export HTML").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name(format!("{}.html", safe_file_stem(&spec.name)))
                                .save_file()
                            {
                                status = Some(
                                    plot::html(spec, 960, 540)
                                        .and_then(|html| {
                                            std::fs::write(&path, html).map_err(|e| e.to_string())
                                        })
                                        .map(|()| format!("Exported {}", path.display()))
                                        .unwrap_or_else(|e| e),
                                );
                            }
                        }
                        if ui.button("Export PDF").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name(format!("{}.pdf", safe_file_stem(&spec.name)))
                                .save_file()
                            {
                                status = Some(
                                    plot::pdf(spec, 960, 540)
                                        .and_then(|pdf| {
                                            std::fs::write(&path, pdf).map_err(|e| e.to_string())
                                        })
                                        .map(|()| format!("Exported {}", path.display()))
                                        .unwrap_or_else(|e| e),
                                );
                            }
                        }
                        if ui.button("Delete").clicked() {
                            delete = Some(index);
                        }
                    });
                    for series in &mut spec.series {
                        ui.checkbox(&mut series.visible, &series.name);
                    }
                    if spec.kind == PlotKind::Heatmap {
                        draw_heatmap(ui, &spec.matrix);
                        return;
                    }
                    if spec.kind == PlotKind::Box {
                        draw_box_summary(ui, spec);
                    }
                    Plot::new(format!("structured_plot_{index}"))
                        .height(260.0)
                        .show(ui, |plot_ui| {
                            for (series_index, series) in
                                spec.series.iter().filter(|s| s.visible).enumerate()
                            {
                                let points = transformed_points(series, spec.x_log, spec.y_log);
                                match spec.kind {
                                    PlotKind::Scatter | PlotKind::Residual => plot_ui.points(
                                        Points::new(&series.name, PlotPoints::from(points))
                                            .radius(3.0),
                                    ),
                                    PlotKind::Bar | PlotKind::FeatureImportance => {
                                        let bars = if !series.values.is_empty() {
                                            series
                                                .values
                                                .iter()
                                                .enumerate()
                                                .map(|(i, v)| Bar::new(i as f64, *v))
                                                .collect()
                                        } else {
                                            points.iter().map(|p| Bar::new(p[0], p[1])).collect()
                                        };
                                        plot_ui.bar_chart(BarChart::new(&series.name, bars));
                                    }
                                    PlotKind::Histogram => plot_ui.bar_chart(BarChart::new(
                                        &series.name,
                                        histogram(&series.values, 24),
                                    )),
                                    PlotKind::Area => plot_ui.line(
                                        Line::new(&series.name, PlotPoints::from(points))
                                            .fill(0.0)
                                            .fill_alpha(0.25),
                                    ),
                                    PlotKind::Box => {
                                        if let Some((min, q1, median, q3, max)) =
                                            quartiles(&series.values)
                                        {
                                            plot_ui.line(Line::new(
                                                &series.name,
                                                PlotPoints::from(vec![[0.0, min], [0.0, max]]),
                                            ));
                                            plot_ui.points(
                                                Points::new(
                                                    format!("{} quartiles", series.name),
                                                    PlotPoints::from(vec![
                                                        [-0.05, q1],
                                                        [0.0, median],
                                                        [0.05, q3],
                                                    ]),
                                                )
                                                .radius(5.0),
                                            );
                                        }
                                    }
                                    _ => plot_ui.line(
                                        Line::new(&series.name, PlotPoints::from(points))
                                            .width(if series_index == 0 { 2.5 } else { 1.5 }),
                                    ),
                                }
                            }
                        });
                    if !spec.x_label.is_empty() || !spec.y_label.is_empty() {
                        ui.label(format!("X: {}    Y: {}", spec.x_label, spec.y_label));
                    }
                });
        }
        if let Some(index) = delete {
            self.structured_plots.remove(index);
        }
        if let Some(index) = move_up {
            self.structured_plots.swap(index, index - 1);
        } else if let Some(index) = move_down {
            self.structured_plots.swap(index, index + 1);
        }
        if let Some(mut spec) = duplicate {
            let base = format!("{} copy", spec.name);
            spec.name = base.clone();
            let mut suffix = 2;
            while self
                .structured_plots
                .iter()
                .any(|existing| existing.name == spec.name)
            {
                spec.name = format!("{base} {suffix}");
                suffix += 1;
            }
            self.structured_plots.push(spec);
        }
        if let Some(message) = status {
            self.console = message;
        }
    }

    /// Body of one console-family dock pane (Rust console / history / Python).
    pub(crate) fn console_pane(&mut self, tab: ConsoleTab, ui: &mut egui::Ui) {
        ui.set_min_height(ui.available_height());
        ui.horizontal(|ui| {
            let (title, clear_hint) = match tab {
                ConsoleTab::Console => ("RUST CONSOLE", "Clear the visible console output"),
                ConsoleTab::History => ("HISTORY LOG", "Clear the command history"),
                ConsoleTab::Python => ("PYTHON RUNTIME", "Clear Python runtime output"),
            };
            ui.label(RichText::new(title).size(10.0).strong().color(MUTED));
            if tab == ConsoleTab::Console {
                if let Some(ms) = self
                    .cell_records
                    .get(&self.selected_cell)
                    .and_then(|r| r.elapsed_ms)
                {
                    ui.label(RichText::new(format!("{ms} ms")).size(10.0).color(accent()));
                }
            }
            if compact_icon_button(ui, egui_phosphor_icons::icons::BROOM, clear_hint).clicked() {
                match tab {
                    ConsoleTab::Console => {
                        // Cell runs keep their output separately from the shared console.
                        // Clear both stores so the selected cell's errors do not reappear.
                        self.console.clear();
                        if let Some(record) = self.cell_records.get_mut(&self.selected_cell) {
                            record.output.clear();
                            record.elapsed_ms = None;
                        }
                    }
                    ConsoleTab::History => self.history.clear(),
                    ConsoleTab::Python => {
                        self.python_console_output.clear();
                        self.python_mime_outputs.clear();
                    }
                }
            }
        });
        ui.separator();
        match tab {
            ConsoleTab::Console => {
                let shown = self
                    .cell_records
                    .get(&self.selected_cell)
                    .filter(|r| !r.output.is_empty())
                    .map(|r| r.output.as_str())
                    .unwrap_or(&self.console);
                egui::ScrollArea::vertical()
                    .id_salt("rust_console_output")
                    .max_height((ui.available_height() - 34.0).max(30.0))
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if let Some(record) = self.cell_records.get(&self.selected_cell) {
                            if let Some(commit) = &record.git_commit {
                                ui.label(
                                    RichText::new(format!(
                                        "Git: {commit}{} · {} MIME output(s)",
                                        if record.git_dirty { " + dirty" } else { "" },
                                        record.rich_outputs.len()
                                    ))
                                    .size(9.0)
                                    .color(MUTED),
                                );
                            }
                        }
                        ui.label(RichText::new(shown).monospace().color(
                            if self.run_state == RunState::Failed {
                                RED
                            } else {
                                TEXT
                            },
                        ));
                        if let Some(record) = self.cell_records.get(&self.selected_cell) {
                            for output in record
                                .rich_outputs
                                .iter()
                                .filter(|output| output.mime != "text/plain")
                            {
                                ui.collapsing(&output.mime, |ui| {
                                    ui.label(
                                        RichText::new(&output.data).monospace().color(accent()),
                                    );
                                });
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("In [ ]:")
                            .monospace()
                            .strong()
                            .color(accent()),
                    );
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.console_input)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .hint_text("Enter Rust expression..."),
                    );
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.run_console_input();
                    }
                    if ui.button("Run").clicked() {
                        self.run_console_input();
                    }
                });
            }
            ConsoleTab::History => {
                egui::ScrollArea::vertical()
                    .id_salt("rust_console_history")
                    .show(ui, |ui| {
                        for (index, command) in self.history.iter().enumerate() {
                            ui.label(
                                RichText::new(format!("In [{}]: {}", index + 1, command))
                                    .monospace()
                                    .color(TEXT),
                            );
                        }
                    });
            }
            ConsoleTab::Python => {
                ui.horizontal(|ui| {
                    if ui.button("Discover").clicked() {
                        self.discover_python_runtimes();
                    }
                    egui::ComboBox::from_id_salt("python_runtime_selector")
                        .selected_text(
                            self.selected_python
                                .as_ref()
                                .map(|path| path.display().to_string())
                                .unwrap_or_else(|| "No interpreter selected".into()),
                        )
                        .show_ui(ui, |ui| {
                            for runtime in &self.python_runtimes {
                                ui.selectable_value(
                                    &mut self.selected_python,
                                    Some(runtime.executable.clone()),
                                    format!(
                                        "{} · {}",
                                        runtime.version,
                                        runtime.executable.display()
                                    ),
                                );
                            }
                        });
                    if ui.button("Start/restart").clicked() {
                        self.start_python_kernel();
                    }
                    if ui.button("Create .venv").clicked() {
                        if let (Some(root), Some(runtime)) = (
                            self.project_root(),
                            self.python_runtimes.iter().find(|runtime| {
                                Some(&runtime.executable) == self.selected_python.as_ref()
                            }),
                        ) {
                            self.python_console_output =
                                python_runtime::create_venv(runtime, &root.join(".venv"))
                                    .unwrap_or_else(|e| e);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Jupyter kernels").clicked() {
                        self.jupyter_kernels = jupyter::discover().unwrap_or_default();
                    }
                    egui::ComboBox::from_id_salt("python_jupyter_kernel")
                        .selected_text(if self.selected_jupyter_kernel.is_empty() {
                            "No Jupyter kernel selected"
                        } else {
                            &self.selected_jupyter_kernel
                        })
                        .show_ui(ui, |ui| {
                            for kernel in &self.jupyter_kernels {
                                if kernel.language.eq_ignore_ascii_case("python") {
                                    ui.selectable_value(
                                        &mut self.selected_jupyter_kernel,
                                        kernel.name.clone(),
                                        &kernel.display_name,
                                    );
                                }
                            }
                        });
                });
                egui::ScrollArea::vertical()
                    .max_height((ui.available_height() - 62.0).max(40.0))
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new(&self.python_console_output).monospace());
                        for output in &self.python_mime_outputs {
                            ui.label(
                                RichText::new(format!("{}: {}", output.mime, output.data))
                                    .monospace()
                                    .color(accent()),
                            );
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Py [ ]:")
                            .monospace()
                            .strong()
                            .color(accent()),
                    );
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.python_console_input)
                            .desired_width(f32::INFINITY)
                            .hint_text("Python code (persistent session)"),
                    );
                    if (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        || ui.button("Run").clicked()
                    {
                        self.run_python_input();
                    }
                });
            }
        }
        ui.take_available_space();
    }
}
