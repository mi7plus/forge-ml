//! `forge.toml` — the human-authored environment manifest that sits beside
//! `Cargo.toml`. It declares the *environment* Forge activates; Cargo still owns
//! crate resolution. It is optional: a project without one gets the defaults
//! (bundled runtime, current toolchain).
//!
//! Every reserved section is now **active**, each behind a provider that reports
//! coverage or a gap in `forge doctor`: `[gpu]` (Phase 4), `[native]` (Phase 5,
//! detect-and-bridge plus a pinned prebuilt channel), `[python]` (Phase 6, a
//! reference to a uv/pixi env, with declared `packages` installed on request),
//! and `[cargo]` (Phase 7, crate dependencies Forge provisions with `cargo add`).
//! Nothing here uses `deny_unknown_fields`, so a manifest written for a newer
//! Forge still loads.

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
    /// Active (Phase 5): native/system libraries (BLAS, OpenSSL, tools), read via
    /// [`Manifest::native_request`] and checked (not installed) by the
    /// `NativeLibProvider`. Kept as a raw table for forward-compat.
    pub native: toml::Table,
    /// Active (Phase 4): GPU backend selection, read via [`Manifest::gpu_request`]
    /// and covered by the `GpuProvider`. Kept as a raw table for forward-compat.
    pub gpu: toml::Table,
    /// Active (Phase 6): a reference to a Python interop environment (version,
    /// manager, bridge), read via [`Manifest::python_request`] and checked (not
    /// created) by the `PythonProvider`. Kept as a raw table for forward-compat.
    pub python: toml::Table,
    /// Active (Phase 7): Cargo crates the project depends on, read via
    /// [`Manifest::cargo_request`] and provisioned into `Cargo.toml`/`Cargo.lock`
    /// by the `CrateProvider` (`forge cargo provide`). Kept as a raw table for
    /// forward-compat.
    pub cargo: toml::Table,
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

/// A typed view of the `[native]` manifest section, read by the native-lib
/// provider. Forge does not resolve or install these; the provider *bridges* to
/// the system's own package manager (pkg-config / vcpkg / apt / brew), checking
/// presence and giving guidance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeRequest {
    /// Whether a `[native]` section is present at all.
    pub present: bool,
    /// Requested BLAS: `openblas` | `mkl` | `accelerate` | `system` | `none`.
    pub blas: Option<String>,
    /// OpenSSL sourcing: `vendored` (no system dep) | `system`.
    pub openssl: Option<String>,
    /// System tools/packages the build expects on PATH (e.g. `cmake`, `protobuf`).
    pub pkgs: Vec<String>,
    /// Fail (rather than warn) when a prerequisite is missing.
    pub require: bool,
}

/// A typed view of the `[python]` manifest section, read by the Python provider.
/// Forge references the interpreter/env (uv/pixi own the env itself), but with
/// `packages` declared it will **install** them into that env on request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PythonRequest {
    /// Whether a `[python]` section is present at all.
    pub present: bool,
    /// Requested interpreter version (major.minor, e.g. `3.13`).
    pub version: Option<String>,
    /// The environment manager that owns the env: `uv` | `pixi` | `system`.
    pub manager: Option<String>,
    /// Bridge surfaces the project uses (`arrow`, `onnx`, `numpy`, …) —
    /// informational; Forge does not install them.
    pub bridge: Vec<String>,
    /// Python packages to install into the referenced env (pip/uv specs, e.g.
    /// `numpy`, `pandas>=2`). Provisioned by `forge python provide`.
    pub packages: Vec<String>,
    /// Fail (rather than warn) when the env is missing or the version mismatches.
    pub require: bool,
}

/// A typed view of the `[cargo]` manifest section, read by the crate provider.
/// Unlike the other sections, Forge here *drives* Cargo: `forge cargo provide`
/// runs `cargo add` for each declared crate and fetches the graph, so the
/// manifest can be the single place a project's crate dependencies are declared.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CargoRequest {
    /// Whether a `[cargo]` section is present at all.
    pub present: bool,
    /// Crate specs, each accepted by `cargo add` (`serde`, `polars@0.40`, …).
    pub crates: Vec<String>,
    /// Fail (rather than warn) when a declared crate is missing from `Cargo.toml`.
    pub require: bool,
}

/// Manifest sections that now have a provider and so are *not* flagged as
/// "recognized but not yet active". `[gpu]` (Phase 4), `[native]` (Phase 5), and
/// `[python]` (Phase 6) have all graduated — no reserved sections remain.
const ACTIVE_RESERVED: &[&str] = &["gpu", "native", "python", "cargo"];

/// Collect a `key = ["a", "b"]` string array from a raw section table, dropping
/// non-string entries. Missing key ⇒ empty vec.
fn string_array(table: &toml::Table, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
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

    /// The typed `[python]` request. Absent section ⇒ a default (not present).
    pub fn python_request(&self) -> PythonRequest {
        let table = &self.python;
        let string = |key: &str| table.get(key).and_then(toml::Value::as_str).map(str::to_owned);
        PythonRequest {
            present: !table.is_empty(),
            // `version` may be a string ("3.13") or a bare number (3.13).
            version: string("version").or_else(|| {
                table
                    .get("version")
                    .and_then(toml::Value::as_float)
                    .map(|value| format!("{value}"))
            }),
            manager: string("manager"),
            bridge: string_array(table, "bridge"),
            packages: string_array(table, "packages"),
            require: table
                .get("require")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
        }
    }

    /// The typed `[cargo]` request. Absent section ⇒ a default (not present).
    pub fn cargo_request(&self) -> CargoRequest {
        let table = &self.cargo;
        CargoRequest {
            present: !table.is_empty(),
            crates: string_array(table, "crates"),
            require: table
                .get("require")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
        }
    }

    /// The typed `[native]` request. Absent section ⇒ a default (not present).
    pub fn native_request(&self) -> NativeRequest {
        let table = &self.native;
        let string = |key: &str| table.get(key).and_then(toml::Value::as_str).map(str::to_owned);
        NativeRequest {
            present: !table.is_empty(),
            blas: string("blas"),
            openssl: string("openssl"),
            pkgs: string_array(table, "pkgs"),
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
        if !self.cargo.is_empty() {
            sections.push("cargo");
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
