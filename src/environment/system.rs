//! The system-toolchain provider: the user's own `rustc`/`cargo` (via rustup or
//! a standalone install), used when no offline runtime bundle is shipped — so
//! Forge works from source and in development builds, not only from the
//! installer.
//!
//! It is a deliberate fallback. When the bundled runtime *is* present that bundle
//! is authoritative, so this provider reports itself missing rather than stacking
//! a second toolchain over it (and so it never spawns `rustc` at app startup in a
//! shipped build). Adding it changes no existing behavior: in a dev build the app
//! already used the ambient toolchain, and this provider's `activate` is a no-op
//! because that toolchain is already on `PATH`.

use super::diagnostics::tool_version;
use super::lock::{sha256_hex, LockEntry};
use super::manifest::Manifest;
use super::provider::{Activation, Capabilities, EnvironmentProvider, Probe};

pub struct SystemToolchainProvider;

impl EnvironmentProvider for SystemToolchainProvider {
    fn id(&self) -> &'static str {
        "system-toolchain"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            toolchain: true,
            crates: true,
            ..Capabilities::default()
        }
    }

    fn probe(&self, manifest: &Manifest) -> Probe {
        // The bundled runtime, when shipped, is authoritative. Short-circuit here
        // so a shipped build never spawns `rustc` on the startup activation path.
        if crate::offline::detect().is_some() {
            return Probe::Missing("the bundled runtime is active in this build".to_owned());
        }
        let Some(rustc) = tool_version("rustc", &["--version"]) else {
            return Probe::Missing("no `rustc` on PATH (install from https://rustup.rs)".to_owned());
        };
        if tool_version("cargo", &["--version"]).is_none() {
            return Probe::Missing("`rustc` found but no `cargo` on PATH".to_owned());
        }
        // Respect a manifest toolchain pin rather than silently using another.
        if let Some(pinned) = &manifest.toolchain.rust {
            if !rustc.contains(pinned.as_str()) {
                return Probe::Incompatible(format!(
                    "manifest pins Rust {pinned}, but the system toolchain is `{rustc}`"
                ));
            }
        }
        Probe::Available
    }

    fn activate(&self, _manifest: &Manifest) -> Option<Activation> {
        // The system toolchain is already on `PATH`; there is nothing to mutate.
        None
    }

    fn materialize(&self, _manifest: &Manifest) -> Result<LockEntry, String> {
        let version =
            tool_version("rustc", &["--version"]).ok_or_else(|| "no system `rustc` to record".to_owned())?;
        Ok(LockEntry {
            id: self.id().to_owned(),
            kind: "system-toolchain".to_owned(),
            version: version.clone(),
            sha256: sha256_hex(version.as_bytes()),
            extra: toml::Table::new(),
        })
    }
}
