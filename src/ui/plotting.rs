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

/// A compact, self-contained renderer for one plot spec, used inline in the
/// Notebook pane (the Plots pane keeps its own full-fidelity viewer with
/// per-plot controls). `id_salt` must be unique per rendered plot.
pub fn draw_inline_plot(ui: &mut egui::Ui, spec: &PlotSpec, id_salt: &str) {
    use crate::plot::PlotKind;
    use egui_plot::{BarChart, Line, Plot, PlotPoints, Points};

    match spec.kind {
        // Heatmaps need a scrolling grid with a stable id; keep them in the
        // Plots pane rather than risk id collisions between cell cards.
        PlotKind::Heatmap => {
            ui.label(
                egui::RichText::new("heatmap — open in the Plots pane")
                    .size(10.0)
                    .italics()
                    .color(Color32::GRAY),
            );
            return;
        }
        // The box summary table is informative and has a per-name id.
        PlotKind::Box => {
            draw_box_summary(ui, spec);
            return;
        }
        _ => {}
    }
    Plot::new(id_salt)
        .height(180.0)
        .allow_scroll(true)
        .allow_drag(true)
        .allow_zoom(true)
        .auto_bounds(true)
        .show(ui, |plot_ui| {
            for series in spec.series.iter().filter(|s| s.visible) {
                let points = transformed_points(series, spec.x_log, spec.y_log);
                match spec.kind {
                    PlotKind::Scatter | PlotKind::Residual => plot_ui
                        .points(Points::new(&series.name, PlotPoints::from(points)).radius(2.5)),
                    PlotKind::Bar | PlotKind::FeatureImportance => {
                        let bars = if series.values.is_empty() {
                            points.iter().map(|p| Bar::new(p[0], p[1])).collect()
                        } else {
                            series
                                .values
                                .iter()
                                .enumerate()
                                .map(|(i, v)| Bar::new(i as f64, *v))
                                .collect()
                        };
                        plot_ui.bar_chart(BarChart::new(&series.name, bars));
                    }
                    PlotKind::Histogram => plot_ui
                        .bar_chart(BarChart::new(&series.name, histogram(&series.values, 24))),
                    PlotKind::Area => plot_ui.line(
                        Line::new(&series.name, PlotPoints::from(points))
                            .fill(0.0)
                            .fill_alpha(0.25),
                    ),
                    PlotKind::Ecdf => plot_ui.line(
                        Line::new(&series.name, PlotPoints::from(ecdf(&series.values))).width(2.0),
                    ),
                    PlotKind::Violin => {
                        // Density-vs-value profile (a compact stand-in for the
                        // Plots pane's mirrored violin).
                        let profile: Vec<[f64; 2]> = kde(&series.values, 48)
                            .iter()
                            .map(|d| [d[1], d[0]])
                            .collect();
                        plot_ui.line(Line::new(&series.name, PlotPoints::from(profile)));
                    }
                    _ => plot_ui.line(Line::new(&series.name, PlotPoints::from(points)).width(2.0)),
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    // These helpers all do unguarded float arithmetic on user data, so the cases
    // that matter are the degenerate ones: empty, single-valued, and all-equal
    // inputs (zero range / zero variance). None may panic or produce NaN/inf.

    #[test]
    fn histogram_handles_empty_and_constant_input() {
        assert!(histogram(&[], 10).is_empty());
        // All-equal values: range is zero, so width falls back to EPSILON and
        // every value lands in the first bin without an out-of-range index.
        let bars = histogram(&[5.0, 5.0, 5.0], 8);
        let total: f64 = bars.iter().map(|b| b.value).sum();
        assert_eq!(total, 3.0);
        assert!(bars
            .iter()
            .all(|b| b.value.is_finite() && b.bar_width.is_finite()));
    }

    #[test]
    fn quartiles_degenerate_inputs() {
        assert!(quartiles(&[]).is_none());
        let (min, q1, median, q3, max) = quartiles(&[7.0]).unwrap();
        assert_eq!([min, q1, median, q3, max], [7.0; 5]);
        // Ordering invariant holds regardless of input order.
        let (min, q1, median, q3, max) = quartiles(&[9.0, 1.0, 5.0, 3.0, 7.0]).unwrap();
        assert!(min <= q1 && q1 <= median && median <= q3 && q3 <= max);
    }

    #[test]
    fn box_stats_constant_group_has_no_outliers() {
        assert!(box_stats(&[]).is_none());
        let stats = box_stats(&[4.0, 4.0, 4.0, 4.0]).unwrap();
        assert!(stats.outliers.is_empty());
        assert_eq!(stats.whisker_lo, 4.0);
        assert_eq!(stats.whisker_hi, 4.0);
        // A far value beyond 1.5·IQR is flagged as an outlier.
        let stats = box_stats(&[1.0, 2.0, 3.0, 4.0, 100.0]).unwrap();
        assert!(stats.outliers.contains(&100.0));
    }

    #[test]
    fn kde_is_finite_even_with_zero_variance() {
        assert!(kde(&[1.0], 32).is_empty()); // needs at least two points
        let curve = kde(&[3.0, 3.0, 3.0], 16);
        assert_eq!(curve.len(), 16);
        assert!(curve
            .iter()
            .all(|[y, d]| y.is_finite() && d.is_finite() && *d >= 0.0));
    }

    #[test]
    fn ecdf_is_monotonic_and_reaches_one() {
        assert!(ecdf(&[]).is_empty());
        let points = ecdf(&[3.0, 1.0, 2.0]);
        let fractions: Vec<f64> = points.iter().map(|p| p[1]).collect();
        assert!(fractions.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(points.last().unwrap()[1], 1.0);
    }

    #[test]
    fn clip_outliers_is_a_noop_below_the_threshold() {
        let small = vec![[0.0, 0.0], [1.0, 1.0], [1000.0, 1000.0]];
        assert_eq!(clip_outliers(&small), small);
        // With enough points, an extreme pair is dropped but the bulk survives.
        let mut many: Vec<[f64; 2]> = (0..100).map(|i| [i as f64, i as f64]).collect();
        many.push([1e9, 1e9]);
        let clipped = clip_outliers(&many);
        assert!(clipped.len() < many.len());
        assert!(!clipped.contains(&[1e9, 1e9]));
    }

    /// Headless render smoke test: drive the actual egui layout pass (no window,
    /// no GPU) over our plotting widgets so a panic in the drawing code — an
    /// out-of-range index, a bad color cast — is caught in CI, not at runtime.
    #[test]
    fn plotting_widgets_render_headlessly() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            draw_heatmap(ui, &[vec![1.0, 2.0], vec![3.0, 4.0]]);
            let spec = PlotSpec {
                version: plot::PLOT_SPEC_VERSION,
                name: "smoke".into(),
                kind: plot::PlotKind::Box,
                x_label: String::new(),
                y_label: String::new(),
                series: vec![plot::PlotSeries {
                    name: "a".into(),
                    points: vec![],
                    values: vec![1.0, 2.0, 3.0, 4.0, 5.0],
                    visible: true,
                }],
                matrix: vec![],
                x_log: false,
                y_log: false,
            };
            draw_box_summary(ui, &spec);
            // The inline renderer (Notebook pane) across representative kinds.
            for kind in [
                plot::PlotKind::Line,
                plot::PlotKind::Scatter,
                plot::PlotKind::Bar,
                plot::PlotKind::Histogram,
                plot::PlotKind::Area,
                plot::PlotKind::Ecdf,
                plot::PlotKind::Violin,
                plot::PlotKind::Heatmap,
            ] {
                let spec = PlotSpec {
                    version: plot::PLOT_SPEC_VERSION,
                    name: format!("k_{kind:?}"),
                    kind,
                    x_label: String::new(),
                    y_label: String::new(),
                    series: vec![plot::PlotSeries {
                        name: "s".into(),
                        points: vec![[0.0, 1.0], [1.0, 2.0], [2.0, 1.5]],
                        values: vec![1.0, 2.0, 3.0, 4.0, 5.0],
                        visible: true,
                    }],
                    matrix: vec![vec![1.0, 2.0], vec![3.0, 4.0]],
                    x_log: false,
                    y_log: false,
                };
                draw_inline_plot(ui, &spec, &format!("inline_{kind:?}"));
            }
        });
        // egui insists texture deltas are consumed before the output is dropped.
        output.textures_delta.clear();
    }

    #[test]
    fn transformed_points_drops_nonpositive_under_log() {
        let series = plot::PlotSeries {
            name: "s".into(),
            values: vec![],
            points: vec![[-1.0, 10.0], [10.0, -1.0], [10.0, 100.0]],
            visible: true,
        };
        let out = transformed_points(&series, true, true);
        assert_eq!(out.len(), 1);
        assert!(out[0].iter().all(|v| v.is_finite()));
    }
}
