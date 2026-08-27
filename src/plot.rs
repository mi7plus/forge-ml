use egui::Color32;
use egui_plot::{Bar, BarChart, Line, PlotPoints};
use serde::{Deserialize, Serialize};

pub const PLOT_SPEC_VERSION: u16 = 1;
const MAX_POINTS: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlotKind {
    Line,
    Scatter,
    Bar,
    Area,
    Histogram,
    Box,
    Heatmap,
    Roc,
    PrecisionRecall,
    Residual,
    FeatureImportance,
}
impl PlotKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::Scatter => "Scatter",
            Self::Bar => "Bar",
            Self::Area => "Area",
            Self::Histogram => "Histogram",
            Self::Box => "Box",
            Self::Heatmap => "Heatmap",
            Self::Roc => "ROC",
            Self::PrecisionRecall => "Precision–recall",
            Self::Residual => "Residual",
            Self::FeatureImportance => "Feature importance",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlotSeries {
    pub name: String,
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
    #[serde(default)]
    pub values: Vec<f64>,
    #[serde(default = "default_true")]
    pub visible: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlotSpec {
    pub version: u16,
    pub name: String,
    pub kind: PlotKind,
    #[serde(default)]
    pub x_label: String,
    #[serde(default)]
    pub y_label: String,
    #[serde(default)]
    pub series: Vec<PlotSeries>,
    #[serde(default)]
    pub matrix: Vec<Vec<f64>>,
    #[serde(default)]
    pub x_log: bool,
    #[serde(default)]
    pub y_log: bool,
}
fn default_true() -> bool {
    true
}

impl PlotSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != PLOT_SPEC_VERSION {
            return Err(format!(
                "Unsupported plot specification version {}",
                self.version
            ));
        }
        if self.name.trim().is_empty() || self.series.len() > 128 {
            return Err("Plots require a name and at most 128 series".into());
        }
        let count = self
            .series
            .iter()
            .map(|s| s.points.len() + s.values.len())
            .sum::<usize>();
        if count > MAX_POINTS {
            return Err(format!("Plot exceeds the {MAX_POINTS}-value safety limit"));
        }
        if self
            .series
            .iter()
            .flat_map(|s| {
                s.points
                    .iter()
                    .flat_map(|p| p.iter())
                    .chain(s.values.iter())
            })
            .any(|v| !v.is_finite())
        {
            return Err("Plot values must be finite".into());
        }
        if self.matrix.len() > 512
            || self
                .matrix
                .iter()
                .any(|row| row.len() > 512 || row.iter().any(|v| !v.is_finite()))
        {
            return Err("Heatmaps are limited to 512×512 finite values".into());
        }
        if self.kind == PlotKind::Heatmap
            && (self.matrix.is_empty()
                || self
                    .matrix
                    .iter()
                    .any(|row| row.len() != self.matrix[0].len()))
        {
            return Err("Heatmaps require a non-empty rectangular matrix".into());
        }
        Ok(())
    }
}

pub fn parse_output(output: &str) -> Vec<PlotSpec> {
    output
        .lines()
        .filter_map(|line| line.trim().strip_prefix("forge_plot:"))
        .filter_map(|json| serde_json::from_str::<PlotSpec>(json.trim()).ok())
        .filter(|spec| spec.validate().is_ok())
        .collect()
}

pub fn svg(spec: &PlotSpec, width: u32, height: u32) -> Result<String, String> {
    spec.validate()?;
    let width = width.max(200);
    let height = height.max(120);
    let mut body = String::new();
    if spec.kind == PlotKind::Heatmap {
        let rows = spec.matrix.len();
        let columns = spec.matrix[0].len();
        let values = spec.matrix.iter().flatten().copied().collect::<Vec<_>>();
        let min = values.iter().copied().reduce(f64::min).unwrap_or(0.0);
        let max = values.iter().copied().reduce(f64::max).unwrap_or(min);
        let cell_width = (width as f64 - 65.0) / columns as f64;
        let cell_height = (height as f64 - 55.0) / rows as f64;
        for (row, values) in spec.matrix.iter().enumerate() {
            for (column, value) in values.iter().enumerate() {
                let t = ((*value - min) / (max - min).max(f64::EPSILON)).clamp(0.0, 1.0);
                let (red, green, blue) = (
                    (30.0 + 210.0 * t) as u8,
                    (50.0 + 80.0 * (1.0 - t)) as u8,
                    (220.0 - 180.0 * t) as u8,
                );
                body.push_str(&format!("<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{cell_width:.2}\" height=\"{cell_height:.2}\" fill=\"#{red:02x}{green:02x}{blue:02x}\"/>", 45.0 + column as f64 * cell_width, 25.0 + row as f64 * cell_height));
            }
        }
        return Ok(svg_document(spec, width, height, &body));
    }
    let points = spec
        .series
        .iter()
        .filter(|s| s.visible)
        .flat_map(series_points)
        .collect::<Vec<_>>();
    let (min_x, max_x, min_y, max_y) = bounds(&points);
    let map = |p: [f64; 2]| {
        let x = 45.0 + (p[0] - min_x) / (max_x - min_x).max(f64::EPSILON) * (width as f64 - 65.0);
        let y = height as f64
            - 35.0
            - (p[1] - min_y) / (max_y - min_y).max(f64::EPSILON) * (height as f64 - 55.0);
        (x, y)
    };
    for (index, series) in spec.series.iter().filter(|s| s.visible).enumerate() {
        let color = ["#278dcc", "#c4772c", "#2e9d60", "#d44855", "#9669d2"][index % 5];
        let mapped = series_points(series).map(map).collect::<Vec<_>>();
        match spec.kind {
            PlotKind::Scatter | PlotKind::Residual => {
                for (x, y) in mapped {
                    body.push_str(&format!(
                        "<circle cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"3\" fill=\"{color}\"/>"
                    ));
                }
            }
            _ => {
                let data = mapped
                    .iter()
                    .map(|(x, y)| format!("{x:.2},{y:.2}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                body.push_str(&format!("<polyline points=\"{data}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2\"/>"));
            }
        }
    }
    Ok(svg_document(spec, width, height, &body))
}
fn series_points(series: &PlotSeries) -> impl Iterator<Item = [f64; 2]> + '_ {
    series.points.iter().copied().chain(
        series
            .points
            .is_empty()
            .then_some(())
            .into_iter()
            .flat_map(|()| {
                series
                    .values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| [index as f64, *value])
            }),
    )
}
fn svg_document(spec: &PlotSpec, width: u32, height: u32, body: &str) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/><text x=\"16\" y=\"20\" font-family=\"sans-serif\" font-weight=\"bold\">{}</text><line x1=\"45\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#555\"/><line x1=\"45\" y1=\"30\" x2=\"45\" y2=\"{}\" stroke=\"#555\"/>{body}</svg>", xml_escape(&spec.name), height - 35, width - 20, height - 35, height - 35)
}
fn bounds(points: &[[f64; 2]]) -> (f64, f64, f64, f64) {
    if points.is_empty() {
        return (0.0, 1.0, 0.0, 1.0);
    }
    points.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(a, b, c, d), p| (a.min(p[0]), b.max(p[0]), c.min(p[1]), d.max(p[1])),
    )
}
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn metric_line(name: &str, values: &[[f64; 2]], color: Color32) -> Line<'static> {
    let points: PlotPoints = values.to_vec().into();
    Line::new(name.to_owned(), points).color(color).width(2.0)
}

pub fn vector_bars(name: &str, values: &[f64], color: Color32) -> BarChart {
    let bars = values
        .iter()
        .enumerate()
        .map(|(index, value)| Bar::new(index as f64, *value))
        .collect();
    BarChart::new(name.to_owned(), bars).color(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_empty_and_populated_series() {
        let _ = metric_line("loss", &[], Color32::WHITE);
        let _ = vector_bars("weights", &[1.0, 2.0], Color32::WHITE);
    }
    #[test]
    fn parses_and_exports_structured_plot() {
        let output = r#"forge_plot:{"version":1,"name":"ROC","kind":"roc","series":[{"name":"model","points":[[0.0,0.0],[1.0,1.0]]}]}"#;
        let specs = parse_output(output);
        assert_eq!(specs.len(), 1);
        assert!(svg(&specs[0], 640, 360).unwrap().contains("polyline"));
    }
    #[test]
    fn rejects_non_finite_and_ragged_heatmaps() {
        let spec = PlotSpec {
            version: 1,
            name: "bad".into(),
            kind: PlotKind::Heatmap,
            x_label: String::new(),
            y_label: String::new(),
            series: Vec::new(),
            matrix: vec![vec![1.0], vec![1.0, 2.0]],
            x_log: false,
            y_log: false,
        };
        assert!(spec.validate().is_err());
    }
}
