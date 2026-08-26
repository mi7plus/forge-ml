use forge_protocol::{ForgeEvent, TableData};
use std::collections::HashMap;

#[derive(Default)]
pub struct DataWorkspace {
    pub metrics: HashMap<String, Vec<[f64; 2]>>,
    pub vectors: HashMap<String, Vec<f64>>,
    pub tables: HashMap<String, TableData>,
}

impl DataWorkspace {
    pub fn apply(&mut self, event: ForgeEvent) {
        match event {
            ForgeEvent::Metric { name, value } => {
                let series = self.metrics.entry(name).or_default();
                series.push([series.len() as f64, value]);
            }
            ForgeEvent::Vector { name, values } => {
                self.vectors.insert(name, values);
            }
            ForgeEvent::Table { name, data } => {
                self.tables.insert(name, data);
            }
        }
    }

    pub fn clear(&mut self) {
        self.metrics.clear();
        self.vectors.clear();
        self.tables.clear();
    }

    pub fn has_telemetry(&self) -> bool {
        !self.metrics.is_empty() || !self.vectors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_metrics_and_replaces_named_datasets() {
        let mut data = DataWorkspace::default();
        data.apply(ForgeEvent::Metric {
            name: "loss".into(),
            value: 1.0,
        });
        data.apply(ForgeEvent::Metric {
            name: "loss".into(),
            value: 0.5,
        });
        data.apply(ForgeEvent::Vector {
            name: "weights".into(),
            values: vec![1.0],
        });
        data.apply(ForgeEvent::Vector {
            name: "weights".into(),
            values: vec![2.0],
        });
        assert_eq!(data.metrics["loss"], [[0.0, 1.0], [1.0, 0.5]]);
        assert_eq!(data.vectors["weights"], [2.0]);
    }
}
