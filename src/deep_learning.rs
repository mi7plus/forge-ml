use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

pub const BURN_VERSION: &str = "0.22.0-pre.3";

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
            Self::Cpu => "CPU / ndarray",
            Self::Wgpu => "WebGPU",
            Self::Cuda => "CUDA",
            Self::Rocm => "ROCm",
        }
    }
    pub fn feature(self) -> &'static str {
        match self {
            Self::Cpu => "ndarray",
            Self::Wgpu => "wgpu",
            Self::Cuda => "cuda",
            Self::Rocm => "rocm",
        }
    }
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
}
