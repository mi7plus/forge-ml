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

/// Drop points outside the central 1st–99th percentile on either axis, so a few
/// far outliers don't force the auto-fit to zoom out and squash the bulk of the
/// data. A no-op for small sets (nothing meaningful to clip).
pub fn clip_outliers(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    if points.len() < 20 {
        return points.to_vec();
    }
    let percentile = |source: &[f64], fraction: f64| {
        let mut sorted: Vec<f64> = source.to_vec();
        sorted.sort_by(f64::total_cmp);
        sorted[((sorted.len() - 1) as f64 * fraction).round() as usize]
    };
    let xs: Vec<f64> = points.iter().map(|p| p[0]).collect();
    let ys: Vec<f64> = points.iter().map(|p| p[1]).collect();
    let (x_lo, x_hi) = (percentile(&xs, 0.01), percentile(&xs, 0.99));
    let (y_lo, y_hi) = (percentile(&ys, 0.01), percentile(&ys, 0.99));
    points
        .iter()
        .copied()
        .filter(|[x, y]| *x >= x_lo && *x <= x_hi && *y >= y_lo && *y <= y_hi)
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

/// Tukey box-and-whisker statistics for one group of values.
pub struct BoxStats {
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
    /// Whisker ends: the most extreme values within 1.5·IQR of the box.
    pub whisker_lo: f64,
    pub whisker_hi: f64,
    /// Points beyond the whiskers.
    pub outliers: Vec<f64>,
}

pub fn box_stats(values: &[f64]) -> Option<BoxStats> {
    let (_, q1, median, q3, _) = quartiles(values)?;
    let iqr = q3 - q1;
    let (lo_fence, hi_fence) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
    let mut whisker_lo = q1;
    let mut whisker_hi = q3;
    let mut outliers = Vec::new();
    for &v in values {
        if v < lo_fence || v > hi_fence {
            outliers.push(v);
        } else {
            whisker_lo = whisker_lo.min(v);
            whisker_hi = whisker_hi.max(v);
        }
    }
    Some(BoxStats {
        q1,
        median,
        q3,
        whisker_lo,
        whisker_hi,
        outliers,
    })
}

/// A Gaussian kernel-density estimate sampled at `samples` points across the
/// data range (Silverman's rule-of-thumb bandwidth). Returns `[value, density]`
/// pairs — the profile a violin plot mirrors.
pub fn kde(values: &[f64], samples: usize) -> Vec<[f64; 2]> {
    let n = values.len();
    if n < 2 || samples < 2 {
        return Vec::new();
    }
    let mean = values.iter().sum::<f64>() / n as f64;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
    let std = variance.sqrt();
    let bandwidth = (1.06 * std * (n as f64).powf(-0.2)).max(1e-6);
    let lo = values.iter().copied().fold(f64::INFINITY, f64::min) - bandwidth;
    let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max) + bandwidth;
    let scale = 1.0 / (n as f64 * bandwidth * (2.0 * std::f64::consts::PI).sqrt());
    (0..samples)
        .map(|i| {
            let y = lo + (hi - lo) * i as f64 / (samples - 1) as f64;
            let density = scale
                * values
                    .iter()
                    .map(|&x| (-0.5 * ((y - x) / bandwidth).powi(2)).exp())
                    .sum::<f64>();
            [y, density]
        })
        .collect()
}

/// The empirical cumulative distribution as step points: for each distinct value
/// the fraction of the sample at or below it.
pub fn ecdf(values: &[f64]) -> Vec<[f64; 2]> {
    if values.is_empty() {
        return Vec::new();
    }
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len() as f64;
    let mut points = Vec::with_capacity(sorted.len() * 2);
    let mut prior = 0.0;
    for (index, &value) in sorted.iter().enumerate() {
        let fraction = (index + 1) as f64 / n;
        points.push([value, prior]); // step up at this value
        points.push([value, fraction]);
        prior = fraction;
    }
    points
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
