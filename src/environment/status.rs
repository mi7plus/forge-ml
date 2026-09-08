//! A machine-readable snapshot of the environment for the standalone Forge
//! Manager app (`forge_ide --env-status-json [dir]`).
//!
//! The Manager is a separate GUI that shells out to `forge_ide` and renders this
//! JSON — exactly how Anaconda Navigator sits over the conda CLI. Keeping the
//! contract here means the Manager never links the environment internals; it just
//! consumes a stable, serialized view of what `doctor`, `gpu detect`,
//! `native check`, and `python check` already compute.

use super::{diagnostics, gpu, native, python, Manifest, Probe, Resolver};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub struct EnvStatus {
    pub forge_version: String,
    pub target: String,
    pub manifest_present: bool,
    pub profile: Option<String>,
    pub providers: Vec<ProviderStatus>,
    pub diagnostics: Vec<CheckStatus>,
    pub gpu: Vec<Named>,
    pub native: NativeStatus,
    pub python: PythonStatus,
    pub gaps: Vec<String>,
}

#[derive(Serialize)]
pub struct ProviderStatus {
    pub id: String,
    pub status: String,
}

#[derive(Serialize)]
pub struct CheckStatus {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Serialize)]
pub struct Named {
    pub name: String,
    pub detail: String,
}

#[derive(Serialize)]
pub struct NativeStatus {
    /// Prerequisites the manifest's `[native]` section declares, checked.
    pub prereqs: Vec<PrereqStatus>,
    /// Tools the shipped catalog can provide (name + version).
    pub catalog: Vec<Named>,
}

#[derive(Serialize)]
pub struct PrereqStatus {
    pub name: String,
    pub satisfied: bool,
    pub detail: String,
    /// True when the shipped catalog can provide this (installable in one click).
    pub providable: bool,
}

#[derive(Serialize)]
pub struct PythonStatus {
    pub present: bool,
    pub interpreter: Option<String>,
    pub source: Option<String>,
    pub version_requested: Option<String>,
    pub manager: Option<String>,
    pub bridge: Vec<String>,
}

/// Gather the full environment status for the project at `root`.
pub fn gather(root: &Path) -> EnvStatus {
    let manifest = Manifest::load(root).ok().flatten();
    let manifest_present = manifest.is_some();
    let manifest = manifest.unwrap_or_default();
    let resolver = Resolver::default_providers();

    let providers = resolver
        .providers()
        .map(|provider| ProviderStatus {
            id: provider.id().to_owned(),
            status: match provider.probe(&manifest) {
                Probe::Available => "available".to_owned(),
                Probe::Missing(reason) => format!("missing — {reason}"),
                Probe::Incompatible(reason) => format!("incompatible — {reason}"),
            },
        })
        .collect();

    let diagnostics = diagnostics::run()
        .into_iter()
        .map(|check| CheckStatus {
            name: check.label.to_owned(),
            status: match check.status {
                diagnostics::Status::Present => "ok",
                diagnostics::Status::Absent => "missing",
                diagnostics::Status::Note => "note",
            }
            .to_owned(),
            detail: check.detail,
        })
        .collect();

    let gpu = gpu::detect()
        .into_iter()
        .map(|backend| Named {
            name: backend.name.to_owned(),
            detail: backend.detail,
        })
        .collect();

    let native_request = manifest.native_request();
    let catalog = native::default_catalog().unwrap_or_default();
    let prereqs = native::statuses(&native_request)
        .into_iter()
        .map(|prereq| {
            let providable = !prereq.satisfied
                && catalog
                    .artifacts
                    .iter()
                    .any(|artifact| prereq.name.starts_with(&artifact.name));
            PrereqStatus {
                name: prereq.name,
                satisfied: prereq.satisfied,
                detail: prereq.detail,
                providable,
            }
        })
        .collect();
    let native = NativeStatus {
        prereqs,
        catalog: catalog
            .artifacts
            .iter()
            .map(|artifact| Named {
                name: artifact.name.clone(),
                detail: artifact.version.clone(),
            })
            .collect(),
    };

    let python_request = manifest.python_request();
    let interpreter = python::find_interpreter(root);
    let python = PythonStatus {
        present: python_request.present,
        interpreter: interpreter.as_ref().map(|found| found.version.clone()),
        source: interpreter.as_ref().map(|found| found.source.clone()),
        version_requested: python_request.version.clone(),
        manager: python_request.manager.clone(),
        bridge: python_request.bridge.clone(),
    };

    let gaps = resolver
        .gaps(&manifest)
        .into_iter()
        .map(|gap| gap.reason)
        .collect();

    EnvStatus {
        forge_version: env!("CARGO_PKG_VERSION").to_owned(),
        target: super::provision::host_target(),
        manifest_present,
        profile: manifest.environment.profile.clone(),
        providers,
        diagnostics,
        gpu,
        native,
        python,
        gaps,
    }
}

/// The status as pretty JSON (`forge_ide --env-status-json`).
pub fn status_json(root: &Path) -> String {
    serde_json::to_string_pretty(&gather(root)).unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"))
}
