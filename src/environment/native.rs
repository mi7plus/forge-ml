//! The native-library provider (Forge Distribution, Phase 5) — a deliberate
//! **detect-and-bridge**, not a resolver.
//!
//! Cross-platform native-dependency resolution (BLAS/LAPACK/OpenSSL/…) is the
//! genuinely conda-shaped problem, and building a from-scratch resolver for it is
//! the trap the roadmap warns against: high cost, and Rust needs it far less than
//! Python (Forge's own ML stack — linfa/smartcore/Millwright — is pure Rust and
//! needs no system BLAS). So this provider **checks** whether a project's
//! declared `[native]` prerequisites are present on the machine and gives
//! package-manager guidance when they are not; it never downloads, builds, or
//! installs anything. That is the "thin bridge to an existing package manager"
//! the plan calls for, and it makes `[native]` a live, honest section without
//! taking on the resolver.

use super::diagnostics::tool_version;
use super::lock::{sha256_hex, LockEntry};
use super::manifest::{Manifest, NativeRequest};
use super::provider::{Activation, Capabilities, EnvironmentProvider, Probe};
use std::process::Command;

/// The status of one declared native prerequisite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prereq {
    pub name: String,
    pub satisfied: bool,
    /// How it was satisfied, or why it is considered missing.
    pub detail: String,
    /// Package-manager guidance when unsatisfied (empty otherwise).
    pub hint: String,
}

/// Check each prerequisite a `[native]` request declares against the system.
/// Best-effort and read-only; it consults pkg-config and PATH, nothing more.
pub fn statuses(request: &NativeRequest) -> Vec<Prereq> {
    let mut out = Vec::new();

    if let Some(blas) = request.blas.as_deref() {
        out.push(blas_status(blas));
    }
    if let Some(openssl) = request.openssl.as_deref() {
        out.push(openssl_status(openssl));
    }
    for pkg in &request.pkgs {
        out.push(tool_status(pkg));
    }
    out
}

fn blas_status(kind: &str) -> Prereq {
    match kind {
        "none" => satisfied("blas", "not required"),
        "accelerate" if cfg!(target_os = "macos") => {
            satisfied("blas (accelerate)", "Apple Accelerate framework (built in)")
        }
        "accelerate" => missing(
            "blas (accelerate)",
            "Accelerate is macOS-only",
            "choose openblas or none on this platform",
        ),
        other => {
            if pkg_config_has(other) {
                satisfied(&format!("blas ({other})"), "found via pkg-config")
            } else {
                // Not fatal by nature: Forge's own stack needs no system BLAS.
                missing(
                    &format!("blas ({other})"),
                    "not found via pkg-config (Forge's own ML stack needs no system BLAS)",
                    &install_hint(other),
                )
            }
        }
    }
}

fn openssl_status(kind: &str) -> Prereq {
    match kind {
        "vendored" => satisfied("openssl (vendored)", "no system dependency"),
        _ => {
            if pkg_config_has("openssl") || tool_version("openssl", &["version"]).is_some() {
                satisfied("openssl (system)", "found on the system")
            } else {
                missing(
                    "openssl (system)",
                    "not found",
                    &install_hint("openssl"),
                )
            }
        }
    }
}

fn tool_status(pkg: &str) -> Prereq {
    if tool_version(pkg, &["--version"]).is_some() {
        satisfied(pkg, "on PATH")
    } else {
        missing(pkg, "not on PATH", &install_hint(pkg))
    }
}

/// A per-platform install hint that bridges to the user's own package manager.
fn install_hint(pkg: &str) -> String {
    if cfg!(target_os = "linux") {
        format!("install with your package manager, e.g. `apt install {pkg}` / `dnf install {pkg}`")
    } else if cfg!(target_os = "macos") {
        format!("install with Homebrew: `brew install {pkg}`")
    } else {
        format!("install with vcpkg (`vcpkg install {pkg}`) or winget")
    }
}

fn pkg_config_has(lib: &str) -> bool {
    Command::new("pkg-config")
        .args(["--exists", lib])
        .status()
        .is_ok_and(|status| status.success())
}

fn satisfied(name: &str, detail: &str) -> Prereq {
    Prereq {
        name: name.to_owned(),
        satisfied: true,
        detail: detail.to_owned(),
        hint: String::new(),
    }
}

fn missing(name: &str, detail: &str, hint: &str) -> Prereq {
    Prereq {
        name: name.to_owned(),
        satisfied: false,
        detail: detail.to_owned(),
        hint: hint.to_owned(),
    }
}

/// A human report for `forge native check`.
pub fn report(request: &NativeRequest) -> String {
    if !request.present {
        return "No [native] section in forge.toml — nothing to check. Forge's own ML \
                stack is pure Rust and needs no system libraries.\n"
            .to_owned();
    }
    let statuses = statuses(request);
    let mut out = String::from("Native prerequisites:\n");
    if statuses.is_empty() {
        out.push_str("  (none declared)\n");
    }
    for prereq in &statuses {
        if prereq.satisfied {
            out.push_str(&format!("  [ok  ] {:<20} {}\n", prereq.name, prereq.detail));
        } else {
            out.push_str(&format!("  [MISS] {:<20} {} — {}\n", prereq.name, prereq.detail, prereq.hint));
        }
    }
    out.push_str(
        "\nForge checks these against your system; it does not install them. Use your \
         platform's package manager (the hints above) — Forge bridges to it rather than \
         resolving native dependencies itself.\n",
    );
    out
}

pub struct NativeLibProvider;

impl EnvironmentProvider for NativeLibProvider {
    fn id(&self) -> &'static str {
        "native-lib"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            native: true,
            ..Capabilities::default()
        }
    }

    fn probe(&self, manifest: &Manifest) -> Probe {
        let request = manifest.native_request();
        // Cheap path: no `[native]` section (startup/default) — no system probing.
        if !request.present {
            return Probe::Missing("no [native] section in forge.toml".to_owned());
        }
        evaluate(&request, &statuses(&request))
    }

    fn activate(&self, _manifest: &Manifest) -> Option<Activation> {
        // The bridge installs nothing and mutates no environment; system libraries
        // are found by the build (pkg-config/linker) where the OS already has them.
        None
    }

    fn materialize(&self, manifest: &Manifest) -> Result<LockEntry, String> {
        let request = manifest.native_request();
        let statuses = statuses(&request);
        let mut extra = toml::Table::new();
        extra.insert("require".to_owned(), toml::Value::Boolean(request.require));
        let satisfied: Vec<toml::Value> = statuses
            .iter()
            .filter(|prereq| prereq.satisfied)
            .map(|prereq| toml::Value::String(prereq.name.clone()))
            .collect();
        let missing: Vec<toml::Value> = statuses
            .iter()
            .filter(|prereq| !prereq.satisfied)
            .map(|prereq| toml::Value::String(prereq.name.clone()))
            .collect();
        extra.insert("satisfied".to_owned(), toml::Value::Array(satisfied));
        extra.insert("missing".to_owned(), toml::Value::Array(missing));
        let version = if statuses.iter().all(|prereq| prereq.satisfied) {
            "satisfied"
        } else {
            "unsatisfied"
        };
        Ok(LockEntry {
            id: self.id().to_owned(),
            kind: "native-libs".to_owned(),
            version: version.to_owned(),
            sha256: sha256_hex(version.as_bytes()),
            extra,
        })
    }
}

/// Pure probe logic: unmet prerequisites are a hard gap only when `require` is
/// set; otherwise Forge reports them but stays available (the curated bundle or a
/// crate's vendored feature usually covers the need).
fn evaluate(request: &NativeRequest, statuses: &[Prereq]) -> Probe {
    let unmet: Vec<&str> = statuses
        .iter()
        .filter(|prereq| !prereq.satisfied)
        .map(|prereq| prereq.name.as_str())
        .collect();
    if unmet.is_empty() {
        Probe::Available
    } else if request.require {
        Probe::Incompatible(format!(
            "[native] requires {} but not found; see `forge native check`",
            unmet.join(", ")
        ))
    } else {
        Probe::Available
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(require: bool) -> NativeRequest {
        NativeRequest {
            present: true,
            blas: None,
            openssl: None,
            pkgs: Vec::new(),
            require,
        }
    }

    fn met(name: &str) -> Prereq {
        satisfied(name, "ok")
    }
    fn unmet(name: &str) -> Prereq {
        missing(name, "not found", "install it")
    }

    #[test]
    fn all_satisfied_is_available() {
        assert_eq!(evaluate(&request(true), &[met("cmake")]), Probe::Available);
    }

    #[test]
    fn missing_required_is_incompatible() {
        let probe = evaluate(&request(true), &[met("cmake"), unmet("protobuf")]);
        assert!(matches!(probe, Probe::Incompatible(reason) if reason.contains("protobuf")));
    }

    #[test]
    fn missing_optional_still_available() {
        assert_eq!(
            evaluate(&request(false), &[unmet("protobuf")]),
            Probe::Available
        );
    }

    #[test]
    fn vendored_openssl_needs_no_system_lib() {
        let prereq = openssl_status("vendored");
        assert!(prereq.satisfied);
    }

    #[test]
    fn blas_none_is_satisfied_and_accelerate_is_macos_only() {
        assert!(blas_status("none").satisfied);
        assert_eq!(blas_status("accelerate").satisfied, cfg!(target_os = "macos"));
    }
}
