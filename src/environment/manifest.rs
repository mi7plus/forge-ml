//! `forge.toml` — the human-authored environment manifest that sits beside
//! `Cargo.toml`. It declares the *environment* Forge activates; Cargo still owns
//! crate resolution. It is optional: a project without one gets the defaults
//! (bundled runtime, current toolchain).
//!
//! The `[native]`, `[gpu]`, and `[python]` sections are **reserved**: they parse
//! and validate today but aren't acted on yet, so a full environment manager is
//! an additive change rather than a migration. Nothing here uses
//! `deny_unknown_fields`, so a manifest written for a newer Forge still loads.

use serde::Deserialize;
use std::path::Path;

/// The schema version this build understands. A manifest declaring a higher
/// version still loads (fields we don't know are ignored), with a warning.
pub const SUPPORTED_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Manifest {
    /// Manifest schema version. `0` (unset) is treated as [`SUPPORTED_SCHEMA`].
    pub schema: u32,
    pub environment: Environment,
    pub toolchain: Toolchain,
    /// Reserved: native/system libraries (BLAS, LAPACK, OpenSSL, …). Kept as a
    /// raw table so unknown keys never error and its mere presence is detectable.
    pub native: toml::Table,
    /// Reserved: GPU backend + toolkit selection.
    pub gpu: toml::Table,
    /// Reserved: managed Python interop environment.
    pub python: toml::Table,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Environment {
    pub name: Option<String>,
    pub profile: Option<String>,
    pub channel: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Toolchain {
    /// Pinned Rust toolchain (e.g. `"1.98.0"`). Source of truth; a
    /// `rust-toolchain.toml` can be generated from it for cargo/rustup.
    pub rust: Option<String>,
}

impl Manifest {
    pub const FILE_NAME: &'static str = "forge.toml";

    /// Parse a manifest from TOML text.
    pub fn parse(text: &str) -> Result<Manifest, String> {
        toml::from_str::<Manifest>(text).map_err(|error| error.to_string())
    }

    /// Load `<project_root>/forge.toml`. Returns `Ok(None)` when the file is
    /// absent (the common case) and `Err` only on a present-but-invalid manifest.
    pub fn load(project_root: &Path) -> Result<Option<Manifest>, String> {
        let path = project_root.join(Self::FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    /// The effective schema version (unset ⇒ current).
    pub fn schema_version(&self) -> u32 {
        if self.schema == 0 {
            SUPPORTED_SCHEMA
        } else {
            self.schema
        }
    }

    /// The tracked channel, defaulting to `stable`.
    pub fn channel(&self) -> &str {
        self.environment.channel.as_deref().unwrap_or("stable")
    }

    /// Reserved sections that carry configuration but aren't implemented yet, in
    /// declaration order. These drive the "recognized, not yet active" diagnostics.
    pub fn reserved_in_use(&self) -> Vec<&'static str> {
        let mut sections = Vec::new();
        if !self.native.is_empty() {
            sections.push("native");
        }
        if !self.gpu.is_empty() {
            sections.push("gpu");
        }
        if !self.python.is_empty() {
            sections.push("python");
        }
        sections
    }

    /// Non-fatal advisories about this manifest (newer schema, reserved sections).
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.schema_version() > SUPPORTED_SCHEMA {
            out.push(format!(
                "forge.toml declares schema {} but this build supports {SUPPORTED_SCHEMA}; \
                 newer fields are ignored.",
                self.schema_version()
            ));
        }
        for section in self.reserved_in_use() {
            out.push(format!(
                "[{section}] is recognized but not yet active in this build; \
                 it will be honored once a provider supports it."
            ));
        }
        out
    }
}
