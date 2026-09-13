//! The crate provider (Forge Distribution, Phase 7) — the one section where
//! Forge *drives* the underlying tool rather than only referencing it.
//!
//! `[cargo].crates` lets `forge.toml` be the single place a project declares its
//! Cargo dependencies. `forge cargo check` reports which are already in
//! `Cargo.toml`; `forge cargo provide` runs `cargo add` for each declared crate
//! and fetches the graph, so `Cargo.toml`/`Cargo.lock` — still the source of
//! truth for resolution — end up carrying exactly what the manifest asked for.
//! Forge never re-implements Cargo's resolver; it invokes Cargo.

use super::diagnostics::tool_version;
use super::lock::{sha256_hex, LockEntry};
use super::manifest::{CargoRequest, Manifest};
use super::provider::{Activation, Capabilities, EnvironmentProvider, Probe};
use std::path::Path;
use std::process::Command;

/// Whether `cargo` is runnable on this machine.
fn cargo_available() -> bool {
    tool_version("cargo", &["--version"]).is_some()
}

/// The crate name a `cargo add` spec refers to (`polars@0.40` ⇒ `polars`,
/// `serde` ⇒ `serde`), for matching against `Cargo.toml` dependency keys.
fn crate_name(spec: &str) -> &str {
    spec.split(['@', ' ', '=']).next().unwrap_or(spec).trim()
}

/// The dependency names already declared in `<root>/Cargo.toml`, across the
/// `[dependencies]`, `[dev-dependencies]`, and `[build-dependencies]` tables.
fn declared_dependencies(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) else {
        return Vec::new();
    };
    let Ok(value) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = value.get(section).and_then(toml::Value::as_table) {
            names.extend(table.keys().cloned());
        }
    }
    names
}

/// `forge cargo check` — report which declared crates are already in `Cargo.toml`.
pub fn report(request: &CargoRequest, root: &Path) -> String {
    if !request.present {
        return "No [cargo] section in forge.toml — Cargo.toml owns the crate graph. Add \
                a [cargo] section with `crates = [...]` to let `forge cargo provide` keep it \
                in sync.\n"
            .to_owned();
    }
    if !root.join("Cargo.toml").is_file() {
        return "forge cargo check: no Cargo.toml here — run inside a Cargo project.\n".to_owned();
    }
    let declared = declared_dependencies(root);
    let mut out = String::from("Cargo crates (declared in forge.toml):\n");
    if request.crates.is_empty() {
        out.push_str("  (none listed under [cargo].crates)\n");
    }
    for spec in &request.crates {
        let name = crate_name(spec);
        let present = declared.iter().any(|dep| dep == name);
        if present {
            out.push_str(&format!("  [ok  ] {spec}\n"));
        } else {
            out.push_str(&format!("  [MISS] {spec} — not in Cargo.toml\n"));
        }
    }
    out.push_str(
        "\nRun `forge cargo provide` to `cargo add` the missing crates and fetch the graph. \
         Cargo.lock stays the source of truth for resolved versions.\n",
    );
    out
}

/// `forge cargo provide` — `cargo add` each declared crate, then `cargo fetch`.
/// Real changes to `Cargo.toml`/`Cargo.lock`; nothing is executed beyond Cargo.
pub fn provide(root: &Path, request: &CargoRequest) -> String {
    if !request.present || request.crates.is_empty() {
        return "Nothing to provide — declare crates under `[cargo].crates` in forge.toml.\n"
            .to_owned();
    }
    if !root.join("Cargo.toml").is_file() {
        return "forge cargo provide: no Cargo.toml here — run inside a Cargo project.\n"
            .to_owned();
    }
    if !cargo_available() {
        return "forge cargo provide: `cargo` is not on PATH.\n".to_owned();
    }

    let declared = declared_dependencies(root);
    let mut out = String::from("Providing Cargo crates:\n");
    let mut added = 0;
    for spec in &request.crates {
        if declared.iter().any(|dep| dep == crate_name(spec)) {
            out.push_str(&format!("  [have] {spec}\n"));
            continue;
        }
        match Command::new("cargo")
            .current_dir(root)
            .args(["add", spec])
            .output()
        {
            Ok(output) if output.status.success() => {
                out.push_str(&format!("  [ok  ] {spec}\n"));
                added += 1;
            }
            Ok(output) => {
                let err = String::from_utf8_lossy(&output.stderr);
                out.push_str(&format!("  [FAIL] {spec} — {}\n", err.trim().lines().last().unwrap_or("cargo add failed")));
            }
            Err(error) => out.push_str(&format!("  [FAIL] {spec} — {error}\n")),
        }
    }

    // Resolve + populate the lockfile and cache so the graph is ready offline.
    match Command::new("cargo").current_dir(root).arg("fetch").output() {
        Ok(output) if output.status.success() => {
            out.push_str(&format!("\nAdded {added} crate(s); `cargo fetch` resolved the graph.\n"));
        }
        Ok(output) => out.push_str(&format!(
            "\nAdded {added} crate(s), but `cargo fetch` failed: {}\n",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => out.push_str(&format!("\nAdded {added} crate(s); `cargo fetch` could not run: {error}\n")),
    }
    out
}

pub struct CrateProvider;

impl EnvironmentProvider for CrateProvider {
    fn id(&self) -> &'static str {
        "cargo"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            crates: true,
            ..Capabilities::default()
        }
    }

    fn probe(&self, manifest: &Manifest) -> Probe {
        let request = manifest.cargo_request();
        if !request.present {
            return Probe::Missing("no [cargo] section in forge.toml".to_owned());
        }
        // Root-less: the provider can provision iff Cargo is available. The
        // per-crate "declared in Cargo.toml" view is `forge cargo check [dir]`.
        if !cargo_available() {
            return Probe::Missing("`cargo` is not on PATH".to_owned());
        }
        Probe::Available
    }

    fn activate(&self, _manifest: &Manifest) -> Option<Activation> {
        // Crates are provisioned into Cargo.toml/Cargo.lock, not the live PATH.
        None
    }

    fn materialize(&self, manifest: &Manifest) -> Result<LockEntry, String> {
        let request = manifest.cargo_request();
        let mut crates = request.crates.clone();
        crates.sort();
        let joined = crates.join("\n");
        let mut extra = toml::Table::new();
        extra.insert("require".to_owned(), toml::Value::Boolean(request.require));
        extra.insert(
            "crates".to_owned(),
            toml::Value::Array(crates.iter().cloned().map(toml::Value::String).collect()),
        );
        Ok(LockEntry {
            id: self.id().to_owned(),
            kind: "cargo-crates".to_owned(),
            version: format!("{} crate(s)", request.crates.len()),
            sha256: sha256_hex(joined.as_bytes()),
            extra,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_strips_version_and_features() {
        assert_eq!(crate_name("serde"), "serde");
        assert_eq!(crate_name("polars@0.40"), "polars");
        assert_eq!(crate_name("ndarray = 0.15"), "ndarray");
    }

    #[test]
    fn absent_section_probes_missing() {
        let manifest = Manifest::default();
        assert!(matches!(
            CrateProvider.probe(&manifest),
            Probe::Missing(_)
        ));
    }

    #[test]
    fn materialize_records_sorted_crates() {
        let manifest =
            Manifest::parse("[cargo]\ncrates = [\"polars\", \"ndarray\"]\n").unwrap();
        let entry = CrateProvider.materialize(&manifest).unwrap();
        assert_eq!(entry.id, "cargo");
        assert_eq!(entry.kind, "cargo-crates");
        let crates = entry.extra.get("crates").unwrap().as_array().unwrap();
        assert_eq!(crates[0].as_str(), Some("ndarray"));
        assert_eq!(crates[1].as_str(), Some("polars"));
    }
}
