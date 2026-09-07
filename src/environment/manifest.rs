//! `forge.toml` — the human-authored environment manifest that sits beside
//! `Cargo.toml`. It declares the *environment* Forge activates; Cargo still owns
//! crate resolution. It is optional: a project without one gets the defaults
//! (bundled runtime, current toolchain).
//!
//! The `[gpu]` section is **active** (Phase 4): a `GpuProvider` reads its
//! `backend` / `require` and reports coverage or a gap in `forge doctor`. The
//! `[native]` and `[python]` sections remain **reserved** — they parse and
//! validate today but aren't acted on yet, so a full environment manager stays
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
    /// Active (Phase 4): GPU backend selection, read via [`Manifest::gpu_request`]
    /// and covered by the `GpuProvider`. Kept as a raw table for forward-compat.
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

/// A typed view of the `[gpu]` manifest section, read by the GPU provider.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GpuRequest {
    /// Whether a `[gpu]` section is present at all.
    pub present: bool,
    /// Requested backend: `cuda` | `rocm` | `metal` | `directml` | `webgpu` |
    /// `none`. `None` means "any available backend".
    pub backend: Option<String>,
    /// An optional CUDA toolkit version constraint (e.g. `"13"`).
    pub cuda: Option<String>,
    /// Fail (rather than fall back to the CPU) if the backend is absent.
    pub require: bool,
}

/// Reserved manifest sections that now have a provider and so are *not* flagged
/// as "recognized but not yet active". `[gpu]` graduated here in Phase 4.
const ACTIVE_RESERVED: &[&str] = &["gpu"];

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

    /// The typed `[gpu]` request. Absent section ⇒ a default (not present).
    pub fn gpu_request(&self) -> GpuRequest {
        let table = &self.gpu;
        let string = |key: &str| table.get(key).and_then(toml::Value::as_str).map(str::to_owned);
        GpuRequest {
            present: !table.is_empty(),
            backend: string("backend"),
            // `cuda` may be written as a string ("13") or a bare number (13).
            cuda: string("cuda").or_else(|| {
                table
                    .get("cuda")
                    .and_then(toml::Value::as_integer)
                    .map(|value| value.to_string())
            }),
            require: table
                .get("require")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
        }
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
            if ACTIVE_RESERVED.contains(&section) {
                continue; // has a provider now; its real status shows in `doctor`.
            }
            out.push(format!(
                "[{section}] is recognized but not yet active in this build; \
                 it will be honored once a provider supports it."
            ));
        }
        out
    }
}
