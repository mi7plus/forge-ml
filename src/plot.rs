use egui::Color32;
use egui_plot::{Bar, BarChart, Line, PlotPoints};

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
}
