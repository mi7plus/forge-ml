//! The GPU provider (Forge Distribution, Phase 4).
//!
//! GPU acceleration ships compiled into every installer — Millwright ONNX via
//! DirectML (Windows) / CoreML (macOS) / CUDA (Linux), and Burn WebGPU training.
//! This provider makes the *environment system* aware of that: it detects the
//! GPU backends present on the machine, **covers** the `[gpu]` manifest section
//! (so it stops showing up as a gap), honors `backend` / `require`, and records
//! the selection in `forge.lock`.
//!
//! It detects and selects; it does not *install* toolkits. Cross-platform
//! toolkit management is the deliberately deferred Phase 5 work — here we report
//! what exists and let `require` turn a missing backend into a hard gap.

use super::diagnostics::tool_version;
use super::lock::{sha256_hex, LockEntry};
use super::manifest::{GpuRequest, Manifest};
use super::provider::{Activation, Capabilities, EnvironmentProvider, Probe};
use std::path::Path;
use std::process::Command;

/// One detected GPU acceleration path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backend {
    /// Canonical name, matching `[gpu].backend`: cuda | rocm | metal | directml.
    pub name: &'static str,
    /// How it was found (version or location).
    pub detail: String,
}

/// Detect the GPU backends available on this machine. Best-effort, read-only.
pub fn detect() -> Vec<Backend> {
    let mut backends = Vec::new();

    // CUDA — the toolkit (nvcc) enables Linux GPU ONNX inference; the driver
    // alone (nvidia-smi) can run prebuilt kernels but not builds.
    if let Some(version) = tool_version("nvcc", &["--version"]).map(|out| {
        out.lines()
            .find(|line| line.contains("release"))
            .unwrap_or(&out)
            .trim()
            .to_owned()
    }) {
        backends.push(Backend {
            name: "cuda",
            detail: format!("nvcc: {version}"),
        });
    } else if Command::new("nvidia-smi")
        .arg("-L")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        backends.push(Backend {
            name: "cuda",
            detail: "NVIDIA driver present (no nvcc toolkit)".to_owned(),
        });
    }

    // ROCm (Linux) — hipcc on PATH or the default install directory.
    if cfg!(target_os = "linux") {
        if let Some(version) = tool_version("hipcc", &["--version"]) {
            backends.push(Backend {
                name: "rocm",
                detail: format!("hipcc: {version}"),
            });
        } else if Path::new("/opt/rocm").is_dir() {
            backends.push(Backend {
                name: "rocm",
                detail: "/opt/rocm present".to_owned(),
            });
        }
    }

    // Metal / CoreML (macOS) and DirectML (Windows) are native and always present.
    if cfg!(target_os = "macos") {
        backends.push(Backend {
            name: "metal",
            detail: "Apple Metal / CoreML (built in)".to_owned(),
        });
    }
    if cfg!(target_os = "windows") {
        backends.push(Backend {
            name: "directml",
            detail: "DirectML (DirectX 12)".to_owned(),
        });
    }

    backends
}

/// A human report for `forge gpu detect`.
pub fn report() -> String {
    let detected = detect();
    let mut out = String::from("GPU backends detected:\n");
    if detected.is_empty() {
        out.push_str("  none — Millwright ONNX and Burn training will use the CPU\n");
    } else {
        for backend in &detected {
            out.push_str(&format!("  [ok] {:<9} {}\n", backend.name, backend.detail));
        }
    }
    out.push_str(
        "\nGPU acceleration is compiled into every installer; Burn WebGPU training\n\
         also uses the OS graphics drivers (Vulkan/Metal/DX12). Choose the device\n\
         in the app under Settings -> Compute, or declare a [gpu] section in\n\
         forge.toml (backend, require) to have `forge doctor` enforce it.\n",
    );
    out
}

pub struct GpuProvider;

impl EnvironmentProvider for GpuProvider {
    fn id(&self) -> &'static str {
        "gpu"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            gpu: true,
            ..Capabilities::default()
        }
    }

    fn probe(&self, manifest: &Manifest) -> Probe {
        let request = manifest.gpu_request();
        // Cheap path: with no `[gpu]` section (the startup/default case) there is
        // nothing to satisfy — and we avoid spawning nvcc/nvidia-smi at startup.
        if !request.present {
            return Probe::Missing("no [gpu] section in forge.toml".to_owned());
        }
        let detected: Vec<&'static str> = detect().iter().map(|backend| backend.name).collect();
        evaluate(&request, &detected)
    }

    fn activate(&self, _manifest: &Manifest) -> Option<Activation> {
        // GPU acceleration is compiled in and any toolkit lives on the user's own
        // PATH; there is no Forge-managed environment mutation to apply.
        None
    }

    fn materialize(&self, manifest: &Manifest) -> Result<LockEntry, String> {
        let request = manifest.gpu_request();
        let detected = detect();
        let selected = select(&request, &detected).unwrap_or("cpu-fallback");
        let mut extra = toml::Table::new();
        extra.insert("require".to_owned(), toml::Value::Boolean(request.require));
        if let Some(backend) = &request.backend {
            extra.insert(
                "requested".to_owned(),
                toml::Value::String(backend.clone()),
            );
        }
        extra.insert(
            "detected".to_owned(),
            toml::Value::Array(
                detected
                    .iter()
                    .map(|backend| toml::Value::String(backend.name.to_owned()))
                    .collect(),
            ),
        );
        Ok(LockEntry {
            id: self.id().to_owned(),
            kind: "gpu-backend".to_owned(),
            version: selected.to_owned(),
            sha256: sha256_hex(selected.as_bytes()),
            extra,
        })
    }
}

/// The backend that will actually be used: the requested one if present, else
/// the first detected (when a fallback is allowed), else `None` (CPU).
fn select(request: &GpuRequest, detected: &[Backend]) -> Option<&'static str> {
    match request.backend.as_deref() {
        Some("none") => None,
        Some(requested) => detected
            .iter()
            .find(|backend| backend.name == requested)
            .map(|backend| backend.name),
        None => detected.first().map(|backend| backend.name),
    }
}

/// Pure probe logic: is the `[gpu]` request satisfiable given `detected`? A
/// missing backend is only fatal when `require = true`; otherwise Forge falls
/// back to the CPU.
fn evaluate(request: &GpuRequest, detected: &[&'static str]) -> Probe {
    match request.backend.as_deref() {
        Some("none") => Probe::Available, // explicitly CPU
        Some(backend) => {
            if detected.contains(&backend) {
                Probe::Available
            } else if request.require {
                Probe::Incompatible(format!(
                    "[gpu] requires `{backend}` but it was not detected on this machine"
                ))
            } else {
                Probe::Available // fall back to the CPU
            }
        }
        None => {
            if !detected.is_empty() || !request.require {
                Probe::Available
            } else {
                Probe::Incompatible(
                    "[gpu] require=true but no GPU backend was detected".to_owned(),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(backend: Option<&str>, require: bool) -> GpuRequest {
        GpuRequest {
            present: true,
            backend: backend.map(str::to_owned),
            cuda: None,
            require,
        }
    }

    #[test]
    fn requested_backend_present_is_available() {
        assert_eq!(evaluate(&request(Some("cuda"), true), &["cuda"]), Probe::Available);
    }

    #[test]
    fn required_backend_absent_is_incompatible() {
        let probe = evaluate(&request(Some("cuda"), true), &["directml"]);
        assert!(matches!(probe, Probe::Incompatible(_)));
    }

    #[test]
    fn optional_backend_absent_falls_back_to_cpu() {
        // require = false ⇒ still available (CPU fallback), even with no GPU.
        assert_eq!(evaluate(&request(Some("cuda"), false), &[]), Probe::Available);
        assert_eq!(evaluate(&request(None, false), &[]), Probe::Available);
    }

    #[test]
    fn any_backend_required_but_none_detected_is_incompatible() {
        let probe = evaluate(&request(None, true), &[]);
        assert!(matches!(probe, Probe::Incompatible(_)));
    }

    #[test]
    fn backend_none_is_always_available() {
        assert_eq!(evaluate(&request(Some("none"), true), &[]), Probe::Available);
    }

    #[test]
    fn select_prefers_requested_then_first_detected() {
        let detected = vec![
            Backend { name: "directml", detail: String::new() },
            Backend { name: "cuda", detail: String::new() },
        ];
        assert_eq!(select(&request(Some("cuda"), false), &detected), Some("cuda"));
        assert_eq!(select(&request(None, false), &detected), Some("directml"));
        assert_eq!(select(&request(Some("none"), false), &detected), None);
        assert_eq!(select(&request(Some("rocm"), false), &detected), None);
    }
}
