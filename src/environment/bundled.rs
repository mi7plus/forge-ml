//! The one provider that exists today: the self-contained offline runtime bundle
//! shipped inside the installer. It is a thin adapter over [`crate::offline`] —
//! `probe` is bundle detection, `activate` is the same environment mutation the
//! app has always applied, and `materialize` records the bundle in `forge.lock`.
//! Adding a `SystemToolchainProvider`, `GpuProvider`, etc. later touches nothing
//! here or in the resolver.

use super::lock::{sha256_hex, LockEntry};
use super::manifest::Manifest;
use super::provider::{Activation, Capabilities, CargoConfig, EnvironmentProvider, Probe};
use std::ffi::OsString;

pub struct BundledRuntimeProvider;

impl EnvironmentProvider for BundledRuntimeProvider {
    fn id(&self) -> &'static str {
        "bundled-runtime"
    }

    fn capabilities(&self) -> Capabilities {
        // The bundle carries the toolchain and the vendored crate closure. Native
        // libs / GPU / Python are deliberately not claimed — that's what makes the
        // reserved manifest sections show up as gaps until a provider covers them.
        Capabilities {
            toolchain: true,
            crates: true,
            ..Capabilities::default()
        }
    }

    fn probe(&self, manifest: &Manifest) -> Probe {
        let Some(runtime) = crate::offline::detect() else {
            return Probe::Missing(
                "no offline runtime bundle in this build (using the system toolchain)".to_owned(),
            );
        };
        // If the manifest pins a toolchain the bundle doesn't provide, say so
        // rather than silently activating a different one.
        if let Some(pinned) = &manifest.toolchain.rust {
            if !runtime.version.contains(pinned.as_str()) {
                return Probe::Incompatible(format!(
                    "manifest pins Rust {pinned}, but the bundled runtime is {}",
                    runtime.version
                ));
            }
        }
        Probe::Available
    }

    fn activate(&self, _manifest: &Manifest) -> Option<Activation> {
        let runtime = crate::offline::detect()?;
        Some(Activation {
            path_prepend: vec![runtime.bin.clone()],
            env: vec![
                (OsString::from("CARGO_NET_OFFLINE"), OsString::from("true")),
                // A rustup shim on PATH would otherwise try to resolve a toolchain
                // and reach the network.
                (OsString::from("RUSTUP_TOOLCHAIN"), OsString::from("")),
            ],
            cargo_config: Some(CargoConfig {
                home: crate::offline::cargo_home_dir(),
                contents: crate::offline::cargo_config_contents(runtime),
            }),
            scratch: Some(crate::offline::scratch_dir()),
        })
    }

    fn materialize(&self, _manifest: &Manifest) -> Result<LockEntry, String> {
        let runtime = crate::offline::detect()
            .ok_or_else(|| "no offline runtime bundle to record".to_owned())?;
        // Cheap identity hash over the toolchain version + vendor path; hashing
        // the full ~900 MB vendor tree would be too slow for an on-demand sync.
        let identity = format!("{}|{}", runtime.version, runtime.vendor.display());
        let mut extra = toml::Table::new();
        extra.insert(
            "vendor".to_owned(),
            toml::Value::String(runtime.vendor.display().to_string()),
        );
        Ok(LockEntry {
            id: self.id().to_owned(),
            kind: "offline-bundle".to_owned(),
            version: runtime.version.clone(),
            sha256: sha256_hex(identity.as_bytes()),
            extra,
        })
    }
}
