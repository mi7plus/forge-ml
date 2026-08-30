//! Small plot-drawing helpers: axis transforms, histograms, box summaries, and
//! the confusion/heatmap grid.

use crate::plot::{self, PlotSpec};
use eframe::egui;
use egui::Color32;
use egui_plot::Bar;

pub fn transformed_points(series: &plot::PlotSeries, x_log: bool, y_log: bool) -> Vec<[f64; 2]> {
    let raw = if series.points.is_empty() {
        series
            .values
            .iter()
            .enumerate()
            .map(|(i, v)| [i as f64, *v])
            .collect::<Vec<_>>()
    } else {
        series.points.clone()
    };
    raw.into_iter()
        .filter_map(|[x, y]| {
            if (x_log && x <= 0.0) || (y_log && y <= 0.0) {
                None
            } else {
                Some([
                    if x_log { x.log10() } else { x },
                    if y_log { y.log10() } else { y },
                ])
            }
        })
        .collect()
}

pub fn histogram(values: &[f64], bins: usize) -> Vec<Bar> {
    if values.is_empty() {
        return Vec::new();
    }
    let min = values.iter().copied().reduce(f64::min).unwrap_or(0.0);
    let max = values.iter().copied().reduce(f64::max).unwrap_or(min);
    let width = ((max - min) / bins as f64).max(f64::EPSILON);
    let mut counts = vec![0usize; bins];
    for value in values {
        let index = (((*value - min) / width) as usize).min(bins - 1);
        counts[index] += 1;
    }
    counts
        .into_iter()
        .enumerate()
        .map(|(i, count)| Bar::new(min + (i as f64 + 0.5) * width, count as f64).width(width))
        .collect()
}

pub fn quartiles(values: &[f64]) -> Option<(f64, f64, f64, f64, f64)> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let at = |fraction: f64| values[((values.len() - 1) as f64 * fraction).round() as usize];
    Some((
        values[0],
        at(0.25),
        at(0.5),
        at(0.75),
        *values.last().unwrap(),
    ))
}

pub fn draw_box_summary(ui: &mut egui::Ui, spec: &PlotSpec) {
    egui::Grid::new(format!("box_summary_{}", spec.name))
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Series");
            for label in ["Min", "Q1", "Median", "Q3", "Max"] {
                ui.strong(label);
            }
            ui.end_row();
            for series in spec.series.iter().filter(|s| s.visible) {
                if let Some((min, q1, median, q3, max)) = quartiles(&series.values) {
                    ui.label(&series.name);
                    for value in [min, q1, median, q3, max] {
                        ui.label(format!("{value:.4}"));
                    }
                    ui.end_row();
                }
            }
        });
}

pub fn draw_heatmap(ui: &mut egui::Ui, matrix: &[Vec<f64>]) {
    let values = matrix.iter().flatten().copied().collect::<Vec<_>>();
    let min = values.iter().copied().reduce(f64::min).unwrap_or(0.0);
    let max = values.iter().copied().reduce(f64::max).unwrap_or(min);
    egui::ScrollArea::both().max_height(320.0).show(ui, |ui| {
        egui::Grid::new("structured_heatmap")
            .spacing([2.0, 2.0])
            .show(ui, |ui| {
                for row in matrix {
                    for value in row {
                        let t =
                            ((*value - min) / (max - min).max(f64::EPSILON)).clamp(0.0, 1.0) as f32;
                        let color = Color32::from_rgb(
                            (30.0 + 210.0 * t) as u8,
                            (50.0 + 80.0 * (1.0 - t)) as u8,
                            (220.0 - 180.0 * t) as u8,
                        );
                        ui.colored_label(color, format!("{value:.2}"));
                    }
                    ui.end_row();
                }
            });
    });
}
