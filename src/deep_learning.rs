use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::millwright_studio::TrainingEvent;
use forge_protocol::TableData;

pub const BURN_VERSION: &str = "0.22.0-pre.3";
static NEXT_BURN_DEMO: AtomicU64 = AtomicU64::new(1);
const MAX_NATIVE_EPOCHS: usize = 10_000;
const MAX_NATIVE_ROWS: usize = 100_000;

#[derive(Debug, Clone, PartialEq)]
pub struct NativeTrainingData {
    pub dataset: String,
    pub feature: String,
    pub target: String,
    inputs: Vec<f32>,
    targets: Vec<f32>,
}

pub fn native_training_data(
    dataset: &str,
    table: &TableData,
    feature: &str,
    target: &str,
) -> Result<NativeTrainingData, String> {
    for (kind, value) in [
        ("Dataset", dataset),
        ("Feature column", feature),
        ("Target column", target),
    ] {
        if value.trim().is_empty() || value.len() > 128 {
            return Err(format!("{kind} name must contain 1 through 128 bytes"));
        }
    }
    if table.rows.len() > MAX_NATIVE_ROWS {
        return Err(format!(
            "Native Burn training accepts at most {MAX_NATIVE_ROWS} dataset rows"
        ));
    }
    let feature_index = table
        .columns
        .iter()
        .position(|column| column == feature)
        .ok_or_else(|| format!("Feature column `{feature}` was not found"))?;
    let target_index = table
        .columns
        .iter()
        .position(|column| column == target)
        .ok_or_else(|| format!("Target column `{target}` was not found"))?;
    if feature_index == target_index {
        return Err("Feature and target columns must be different".into());
    }
    let mut inputs = Vec::new();
    let mut targets = Vec::new();
    for row in &table.rows {
        let pair = row
            .get(feature_index)
            .zip(row.get(target_index))
            .and_then(|(x, y)| Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?)))
            .filter(|(x, y)| x.is_finite() && y.is_finite());
        if let Some((x, y)) = pair {
            inputs.push(x);
            targets.push(y);
        }
    }
    if inputs.len() < 2 {
        return Err("Native Burn training requires at least two complete numeric rows".into());
    }
    Ok(NativeTrainingData {
        dataset: dataset.to_owned(),
        feature: feature.to_owned(),
        target: target.to_owned(),
        inputs,
        targets,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeTrainingConfig {
    pub epochs: usize,
    pub learning_rate: f64,
}

impl Default for NativeTrainingConfig {
    fn default() -> Self {
        Self {
            epochs: 40,
            learning_rate: 0.05,
        }
    }
}

impl NativeTrainingConfig {
    fn validate(self) -> Result<Self, String> {
        if !(1..=MAX_NATIVE_EPOCHS).contains(&self.epochs) {
            return Err(format!(
                "Native Burn epochs must be between 1 and {MAX_NATIVE_EPOCHS}"
            ));
        }
        if !self.learning_rate.is_finite() || self.learning_rate <= 0.0 || self.learning_rate > 1.0
        {
            return Err(
                "Native Burn learning rate must be finite and greater than 0 through 1".into(),
            );
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Backend {
    Cpu,
    Wgpu,
    Cuda,
    Rocm,
}
impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU / Flex",
            Self::Wgpu => "WebGPU",
            Self::Cuda => "CUDA",
            Self::Rocm => "ROCm",
        }
    }
    pub fn feature(self) -> &'static str {
        match self {
            Self::Cpu => "flex",
            Self::Wgpu => "wgpu",
            Self::Cuda => "cuda",
            Self::Rocm => "rocm",
        }
    }
}

pub fn native_burn_self_test() -> String {
    use burn::tensor::{Device, Tensor};
    let input = Tensor::<1>::from_data([1.0_f32, 2.0, 3.0], &Device::flex());
    let sum: f32 = input.sum().into_scalar();
    format!("Embedded Burn {BURN_VERSION} Flex runtime ready (tensor sum {sum:.1}).")
}

#[cfg(test)]
pub fn native_burn_training_demo(backend: Backend) -> Result<Vec<TrainingEvent>, String> {
    native_burn_training_demo_with_progress(
        backend,
        NativeTrainingConfig::default(),
        None,
        || false,
        |_| {},
    )
}

pub fn native_burn_training_demo_with_progress(
    backend: Backend,
    config: NativeTrainingConfig,
    data: Option<NativeTrainingData>,
    cancelled: impl Fn() -> bool,
    on_event: impl FnMut(TrainingEvent),
) -> Result<Vec<TrainingEvent>, String> {
    let config = config.validate()?;
    if matches!(backend, Backend::Cuda | Backend::Rocm) {
        return Err(format!(
            "{} is available for generated or remote Burn projects, not the embedded runtime",
            backend.label()
        ));
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        native_burn_training_demo_inner(backend, config, data, cancelled, on_event)
    }))
    .map_err(|_| {
        format!(
            "The embedded {} Burn device could not be initialized",
            backend.label()
        )
    })?
}

fn native_burn_training_demo_inner(
    backend: Backend,
    config: NativeTrainingConfig,
    data: Option<NativeTrainingData>,
    cancelled: impl Fn() -> bool,
    mut on_event: impl FnMut(TrainingEvent),
) -> Result<Vec<TrainingEvent>, String> {
    use burn::{
        nn::LinearConfig,
        optim::{GradientsParams, SgdConfig},
        tensor::{Device, DeviceKind, Tensor},
    };

    let device = match backend {
        Backend::Cpu => Device::flex(),
        Backend::Wgpu => Device::wgpu(DeviceKind::DefaultDevice),
        Backend::Cuda | Backend::Rocm => unreachable!("unsupported backend rejected above"),
    }
    .autodiff();
    device.seed(7);
    let mut model = LinearConfig::new(1, 1).init(&device);
    let mut optimizer = SgdConfig::new().init();
    let (input, target, data_label) = if let Some(data) = data {
        let rows = data.inputs.len();
        (
            Tensor::<1>::from_floats(data.inputs.as_slice(), &device).reshape([rows, 1]),
            Tensor::<1>::from_floats(data.targets.as_slice(), &device).reshape([rows, 1]),
            format!(
                "{} {}→{} ({rows} rows)",
                data.dataset, data.feature, data.target
            ),
        )
    } else {
        (
            Tensor::<2>::from_data([[-2.0_f32], [-1.0], [0.0], [1.0], [2.0]], &device),
            Tensor::<2>::from_data([[-3.0_f32], [-1.0], [1.0], [3.0], [5.0]], &device),
            "built-in sample".into(),
        )
    };
    let run_id = format!(
        "burn-{}-{}",
        backend.feature(),
        NEXT_BURN_DEMO.fetch_add(1, Ordering::Relaxed)
    );
    let initial_events = [
        TrainingEvent::RunContext {
            run_id: run_id.clone(),
        },
        TrainingEvent::Started {
            job: format!(
                "Embedded Burn {} linear regression · {} ({} epochs, lr {})",
                backend.label(),
                data_label,
                config.epochs,
                config.learning_rate
            ),
            total_trials: 1,
        },
    ];
    let mut events = Vec::with_capacity(config.epochs.saturating_mul(2).saturating_add(4));
    for event in initial_events {
        on_event(event.clone());
        events.push(event);
    }
    let mut final_loss = None;
    for epoch in 1..=config.epochs {
        if cancelled() {
            return Err("Embedded Burn training was cancelled".into());
        }
        let output = model.forward(input.clone());
        let loss = (output - target.clone()).powf_scalar(2.0).mean();
        let loss_value = loss.clone().into_scalar::<f32>() as f64;
        if !loss_value.is_finite() {
            return Err("Embedded Burn training produced a non-finite loss".into());
        }
        let gradients = loss.backward();
        let gradients = GradientsParams::from_grads(gradients, &model);
        model = optimizer.step(config.learning_rate, model, gradients);
        final_loss = Some(loss_value);
        for event in [
            TrainingEvent::RunContext {
                run_id: run_id.clone(),
            },
            TrainingEvent::Epoch {
                epoch,
                total: config.epochs,
                loss: loss_value,
                metric: None,
            },
        ] {
            on_event(event.clone());
            events.push(event);
        }
    }
    let best_score = -final_loss.ok_or("Embedded Burn training emitted no epochs")?;
    for event in [
        TrainingEvent::RunContext { run_id },
        TrainingEvent::Completed { best_score },
    ] {
        on_event(event.clone());
        events.push(event);
    }
    Ok(events)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelSummary {
    pub name: String,
    pub layers: Vec<(String, String, usize)>,
    pub parameters: usize,
    pub trainable_parameters: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TensorView {
    pub name: String,
    pub shape: Vec<usize>,
    pub values: Vec<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageView {
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbeddingView {
    pub name: String,
    pub points: Vec<[f64; 2]>,
    pub labels: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PredictionView {
    pub name: String,
    pub labels: Vec<String>,
    pub probabilities: Vec<f64>,
}
#[derive(Debug, Clone, Default)]
pub struct DeepOutputs {
    pub model: Option<ModelSummary>,
    pub tensors: Vec<TensorView>,
    pub images: Vec<ImageView>,
    pub embeddings: Vec<EmbeddingView>,
    pub predictions: Vec<PredictionView>,
    pub checkpoints: Vec<String>,
}

pub fn parse_output(output: &str, state: &mut DeepOutputs) {
    for line in output.lines() {
        macro_rules! parse {
            ($prefix:literal, $target:expr, $ty:ty) => {
                if let Some(json) = line.strip_prefix($prefix) {
                    if let Ok(value) = serde_json::from_str::<$ty>(json.trim()) {
                        $target(value);
                    }
                    continue;
                }
            };
        }
        parse!(
            "forge_model:",
            |value| state.model = Some(value),
            ModelSummary
        );
        parse!(
            "forge_tensor:",
            |value| state.tensors.push(value),
            TensorView
        );
        parse!("forge_image:", |value| state.images.push(value), ImageView);
        parse!(
            "forge_embedding:",
            |value| state.embeddings.push(value),
            EmbeddingView
        );
        parse!(
            "forge_predictions:",
            |value| state.predictions.push(value),
            PredictionView
        );
        if let Some(path) = line.strip_prefix("forge_checkpoint:") {
            state.checkpoints.push(path.trim().to_owned());
        }
    }
}

pub fn generate_burn_project(root: &Path, backend: Backend) -> Result<String, String> {
    let project = root.join("burn-model");
    if project.exists() {
        return Err(format!("{} already exists.", project.display()));
    }
    std::fs::create_dir_all(project.join("src")).map_err(|e| e.to_string())?;
    let cargo = format!("[package]\nname = \"burn-model\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nburn = {{ version = \"{BURN_VERSION}\", default-features = false, features = [\"std\", \"train\", \"{}\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\n", backend.feature());
    let main = r##"use burn::nn;
use burn::prelude::*;

#[derive(Module, Debug)]
struct Model<B: Backend> { linear: nn::Linear<B> }

impl<B: Backend> Model<B> {
    fn new(device: &B::Device) -> Self { Self { linear: nn::LinearConfig::new(4, 2).init(device) } }
    fn forward(&self, input: Tensor<B, 2>) -> Tensor<B, 2> { self.linear.forward(input) }
}

fn main() {
    println!(r#"forge_model:{"name":"BurnModel","layers":[["linear","Linear 4→2",10]],"parameters":10,"trainable_parameters":10}"#);
    // Add your dataset and LearnerBuilder here. Forge monitors framework-neutral outputs.
}
"##;
    std::fs::write(project.join("Cargo.toml"), cargo).map_err(|e| e.to_string())?;
    std::fs::write(project.join("src/main.rs"), main).map_err(|e| e.to_string())?;
    Ok(format!(
        "Generated Burn {BURN_VERSION} {} template at {}",
        backend.label(),
        project.display()
    ))
}

#[derive(Debug, Clone, Default)]
pub struct ResourceSnapshot {
    pub cpu_percent: f32,
    pub used_memory: u64,
    pub total_memory: u64,
    pub gpu: String,
}
pub fn resources(system: &mut sysinfo::System) -> ResourceSnapshot {
    system.refresh_cpu_usage();
    system.refresh_memory();
    let gpu = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "GPU telemetry unavailable".into());
    ResourceSnapshot {
        cpu_percent: system.global_cpu_usage(),
        used_memory: system.used_memory(),
        total_memory: system.total_memory(),
        gpu,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_framework_neutral_tensor() {
        let mut state = DeepOutputs::default();
        parse_output(
            r#"forge_tensor:{"name":"x","shape":[2],"values":[1.0,2.0]}"#,
            &mut state,
        );
        assert_eq!(state.tensors[0].shape, [2]);
    }

    #[test]
    fn embedded_burn_executes_a_native_tensor() {
        assert!(native_burn_self_test().contains("tensor sum 6.0"));
    }

    #[test]
    fn embedded_burn_trains_and_emits_typed_progress() {
        let events = native_burn_training_demo(Backend::Cpu).unwrap();
        let losses = events
            .iter()
            .filter_map(|event| match event {
                TrainingEvent::Epoch { loss, .. } => Some(*loss),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(losses.len(), 40);
        assert!(losses.last().unwrap() < losses.first().unwrap());
        assert!(events
            .iter()
            .all(crate::millwright_studio::validate_training_event));
        assert!(matches!(
            events.last(),
            Some(TrainingEvent::Completed { .. })
        ));
        assert!(native_burn_training_demo(Backend::Cuda).is_err());
        assert!(native_burn_training_demo(Backend::Rocm).is_err());
    }

    #[test]
    fn embedded_burn_training_honors_cancellation_without_completion() {
        let mut emitted = Vec::new();
        let result = native_burn_training_demo_with_progress(
            Backend::Cpu,
            NativeTrainingConfig::default(),
            None,
            || true,
            |event| emitted.push(event),
        );
        assert_eq!(result.unwrap_err(), "Embedded Burn training was cancelled");
        assert_eq!(emitted.len(), 2);
        assert!(!emitted
            .iter()
            .any(|event| matches!(event, TrainingEvent::Completed { .. })));
    }

    #[test]
    fn embedded_burn_training_validates_and_applies_configuration() {
        let table = TableData {
            columns: vec!["feature".into(), "target".into(), "label".into()],
            rows: vec![
                vec!["-1".into(), "-1".into(), "a".into()],
                vec!["0".into(), "1".into(), "b".into()],
                vec!["bad".into(), "ignored".into(), "c".into()],
                vec!["1".into(), "3".into(), "d".into()],
            ],
        };
        let data = native_training_data("sample", &table, "feature", "target").unwrap();
        assert_eq!(data.inputs, vec![-1.0, 0.0, 1.0]);
        assert_eq!(data.targets, vec![-1.0, 1.0, 3.0]);
        let events = native_burn_training_demo_with_progress(
            Backend::Cpu,
            NativeTrainingConfig {
                epochs: 3,
                learning_rate: 0.02,
            },
            Some(data),
            || false,
            |_| {},
        )
        .unwrap();
        assert_eq!(events.len(), 10);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, TrainingEvent::Epoch { .. }))
                .count(),
            3
        );
        assert!(matches!(
            &events[1],
            TrainingEvent::Started { job, .. }
                if job.contains("sample feature→target (3 rows)")
        ));
        assert!(native_training_data("sample", &table, "feature", "feature").is_err());
        assert!(native_training_data("sample", &table, "missing", "target").is_err());
        for config in [
            NativeTrainingConfig {
                epochs: 0,
                learning_rate: 0.05,
            },
            NativeTrainingConfig {
                epochs: 1,
                learning_rate: f64::NAN,
            },
            NativeTrainingConfig {
                epochs: 1,
                learning_rate: 0.0,
            },
        ] {
            assert!(native_burn_training_demo_with_progress(
                Backend::Cpu,
                config,
                None,
                || false,
                |_| {}
            )
            .is_err());
        }
    }
}
