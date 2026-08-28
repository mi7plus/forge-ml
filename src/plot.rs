use egui::Color32;
use egui_plot::{Bar, BarChart, Line, PlotPoints};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

pub const PLOT_SPEC_VERSION: u16 = 1;
const MAX_POINTS: usize = 1_000_000;
const MAX_RASTER_PIXELS: u64 = 16_777_216;

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

/// Render a structured plot to a portable, deterministic RGB PNG.
pub fn png(spec: &PlotSpec, width: u32, height: u32) -> Result<Vec<u8>, String> {
    spec.validate()?;
    let width = width.max(200);
    let height = height.max(120);
    if u64::from(width) * u64::from(height) > MAX_RASTER_PIXELS {
        return Err(format!(
            "PNG dimensions exceed the {MAX_RASTER_PIXELS}-pixel safety limit"
        ));
    }
    let mut canvas = Raster::new(width, height);
    canvas.line(45, 30, 45, height as i32 - 35, [85, 85, 85]);
    canvas.line(
        45,
        height as i32 - 35,
        width as i32 - 20,
        height as i32 - 35,
        [85, 85, 85],
    );
    if spec.kind == PlotKind::Heatmap {
        let values = spec.matrix.iter().flatten().copied().collect::<Vec<_>>();
        let min = values.iter().copied().reduce(f64::min).unwrap_or(0.0);
        let max = values.iter().copied().reduce(f64::max).unwrap_or(min);
        let rows = spec.matrix.len();
        let columns = spec.matrix[0].len();
        for (row, values) in spec.matrix.iter().enumerate() {
            for (column, value) in values.iter().enumerate() {
                let t = ((*value - min) / (max - min).max(f64::EPSILON)).clamp(0.0, 1.0);
                let color = [
                    (30.0 + 210.0 * t) as u8,
                    (50.0 + 80.0 * (1.0 - t)) as u8,
                    (220.0 - 180.0 * t) as u8,
                ];
                let x0 = 45 + ((width - 65) as usize * column / columns) as i32;
                let x1 = 45 + ((width - 65) as usize * (column + 1) / columns) as i32;
                let y0 = 30 + ((height - 65) as usize * row / rows) as i32;
                let y1 = 30 + ((height - 65) as usize * (row + 1) / rows) as i32;
                canvas.rect(x0, y0, x1, y1, color);
            }
        }
    } else {
        let points = spec
            .series
            .iter()
            .filter(|series| series.visible)
            .flat_map(series_points)
            .collect::<Vec<_>>();
        let (min_x, max_x, min_y, max_y) = bounds(&points);
        let map = |point: [f64; 2]| {
            let x = 45.0
                + (point[0] - min_x) / (max_x - min_x).max(f64::EPSILON) * (width as f64 - 65.0);
            let y = height as f64
                - 35.0
                - (point[1] - min_y) / (max_y - min_y).max(f64::EPSILON) * (height as f64 - 65.0);
            (x.round() as i32, y.round() as i32)
        };
        const COLORS: [[u8; 3]; 5] = [
            [39, 141, 204],
            [196, 119, 44],
            [46, 157, 96],
            [212, 72, 85],
            [150, 105, 210],
        ];
        for (index, series) in spec
            .series
            .iter()
            .filter(|series| series.visible)
            .enumerate()
        {
            let mapped = series_points(series).map(map).collect::<Vec<_>>();
            let color = COLORS[index % COLORS.len()];
            match spec.kind {
                PlotKind::Scatter | PlotKind::Residual => {
                    for (x, y) in mapped {
                        canvas.circle(x, y, 3, color);
                    }
                }
                PlotKind::Bar | PlotKind::Histogram | PlotKind::FeatureImportance => {
                    let baseline = map([0.0, 0.0]).1.clamp(30, height as i32 - 35);
                    let half_width = ((width.saturating_sub(65) as usize / mapped.len().max(1))
                        .clamp(1, 16) as i32)
                        / 2;
                    for (x, y) in mapped {
                        canvas.rect(
                            x - half_width,
                            y.min(baseline),
                            x + half_width + 1,
                            y.max(baseline) + 1,
                            color,
                        );
                    }
                }
                _ => {
                    for pair in mapped.windows(2) {
                        canvas.line(pair[0].0, pair[0].1, pair[1].0, pair[1].1, color);
                    }
                    if mapped.len() == 1 {
                        canvas.circle(mapped[0].0, mapped[0].1, 2, color);
                    }
                }
            }
        }
    }
    canvas.encode()
}

/// Export a self-contained interactive plot that does not load remote scripts or styles.
pub fn html(spec: &PlotSpec, width: u32, height: u32) -> Result<String, String> {
    spec.validate()?;
    let width = width.max(200);
    let height = height.max(120);
    if u64::from(width) * u64::from(height) > MAX_RASTER_PIXELS {
        return Err(format!(
            "HTML canvas dimensions exceed the {MAX_RASTER_PIXELS}-pixel safety limit"
        ));
    }
    let payload = serde_json::to_string(spec)
        .map_err(|error| error.to_string())?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    let title = xml_escape(&spec.name);
    Ok(INTERACTIVE_HTML
        .replace("__TITLE__", &title)
        .replace("__WIDTH__", &width.to_string())
        .replace("__HEIGHT__", &height.to_string())
        .replace("__SPEC__", &payload))
}

const INTERACTIVE_HTML: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'">
<title>__TITLE__</title><style>
:root{color-scheme:light dark;font-family:system-ui,sans-serif}body{margin:20px;background:#fff;color:#20242b}
#toolbar{display:flex;gap:12px;flex-wrap:wrap;align-items:center;margin-bottom:8px}button,label{font:inherit}
canvas{max-width:100%;border:1px solid #c9ced6;background:#fff;cursor:crosshair}#tip{min-width:180px;color:#4b5563}
</style></head><body><h2>__TITLE__</h2><div id="toolbar"><button id="reset">Reset view</button><span id="series"></span><span id="tip">Hover for coordinates</span></div>
<canvas id="plot" width="__WIDTH__" height="__HEIGHT__"></canvas><script>
'use strict';const spec=__SPEC__,canvas=document.getElementById('plot'),ctx=canvas.getContext('2d');
const colors=['#278dcc','#c4772c','#2e9d60','#d44855','#9669d2'];let zoom=1,panX=0,panY=0,drag=null;
const visible=spec.series.map(s=>s.visible!==false),seriesBox=document.getElementById('series'),tip=document.getElementById('tip');
spec.series.forEach((s,i)=>{const l=document.createElement('label'),c=document.createElement('input');c.type='checkbox';c.checked=visible[i];c.onchange=()=>{visible[i]=c.checked;draw()};l.append(c,document.createTextNode(' '+s.name));seriesBox.append(l)});
function points(s){return s.points.length?s.points:s.values.map((v,i)=>[i,v])}function all(){return spec.series.flatMap((s,i)=>visible[i]?points(s):[])}
function bounds(){const p=all();if(!p.length)return[0,1,0,1];return p.reduce((b,q)=>[Math.min(b[0],q[0]),Math.max(b[1],q[0]),Math.min(b[2],q[1]),Math.max(b[3],q[1])],[Infinity,-Infinity,Infinity,-Infinity])}
function mapper(){const b=bounds(),dx=Math.max(Number.EPSILON,b[1]-b[0]),dy=Math.max(Number.EPSILON,b[3]-b[2]);return p=>[45+(p[0]-b[0])/dx*(canvas.width-65)*zoom+panX,canvas.height-35-(p[1]-b[2])/dy*(canvas.height-65)*zoom+panY]}
function draw(){ctx.clearRect(0,0,canvas.width,canvas.height);ctx.strokeStyle='#555';ctx.beginPath();ctx.moveTo(45,30);ctx.lineTo(45,canvas.height-35);ctx.lineTo(canvas.width-20,canvas.height-35);ctx.stroke();
if(spec.kind==='heatmap'){const a=spec.matrix.flat(),lo=Math.min(...a),hi=Math.max(...a),rows=spec.matrix.length,cols=spec.matrix[0].length;spec.matrix.forEach((row,y)=>row.forEach((v,x)=>{const t=(v-lo)/Math.max(Number.EPSILON,hi-lo);ctx.fillStyle=`rgb(${30+210*t},${50+80*(1-t)},${220-180*t})`;ctx.fillRect(45+x*(canvas.width-65)/cols,30+y*(canvas.height-65)/rows,(canvas.width-65)/cols+1,(canvas.height-65)/rows+1)}));return}
const map=mapper();spec.series.forEach((s,i)=>{if(!visible[i])return;const p=points(s).map(map);ctx.strokeStyle=ctx.fillStyle=colors[i%colors.length];if(['scatter','residual'].includes(spec.kind)){p.forEach(q=>{ctx.beginPath();ctx.arc(q[0],q[1],3,0,7);ctx.fill()})}else if(['bar','histogram','feature_importance'].includes(spec.kind)){const base=Math.min(canvas.height-35,Math.max(30,map([0,0])[1])),w=Math.max(1,Math.min(16,(canvas.width-65)/Math.max(1,p.length)/2));p.forEach(q=>ctx.fillRect(q[0]-w,Math.min(q[1],base),w*2,Math.max(1,Math.abs(base-q[1]))))}else{ctx.beginPath();p.forEach((q,j)=>j?ctx.lineTo(...q):ctx.moveTo(...q));ctx.stroke()}})}
canvas.onwheel=e=>{e.preventDefault();zoom=Math.min(20,Math.max(.25,zoom*(e.deltaY<0?1.1:.9)));draw()};canvas.onmousedown=e=>drag=[e.offsetX-panX,e.offsetY-panY];canvas.onmouseup=canvas.onmouseleave=()=>drag=null;canvas.onmousemove=e=>{if(drag){panX=e.offsetX-drag[0];panY=e.offsetY-drag[1];draw()}tip.textContent=`pixel ${e.offsetX.toFixed(0)}, ${e.offsetY.toFixed(0)}`};document.getElementById('reset').onclick=()=>{zoom=1;panX=panY=0;draw()};draw();
</script></body></html>"#;

struct Raster {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
impl Raster {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![255; width as usize * height as usize * 3],
        }
    }
    fn pixel(&mut self, x: i32, y: i32, color: [u8; 3]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let offset = (y as usize * self.width as usize + x as usize) * 3;
        self.pixels[offset..offset + 3].copy_from_slice(&color);
    }
    fn line(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: [u8; 3]) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            self.pixel(x0, y0, color);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let twice = error * 2;
            if twice >= dy {
                error += dy;
                x0 += sx;
            }
            if twice <= dx {
                error += dx;
                y0 += sy;
            }
        }
    }
    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 3]) {
        for y in y0.max(0)..y1.min(self.height as i32) {
            for x in x0.max(0)..x1.min(self.width as i32) {
                self.pixel(x, y, color);
            }
        }
    }
    fn circle(&mut self, center_x: i32, center_y: i32, radius: i32, color: [u8; 3]) {
        for y in -radius..=radius {
            for x in -radius..=radius {
                if x * x + y * y <= radius * radius {
                    self.pixel(center_x + x, center_y + y, color);
                }
            }
        }
    }
    fn encode(self) -> Result<Vec<u8>, String> {
        let mut output = Vec::new();
        {
            let mut encoder = png::Encoder::new(Cursor::new(&mut output), self.width, self.height);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
            writer
                .write_image_data(&self.pixels)
                .map_err(|error| error.to_string())?;
        }
        Ok(output)
    }
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
        let raster = png(&specs[0], 640, 360).unwrap();
        assert_eq!(&raster[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(raster[16..20].try_into().unwrap()), 640);
        assert_eq!(u32::from_be_bytes(raster[20..24].try_into().unwrap()), 360);
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

    #[test]
    fn interactive_html_is_self_contained_and_script_safe() {
        let spec = PlotSpec {
            version: PLOT_SPEC_VERSION,
            name: "closing </script> & plot".into(),
            kind: PlotKind::Line,
            x_label: "x".into(),
            y_label: "y".into(),
            series: vec![PlotSeries {
                name: "sample </script>".into(),
                points: vec![[0.0, 1.0], [1.0, 2.0]],
                values: Vec::new(),
                visible: true,
            }],
            matrix: Vec::new(),
            x_log: false,
            y_log: false,
        };
        let output = html(&spec, 800, 450).unwrap();
        assert!(output.contains("<canvas id=\"plot\" width=\"800\" height=\"450\""));
        assert!(output.contains("\\u003c/script\\u003e"));
        assert!(!output.contains("http://"));
        assert!(!output.contains("https://"));
    }
}
