use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TrainingEvent {
    Started {
        job: String,
        total_trials: usize,
    },
    TrialStarted {
        trial: usize,
        parameters: String,
    },
    FoldCompleted {
        trial: usize,
        fold: usize,
        folds: usize,
        score: f64,
    },
    TrialCompleted {
        trial: usize,
        score: f64,
    },
    Epoch {
        epoch: usize,
        total: usize,
        loss: f64,
        metric: Option<f64>,
    },
    Batch {
        epoch: usize,
        batch: usize,
        total: usize,
        loss: f64,
        samples_per_second: f64,
    },
    Checkpoint {
        path: String,
        epoch: usize,
    },
    EarlyStopping {
        epoch: usize,
        best_epoch: usize,
        best_score: f64,
    },
    Completed {
        best_score: f64,
    },
    Failed {
        message: String,
    },
}

pub trait TrainingObserver: Send + Sync {
    fn observe(&self, event: TrainingEvent);
}

#[derive(Clone, Default)]
pub struct ChannelObserver(Arc<Mutex<Vec<TrainingEvent>>>);
impl TrainingObserver for ChannelObserver {
    fn observe(&self, event: TrainingEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}
impl ChannelObserver {
    pub fn drain(&self) -> Vec<TrainingEvent> {
        self.0
            .lock()
            .map(|mut events| std::mem::take(&mut *events))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PipelineStep {
    Impute,
    Standardize,
    OneHotEncode,
    RandomForest,
    LogisticRegression,
    LinearRegression,
}

impl PipelineStep {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Impute => "Impute",
            Self::Standardize => "Standardize",
            Self::OneHotEncode => "One-hot encode",
            Self::RandomForest => "Random forest",
            Self::LogisticRegression => "Logistic regression",
            Self::LinearRegression => "Linear regression",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineDesign {
    pub name: String,
    pub target: String,
    pub steps: Vec<PipelineStep>,
}

impl PipelineDesign {
    pub fn rust_code(&self) -> String {
        let mut code = String::from("use millwright::prelude::*;\n\n");
        code.push_str("let table = Table::from_csv(\"data.csv\")?;\n");
        code.push_str(&format!(
            "let train = table.into_dataset(\"{}\")?;\n",
            self.target
        ));
        code.push_str("let mut pipeline = Pipeline::new()\n");
        for step in &self.steps {
            let line = match step {
                PipelineStep::Impute => "    .step(\"impute\", SimpleImputer::mean())\n",
                PipelineStep::Standardize => "    .step(\"scale\", StandardScaler::new())\n",
                PipelineStep::OneHotEncode => "    .step(\"encode\", OneHotEncoder::infer())\n",
                PipelineStep::RandomForest => "    .estimator(\"model\", RandomForest::new())\n",
                PipelineStep::LogisticRegression => {
                    "    .estimator(\"model\", LogisticRegression::new())\n"
                }
                PipelineStep::LinearRegression => {
                    "    .estimator(\"model\", LinearRegression::new())\n"
                }
            };
            code.push_str(line);
        }
        code.push_str(";\npipeline.fit(&train)?;\n");
        code
    }

    pub fn onnx_export_code(&self, artifact: &str) -> String {
        let mut code = self.rust_code();
        code.push_str("\n// Millwright 2.2's native, published-crate portability boundary.\n");
        code.push_str("use millwright::onnx::{ExportOnnx, InferenceModel};\n");
        code.push_str(&format!(
            "let artifact = std::path::Path::new({artifact:?});\npipeline.export_onnx(artifact)?;\n"
        ));
        code.push_str("let portable = InferenceModel::load(artifact)?;\n");
        code.push_str("let native = pipeline.predict(train.features())?;\n");
        code.push_str("let round_trip = portable.predict(train.features())?;\n");
        code.push_str("assert_eq!(native.len(), round_trip.len());\n");
        code.push_str("println!(\"Exported and verified {} predictions at {}\", round_trip.len(), artifact.display());\n");
        code
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub accuracy: Option<f64>,
    pub rmse: Option<f64>,
    pub confusion: Vec<Vec<u64>>,
    pub roc: Vec<[f64; 2]>,
    pub residuals: Vec<f64>,
    pub feature_importance: Vec<(String, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub model: String,
    pub score: f64,
    pub duration_ms: u128,
    pub parameters: String,
}

pub fn parse_runtime_output(output: &str) -> (Vec<TrainingEvent>, Vec<EvaluationReport>) {
    let mut events = Vec::new();
    let mut reports = Vec::new();
    for line in output.lines() {
        if let Some(json) = line.strip_prefix("forge_training:") {
            if let Ok(event) = serde_json::from_str(json.trim()) {
                events.push(event);
            }
        }
        if let Some(json) = line.strip_prefix("forge_evaluation:") {
            if let Ok(report) = serde_json::from_str(json.trim()) {
                reports.push(report);
            }
        }
    }
    (events, reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generates_pipeline_code_in_step_order() {
        let design = PipelineDesign {
            name: "demo".into(),
            target: "label".into(),
            steps: vec![PipelineStep::Impute, PipelineStep::RandomForest],
        };
        let code = design.rust_code();
        assert!(code.find("SimpleImputer").unwrap() < code.find("RandomForest").unwrap());
        assert!(code.contains("into_dataset(\"label\")"));
    }
    #[test]
    fn generates_native_onnx_export_and_round_trip() {
        let design = PipelineDesign {
            name: "portable".into(),
            target: "label".into(),
            steps: vec![PipelineStep::Standardize, PipelineStep::LinearRegression],
        };
        let code = design.onnx_export_code("models/portable.onnx");
        assert!(code.contains("millwright::onnx::{ExportOnnx, InferenceModel}"));
        assert!(code.contains("pipeline.export_onnx(artifact)?"));
        assert!(code.contains("InferenceModel::load(artifact)?"));
    }
    #[test]
    fn channel_observer_forwards_events() {
        let observer = ChannelObserver::default();
        observer.observe(TrainingEvent::Completed { best_score: 0.9 });
        assert_eq!(
            observer.drain(),
            vec![TrainingEvent::Completed { best_score: 0.9 }]
        );
    }
    #[test]
    fn parses_training_telemetry() {
        let (events, _) =
            parse_runtime_output("forge_training:{\"Completed\":{\"best_score\":0.9}}");
        assert_eq!(events.len(), 1);
    }
}
