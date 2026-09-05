//! Workspace panes: project search results, the experiments/runs board,
//! the charts and structured-plot viewers, and the console pane. Methods on the
//! shared [`crate::ForgeApp`].

use crate::result_ext::ResultText;
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
        // `horizontal_wrapped` so the run/export controls flow onto the next line
        // on a narrow pane instead of overflowing off the right edge (the pane is
        // a vertical-only scroll area, so an overflowing row would be unreachable).
        ui.horizontal_wrapped(|ui| {
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
            if !self.structured_plots.is_empty()
                && ui
                    .button("Clear plots")
                    .on_hover_text("Remove all plots (keeps metric/vector datasets)")
                    .clicked()
            {
                let count = self.structured_plots.len();
                self.structured_plots.clear();
                self.console = format!("Cleared {count} plot(s).");
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
        // Explicit widths + wrapping: a bare singleline TextEdit expands to fill
        // the row, so two/three side by side would push each other off a narrow
        // pane. Fixed widths let them sit together when wide and wrap when narrow.
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_tags)
                    .desired_width(180.0)
                    .hint_text("tags, comma separated"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_notes)
                    .desired_width(220.0)
                    .hint_text("run notes"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_github_issue)
                    .desired_width(180.0)
                    .hint_text("GitHub issue URL"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_github_pr)
                    .desired_width(160.0)
                    .hint_text("PR URL"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_github_action)
                    .desired_width(180.0)
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
                    // Outlier clipping is a per-plot UI toggle kept in egui memory
                    // (ephemeral, so PlotSpec stays a pure data model).
                    let clip_id = ui.make_persistent_id(("clip_outliers", index));
                    let mut clip_outliers_on =
                        ui.data(|d| d.get_temp::<bool>(clip_id).unwrap_or(false));
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut spec.x_log, "log X");
                        ui.checkbox(&mut spec.y_log, "log Y");
                        ui.checkbox(&mut clip_outliers_on, "Hide outliers")
                            .on_hover_text(
                                "Drop the extreme 1% on each axis so the view fits the bulk",
                            );
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
                                        .text(),
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
                                        .text(),
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
                                        .text(),
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
                                        .text(),
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
                    ui.data_mut(|d| d.insert_temp(clip_id, clip_outliers_on));
                    ui.label(
                        RichText::new("scroll = zoom · drag = pan · double-click = reset")
                            .size(9.0)
                            .color(MUTED),
                    );
                    if spec.kind == PlotKind::Heatmap {
                        draw_heatmap(ui, &spec.matrix);
                        return;
                    }
                    if spec.kind == PlotKind::Box {
                        draw_box_summary(ui, spec);
                    }
                    Plot::new(format!("structured_plot_{index}"))
                        .height(260.0)
                        .allow_zoom(true)
                        .allow_drag(true)
                        .allow_scroll(true)
                        .allow_boxed_zoom(true)
                        .auto_bounds(true)
                        .show(ui, |plot_ui| {
                            for (series_index, series) in
                                spec.series.iter().filter(|s| s.visible).enumerate()
                            {
                                let mut points = transformed_points(series, spec.x_log, spec.y_log);
                                if clip_outliers_on {
                                    points = crate::ui::plotting::clip_outliers(&points);
                                }
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
                                        // Tukey box-and-whisker, one box per series
                                        // at its own x offset so groups sit side by side.
                                        if let Some(stats) = box_stats(&series.values) {
                                            let x = series_index as f64;
                                            let (half, cap) = (0.28, 0.14);
                                            plot_ui.polygon(Polygon::new(
                                                &series.name,
                                                PlotPoints::from(vec![
                                                    [x - half, stats.q1],
                                                    [x + half, stats.q1],
                                                    [x + half, stats.q3],
                                                    [x - half, stats.q3],
                                                ]),
                                            ));
                                            plot_ui.line(
                                                Line::new(
                                                    &series.name,
                                                    PlotPoints::from(vec![
                                                        [x - half, stats.median],
                                                        [x + half, stats.median],
                                                    ]),
                                                )
                                                .width(2.0),
                                            );
                                            for (a, b) in [
                                                ([x, stats.q3], [x, stats.whisker_hi]),
                                                ([x, stats.q1], [x, stats.whisker_lo]),
                                                (
                                                    [x - cap, stats.whisker_hi],
                                                    [x + cap, stats.whisker_hi],
                                                ),
                                                (
                                                    [x - cap, stats.whisker_lo],
                                                    [x + cap, stats.whisker_lo],
                                                ),
                                            ] {
                                                plot_ui.line(Line::new(
                                                    &series.name,
                                                    PlotPoints::from(vec![a, b]),
                                                ));
                                            }
                                            if !stats.outliers.is_empty() {
                                                plot_ui.points(
                                                    Points::new(
                                                        format!("{} outliers", series.name),
                                                        PlotPoints::from(
                                                            stats
                                                                .outliers
                                                                .iter()
                                                                .map(|&o| [x, o])
                                                                .collect::<Vec<_>>(),
                                                        ),
                                                    )
                                                    .radius(2.5),
                                                );
                                            }
                                        }
                                    }
                                    PlotKind::Violin => {
                                        // Mirrored KDE profile per group at its x offset.
                                        let density = kde(&series.values, 64);
                                        if !density.is_empty() {
                                            let x = series_index as f64;
                                            let max_d = density
                                                .iter()
                                                .map(|d| d[1])
                                                .fold(0.0_f64, f64::max)
                                                .max(1e-9);
                                            let half = 0.42;
                                            let mut poly: Vec<[f64; 2]> =
                                                Vec::with_capacity(density.len() * 2);
                                            for &[y, d] in &density {
                                                poly.push([x + d / max_d * half, y]);
                                            }
                                            for &[y, d] in density.iter().rev() {
                                                poly.push([x - d / max_d * half, y]);
                                            }
                                            plot_ui.polygon(Polygon::new(
                                                &series.name,
                                                PlotPoints::from(poly),
                                            ));
                                            if let Some((_, _, median, _, _)) =
                                                quartiles(&series.values)
                                            {
                                                plot_ui.line(Line::new(
                                                    &series.name,
                                                    PlotPoints::from(vec![
                                                        [x - half * 0.5, median],
                                                        [x + half * 0.5, median],
                                                    ]),
                                                ));
                                            }
                                        }
                                    }
                                    PlotKind::Ecdf => plot_ui.line(
                                        Line::new(
                                            &series.name,
                                            PlotPoints::from(ecdf(&series.values)),
                                        )
                                        .width(2.0),
                                    ),
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

    /// The Notebook pane: every cell stacked with its captured output (text,
    /// rich MIME, and the plots it produced) rendered inline, Jupyter-style.
    pub(crate) fn notebook_pane(&mut self, ui: &mut egui::Ui) {
        use egui_phosphor_icons::icons;
        let cells = self.cells();
        if cells.is_empty() {
            crate::ui::theme::empty_state(
                ui,
                icons::NOTEBOOK,
                "No cells",
                "Cells are separated by `//# %% name`. Run one to see its output here.",
            );
            return;
        }
        let mut run_cell: Option<usize> = None;
        let mut run_all = false;
        let mut select: Option<usize> = None;
        let mut open_dataset: Option<String> = None;
        // In-place editing works on each cell's raw byte range (marker + body),
        // so a save round-trips exactly. Take the draft out of `self` for the
        // duration so the editable TextEdit doesn't clash with the immutable
        // reads of `cell_records`/`data` inside the list.
        let content = self.active().content.clone();
        let raw_ranges: Vec<String> = crate::notebook::cell_byte_ranges(&content)
            .into_iter()
            .map(|range| content.get(range).unwrap_or_default().to_owned())
            .collect();
        let mut edit = self.notebook_edit.take();
        let mut save: Option<(usize, String)> = None;
        let mut cancel_edit = false;
        let mut swap_first: Option<usize> = None;
        let mut toggle_collapse: Option<usize> = None;
        let cell_count = cells.len();
        ui.horizontal(|ui| {
            ui.label(RichText::new("NOTEBOOK").size(10.0).strong().color(MUTED));
            run_all = ui
                .small_button("Run all")
                .on_hover_text("Run every cell top to bottom")
                .clicked();
            ui.label(
                RichText::new(format!("{} cell(s)", cells.len()))
                    .size(10.0)
                    .color(MUTED),
            );
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("notebook_view")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, (title, source)) in cells.iter().enumerate() {
                    let record = self.cell_records.get(&index);
                    let state = record.and_then(|r| r.state).unwrap_or(CellState::Idle);
                    let (mark, color) = match state {
                        CellState::Queued => ("[q]", MUTED),
                        CellState::Running => ("[>]", EMBER),
                        CellState::Passed => ("[ok]", GREEN),
                        CellState::Failed => ("[!]", RED),
                        CellState::Idle => ("[ ]", MUTED),
                    };
                    let collapsed = self.notebook_collapsed.contains(&index);
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .small_button(if collapsed { "▸" } else { "▾" })
                                .on_hover_text(if collapsed {
                                    "Expand cell"
                                } else {
                                    "Collapse cell"
                                })
                                .clicked()
                            {
                                toggle_collapse = Some(index);
                            }
                            ui.label(RichText::new(mark).monospace().color(color));
                            if ui
                                .selectable_label(
                                    index == self.selected_cell,
                                    RichText::new(format!("{:02}  {title}", index + 1))
                                        .strong()
                                        .color(if index == self.selected_cell {
                                            accent()
                                        } else {
                                            TEXT
                                        }),
                                )
                                .on_hover_text("Select and reveal in the editor")
                                .clicked()
                            {
                                select = Some(index);
                            }
                            if let Some(ms) = record.and_then(|r| r.elapsed_ms) {
                                ui.label(
                                    RichText::new(format!("{ms} ms")).size(10.0).color(accent()),
                                );
                            }
                            if ui.small_button("Run").clicked() {
                                run_cell = Some(index);
                            }
                            let editing_this = matches!(&edit, Some((i, _)) if *i == index);
                            if !editing_this
                                && edit.is_none()
                                && ui
                                    .small_button("Edit")
                                    .on_hover_text("Edit this cell's source in place")
                                    .clicked()
                            {
                                edit = Some((
                                    index,
                                    raw_ranges.get(index).cloned().unwrap_or_default(),
                                ));
                            }
                            if ui
                                .add_enabled(index > 0, egui::Button::new("↑").small())
                                .on_hover_text("Move cell up")
                                .clicked()
                            {
                                swap_first = Some(index - 1);
                            }
                            if ui
                                .add_enabled(index + 1 < cell_count, egui::Button::new("↓").small())
                                .on_hover_text("Move cell down")
                                .clicked()
                            {
                                swap_first = Some(index);
                            }
                        });
                        let editing_this = matches!(&edit, Some((i, _)) if *i == index);
                        if collapsed {
                            // Collapsed: header only.
                        } else if editing_this {
                            if let Some((_, draft)) = &mut edit {
                                ui.add(
                                    egui::TextEdit::multiline(draft)
                                        .code_editor()
                                        .desired_rows(4)
                                        .desired_width(f32::INFINITY),
                                );
                                ui.horizontal(|ui| {
                                    if ui.button("Save").clicked() {
                                        save = Some((index, draft.clone()));
                                    }
                                    if ui.button("Cancel").clicked() {
                                        cancel_edit = true;
                                    }
                                    ui.label(
                                        RichText::new("includes the //# %% header")
                                            .size(9.0)
                                            .color(MUTED),
                                    );
                                });
                            }
                        } else if !source.trim().is_empty() {
                            // Source (read-only), scrolled horizontally so long lines
                            // don't force the whole pane wide.
                            egui::ScrollArea::horizontal()
                                .id_salt(("nb_src", index))
                                .max_height(160.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(source.trim_end()).monospace().size(11.0),
                                        )
                                        .wrap(),
                                    );
                                });
                        }
                        if let Some(record) = record.filter(|_| !collapsed) {
                            if !record.output.is_empty() {
                                ui.separator();
                                ui.label(
                                    RichText::new(&record.output).monospace().size(11.0).color(
                                        if state == CellState::Failed {
                                            RED
                                        } else {
                                            TEXT
                                        },
                                    ),
                                );
                            }
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
                            // Plots this cell emitted, rendered inline.
                            for name in &record.plots {
                                if let Some(spec) =
                                    self.structured_plots.iter().find(|s| &s.name == name)
                                {
                                    ui.add_space(2.0);
                                    ui.label(RichText::new(name).size(10.0).color(MUTED));
                                    crate::ui::plotting::draw_inline_plot(
                                        ui,
                                        spec,
                                        &format!("nb_plot_{index}_{name}"),
                                    );
                                }
                            }
                            // Datasets this cell emitted, as a compact preview.
                            for dataset_ref in &record.datasets {
                                if let Some((kind, name)) = dataset_ref.split_once(':') {
                                    ui.add_space(2.0);
                                    if kind == "table" {
                                        if let Some(data) = self.data.tables.get(name) {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{name} — {} rows × {} cols",
                                                    data.rows.len(),
                                                    data.columns.len()
                                                ))
                                                .size(10.0)
                                                .color(MUTED),
                                            );
                                            draw_table_preview(
                                                ui,
                                                &data.columns,
                                                &data.rows,
                                                ("nb_tbl", index, name),
                                            );
                                        }
                                    } else if let Some(values) = self.data.vectors.get(name) {
                                        let preview = values
                                            .iter()
                                            .take(12)
                                            .map(|v| format!("{v:.4}"))
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        ui.label(
                                            RichText::new(format!(
                                                "{name} — {} value(s): [{preview}{}]",
                                                values.len(),
                                                if values.len() > 12 { ", …" } else { "" }
                                            ))
                                            .size(10.0)
                                            .monospace()
                                            .color(MUTED),
                                        );
                                    }
                                    if ui.small_button("Open in Data viewer").clicked() {
                                        open_dataset = Some(dataset_ref.clone());
                                    }
                                }
                            }
                        }
                    });
                    ui.add_space(6.0);
                }
            });
        // Resolve the in-place edit: save writes the draft back over the cell's
        // raw byte range; cancel discards; otherwise carry the draft forward.
        if let Some((index, draft)) = save {
            let current = self.active().content.clone();
            let ranges = crate::notebook::cell_byte_ranges(&current);
            if let Some(range) = ranges.get(index).cloned() {
                let mut updated = current.clone();
                updated.replace_range(range, &draft);
                self.active_mut().content = updated;
                self.active_mut().dirty = true;
                self.cell_records.clear();
                self.console = format!("Edited cell {} in the notebook.", index + 1);
            }
            self.notebook_edit = None;
        } else if cancel_edit {
            self.notebook_edit = None;
        } else {
            self.notebook_edit = edit;
        }
        if let Some(index) = toggle_collapse {
            if !self.notebook_collapsed.remove(&index) {
                self.notebook_collapsed.insert(index);
            }
        }
        // Reorder: swap cell `first` with the one below it in the buffer. Cell
        // outputs are keyed by index, so clear them (and collapse state) rather
        // than mis-attribute; keep the selection pointed at the moved cell.
        if let Some(first) = swap_first {
            if let Some(updated) =
                crate::notebook::swap_adjacent_cells(&self.active().content, first)
            {
                self.active_mut().content = updated;
                self.active_mut().dirty = true;
                self.cell_records.clear();
                self.notebook_collapsed.clear();
                if self.selected_cell == first {
                    self.selected_cell = first + 1;
                } else if self.selected_cell == first + 1 {
                    self.selected_cell = first;
                }
                self.console = "Reordered notebook cells.".to_owned();
            }
        }
        if let Some(index) = select {
            self.selected_cell = index;
            self.focus_cell_in_editor(index);
        }
        if let Some(dataset_ref) = open_dataset {
            self.open_dataset = Some(dataset_ref);
            self.inspector_tab = InspectorTab::Data;
        }
        if run_all {
            self.enqueue_cells(0..self.cells().len());
        } else if let Some(index) = run_cell {
            self.enqueue_cells([index]);
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
                                python_runtime::create_venv(runtime, &root.join(".venv")).text();
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

/// A compact read-only preview of a table's first rows, for the Notebook pane.
fn draw_table_preview(
    ui: &mut egui::Ui,
    columns: &[String],
    rows: &[Vec<String>],
    id_salt: impl std::hash::Hash + std::fmt::Debug,
) {
    const MAX_ROWS: usize = 5;
    const MAX_COLS: usize = 10;
    let cols = columns.len().min(MAX_COLS);
    egui::ScrollArea::horizontal()
        .id_salt(id_salt)
        .max_height(150.0)
        .show(ui, |ui| {
            egui::Grid::new("nb_table_grid")
                .striped(true)
                .spacing([10.0, 2.0])
                .show(ui, |ui| {
                    for column in columns.iter().take(cols) {
                        ui.label(RichText::new(column).strong().size(11.0).color(accent()));
                    }
                    if columns.len() > cols {
                        ui.label(RichText::new("…").color(MUTED));
                    }
                    ui.end_row();
                    for row in rows.iter().take(MAX_ROWS) {
                        for cell in row.iter().take(cols) {
                            ui.label(RichText::new(cell).monospace().size(11.0));
                        }
                        ui.end_row();
                    }
                });
            if rows.len() > MAX_ROWS {
                ui.label(
                    RichText::new(format!("… {} more row(s)", rows.len() - MAX_ROWS))
                        .size(10.0)
                        .color(MUTED),
                );
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_preview_renders_headlessly() {
        // Empty, small, and over-limit tables must all render without panicking.
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            draw_table_preview(ui, &[], &[], "empty");
            let cols: Vec<String> = (0..15).map(|i| format!("c{i}")).collect();
            let rows: Vec<Vec<String>> = (0..20)
                .map(|r| (0..15).map(|c| format!("{r}.{c}")).collect())
                .collect();
            draw_table_preview(ui, &cols, &rows, "big");
        });
        output.textures_delta.clear();
    }
}
