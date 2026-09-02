//! The environment-provider interface — the extension point for the Forge
//! environment system. Today there is exactly one provider (the bundled offline
//! runtime, [`super::bundled::BundledRuntimeProvider`]); every future capability
//! (a system toolchain, native libraries, a GPU toolkit, a managed Python env)
//! arrives as a new provider implementing this trait, so the CLI, IDE, and
//! resolver never change to gain one.

use super::lock::LockEntry;
use super::manifest::Manifest;
use std::ffi::OsString;
use std::path::PathBuf;

/// The concerns a provider can satisfy. The resolver uses these both to route
/// manifest sections and to detect gaps ("`[gpu]` requested, nothing provides it").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub toolchain: bool,
    pub crates: bool,
    pub native: bool,
    pub gpu: bool,
    pub python: bool,
}

impl Capabilities {
    /// Whether this provider claims the reserved manifest section named `section`.
    pub fn covers(&self, section: &str) -> bool {
        match section {
            "native" => self.native,
            "gpu" => self.gpu,
            "python" => self.python,
            "toolchain" => self.toolchain,
            _ => false,
        }
    }
}

/// The result of a cheap, read-only availability check on the current machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// Ready to materialize/activate.
    Available,
    /// Not present (e.g. no bundle shipped); the reason is shown by `forge doctor`.
    Missing(String),
    /// Present but can't satisfy the manifest (e.g. wrong toolchain version).
    Incompatible(String),
}

impl Probe {
    pub fn is_available(&self) -> bool {
        matches!(self, Probe::Available)
    }
}

/// The environment mutations needed to *use* what a provider materialized —
/// deliberately data, not side effects, so several providers compose into one
/// activation that the caller applies exactly once (via [`Activation::apply`]).
#[derive(Debug, Clone, Default)]
pub struct Activation {
    /// Directories to prepend to `PATH`, highest priority first.
    pub path_prepend: Vec<PathBuf>,
    /// Environment variables to set verbatim.
    pub env: Vec<(OsString, OsString)>,
    /// A `CARGO_HOME` to create and seed with a generated `config.toml`.
    pub cargo_config: Option<CargoConfig>,
    /// A scratch directory to create and expose as `EVCXR_TMPDIR`.
    pub scratch: Option<PathBuf>,
}

/// A generated cargo configuration written into a per-user `CARGO_HOME`.
#[derive(Debug, Clone)]
pub struct CargoConfig {
    pub home: PathBuf,
    pub contents: String,
}

impl Activation {
    /// Combine another provider's activation into this one (later `PATH` entries
    /// keep their relative order after the earlier ones; a later `cargo_config`
    /// or `scratch` wins, matching provider registration order).
    pub fn merge(mut self, other: Activation) -> Activation {
        self.path_prepend.extend(other.path_prepend);
        self.env.extend(other.env);
        if other.cargo_config.is_some() {
            self.cargo_config = other.cargo_config;
        }
        if other.scratch.is_some() {
            self.scratch = other.scratch;
        }
        self
    }

    /// Apply the mutations to this process's environment. Idempotent in effect;
    /// creating the `CARGO_HOME`/scratch directories is best-effort.
    pub fn apply(&self) {
        if !self.path_prepend.is_empty() {
            let sep = if cfg!(windows) { ";" } else { ":" };
            let existing = std::env::var_os("PATH").unwrap_or_default();
            let mut new_path = OsString::new();
            for (index, dir) in self.path_prepend.iter().enumerate() {
                if index > 0 {
                    new_path.push(sep);
                }
                new_path.push(dir);
            }
            if !existing.is_empty() {
                new_path.push(sep);
                new_path.push(existing);
            }
            std::env::set_var("PATH", new_path);
        }
        for (key, value) in &self.env {
            std::env::set_var(key, value);
        }
        if let Some(config) = &self.cargo_config {
            if std::fs::create_dir_all(&config.home).is_ok()
                && std::fs::write(config.home.join("config.toml"), &config.contents).is_ok()
            {
                std::env::set_var("CARGO_HOME", &config.home);
            }
        }
        if let Some(scratch) = &self.scratch {
            let _ = std::fs::create_dir_all(scratch);
            std::env::set_var("EVCXR_TMPDIR", scratch);
        }
    }
}

/// A source of some slice of the activated environment. See the module docs.
pub trait EnvironmentProvider {
    /// Stable identifier recorded in `forge.lock` (e.g. `"bundled-runtime"`).
    fn id(&self) -> &'static str;

    /// Which manifest concerns this provider claims.
    fn capabilities(&self) -> Capabilities;

    /// Cheap, read-only: can this provider satisfy `manifest` on this machine?
    fn probe(&self, manifest: &Manifest) -> Probe;

    /// Produce the (fast) live activation for `manifest`, or `None` if this
    /// provider contributes nothing. Must not hash large trees — that's for
    /// [`materialize`](EnvironmentProvider::materialize).
    fn activate(&self, manifest: &Manifest) -> Option<Activation>;

    /// Record what this provider contributes into a lock entry (with hashes),
    /// for `forge env sync` / `forge.lock`. May be more expensive than `activate`.
    fn materialize(&self, manifest: &Manifest) -> Result<LockEntry, String>;
}
