use forge_protocol::RunId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize)]
pub struct ExperimentRun {
    #[serde(default)]
    pub id: RunId,
    pub name: String,
    pub metrics: HashMap<String, Vec<[f64; 2]>>,
    pub vectors: HashMap<String, Vec<f64>>,
    pub execution_count: usize,
}

impl ExperimentRun {
    pub fn snapshot(
        name: String,
        metrics: &HashMap<String, Vec<[f64; 2]>>,
        vectors: &HashMap<String, Vec<f64>>,
        execution_count: usize,
    ) -> Self {
        Self {
            id: RunId::new(),
            name,
            metrics: metrics.clone(),
            vectors: vectors.clone(),
            execution_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_snapshots_receive_stable_ids() {
        let run: ExperimentRun = serde_json::from_str(
            r#"{"name":"baseline","metrics":{},"vectors":{},"execution_count":1}"#,
        )
        .unwrap();
        assert!(!run.id.as_str().is_empty());
    }
}
