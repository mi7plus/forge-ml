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
use super::provision::{self, Catalog, Fetcher, HttpFetcher};
use std::path::Path;
use std::process::Command;

/// The per-project curated catalog of pinned native prebuilts (`forge native
/// pin` appends to it; `forge native provide` reads it).
pub const CATALOG_FILE: &str = "forge-native.toml";

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

/// The starter catalog Forge ships (cmake / protoc / ninja …), pinned and
/// refreshed by CI. Embedded so `forge native provide <tool>` works out of the
/// box; empty until the generator has run at least once.
pub fn default_catalog() -> Result<Catalog, String> {
    Catalog::parse(include_str!("../../packaging/native-catalog.toml"))
}

/// Load the project's pinned catalog (`forge-native.toml`), or an empty catalog
/// when there is none.
pub fn load_catalog(root: &Path) -> Result<Catalog, String> {
    match std::fs::read_to_string(root.join(CATALOG_FILE)) {
        Ok(text) => Catalog::parse(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Catalog::default()),
        Err(error) => Err(error.to_string()),
    }
}

/// Whether a tool named `name` is already runnable on the system.
fn on_system(name: &str) -> bool {
    tool_version(name, &["--version"]).is_some()
}

/// `forge native provide` — download, verify, extract, and expose every catalog
/// artifact that ships a prebuilt for this host. Writes the resulting `PATH`/env
/// exposure to `<root>/.forge/native-env`, which `forge run`/`build`/`test`
/// apply. Real network access; nothing downloaded is ever executed.
pub fn provide(root: &Path, only: &[String]) -> String {
    let default = default_catalog().unwrap_or_default();
    let project = match load_catalog(root) {
        Ok(catalog) => catalog,
        Err(error) => return format!("forge native provide: reading {CATALOG_FILE}: {error}\n"),
    };
    let lookup = |name: &str| project.find(name).or_else(|| default.find(name));

    // What to provision. With explicit names (the Manager's per-tool Install),
    // provision exactly those from either catalog. Otherwise: everything the
    // project pinned itself, plus any tool the manifest's `[native].pkgs` needs
    // that is missing on the system and shipped in the embedded catalog.
    let mut selected: Vec<&provision::Artifact> = Vec::new();
    if only.is_empty() {
        selected.extend(project.artifacts.iter());
        let manifest = Manifest::load(root).ok().flatten().unwrap_or_default();
        for pkg in &manifest.native_request().pkgs {
            if selected.iter().any(|artifact| &artifact.name == pkg) {
                continue;
            }
            if !on_system(pkg) {
                if let Some(artifact) = default.find(pkg) {
                    selected.push(artifact);
                }
            }
        }
    } else {
        for name in only {
            match lookup(name) {
                Some(artifact) if !selected.iter().any(|a| a.name == artifact.name) => {
                    selected.push(artifact)
                }
                Some(_) => {}
                None => {
                    return format!("forge native provide: `{name}` is not in the catalog.\n")
                }
            }
        }
    }

    if selected.is_empty() {
        return format!(
            "Nothing to provide. Declare tools in `[native].pkgs` (provided from Forge's \
             catalog when missing) or pin your own prebuilts in {CATALOG_FILE} with \
             `forge native pin`.\n"
        );
    }

    let cache = root.join(".forge").join("native");
    let fetcher = HttpFetcher;
    let mut env_lines = Vec::new();
    let mut out = String::from("Providing native artifacts:\n");
    for artifact in &selected {
        let Some(entry) = artifact.for_host() else {
            out.push_str(&format!(
                "  [skip] {:<14} no prebuilt for {} in the catalog\n",
                artifact.name,
                provision::host_target()
            ));
            continue;
        };
        match provision::provision(&artifact.name, entry, &cache, &fetcher) {
            Ok(exposure) => {
                out.push_str(&format!("  [ok  ] {:<14} provided\n", artifact.name));
                for dir in &exposure.path_dirs {
                    env_lines.push(format!("path\t{}", dir.display()));
                }
                for (key, value) in &exposure.env {
                    env_lines.push(format!("env\t{key}={value}"));
                }
            }
            Err(error) => out.push_str(&format!("  [FAIL] {:<14} {error}\n", artifact.name)),
        }
    }

    let forge_dir = root.join(".forge");
    if let Err(error) = std::fs::create_dir_all(&forge_dir)
        .and_then(|()| std::fs::write(forge_dir.join("native-env"), env_lines.join("\n") + "\n"))
    {
        out.push_str(&format!("  (could not write .forge/native-env: {error})\n"));
    } else {
        out.push_str(
            "\nWrote .forge/native-env — `forge run`/`build`/`test` apply it automatically.\n",
        );
    }
    out
}

/// `forge native pin` — fetch an artifact, hash it, and print a ready-to-paste
/// catalog entry for the current host so pins are never fabricated by hand.
pub fn pin(url: &str, archive: &str, name: Option<&str>) -> String {
    if !matches!(archive, "zip" | "tar-gz") {
        return format!("forge native pin: --archive must be `zip` or `tar-gz` (got `{archive}`)\n");
    }
    let bytes = match HttpFetcher.fetch(url) {
        Ok(bytes) => bytes,
        Err(error) => return format!("forge native pin: fetching {url}: {error}\n"),
    };
    let sha = crate::experiment::stable_digest(&bytes);
    let name = name.map(str::to_owned).unwrap_or_else(|| guess_name(url));
    format!(
        "# Verified {} bytes. Paste into {CATALOG_FILE} (add other targets similarly):\n\
         [[artifact]]\n\
         name = \"{name}\"\n\
         kind = \"tool\"          # tool | library\n\
         [artifact.targets.{target}]\n\
         url = \"{url}\"\n\
         sha256 = \"{sha}\"\n\
         archive = \"{archive}\"\n\
         bin_dir = \"bin\"        # tools: dir to add to PATH; libraries: use `env` instead\n",
        bytes.len(),
        target = provision::host_target(),
    )
}

fn guess_name(url: &str) -> String {
    url.rsplit('/')
        .next()
        .and_then(|file| file.split(['-', '.', '_']).next())
        .filter(|name| !name.is_empty())
        .unwrap_or("artifact")
        .to_owned()
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

    #[test]
    fn embedded_default_catalog_parses() {
        // The committed packaging/native-catalog.toml must always be valid TOML,
        // whether empty or populated by CI.
        assert!(super::default_catalog().is_ok());
    }
}
