//! The Forge environment system: a declarative manifest ([`Manifest`] /
//! `forge.toml`), a generated lock ([`Lock`] / `forge.lock`), and a
//! provider interface ([`EnvironmentProvider`]) the CLI and IDE resolve through.
//!
//! Today there is one provider — the bundled offline runtime — and the app's
//! behavior is unchanged: [`activate_runtime`] applies exactly the environment it
//! always has. The value now is the *seam*: the reserved `[native]`/`[gpu]`/
//! `[python]` manifest sections and the provider trait mean a full environment
//! manager is added later as new providers and filled-in sections, never a
//! rewrite. See `docs/FORGE_ENV.md`.

mod bundled;
mod lock;
mod manifest;
mod provider;

pub use bundled::BundledRuntimeProvider;
pub use lock::{sha256_hex, Lock};
pub use manifest::Manifest;
pub use provider::{Activation, EnvironmentProvider, Probe};

use std::path::{Path, PathBuf};

/// A reserved manifest section that no available provider can satisfy yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub section: &'static str,
    pub reason: String,
}

/// Composes environment providers into one environment for a manifest.
pub struct Resolver {
    providers: Vec<Box<dyn EnvironmentProvider>>,
}

impl Resolver {
    pub fn new() -> Self {
        Resolver {
            providers: Vec::new(),
        }
    }

    /// The resolver the app ships with: just the bundled offline runtime.
    pub fn default_providers() -> Self {
        let mut resolver = Resolver::new();
        resolver.register(Box::new(BundledRuntimeProvider));
        resolver
    }

    pub fn register(&mut self, provider: Box<dyn EnvironmentProvider>) {
        self.providers.push(provider);
    }

    /// Compose the live activation from every currently-available provider, in
    /// registration order. `None` when nothing contributes (e.g. no bundle).
    pub fn activate_available(&self, manifest: &Manifest) -> Option<Activation> {
        let mut composed: Option<Activation> = None;
        for provider in &self.providers {
            if !provider.probe(manifest).is_available() {
                continue;
            }
            if let Some(activation) = provider.activate(manifest) {
                composed = Some(match composed {
                    Some(existing) => existing.merge(activation),
                    None => activation,
                });
            }
        }
        composed
    }

    /// Materialize the environment for `manifest` into a [`Lock`] (`forge env sync`).
    pub fn sync(&self, manifest: &Manifest, forge_version: &str) -> Result<Lock, String> {
        let mut entries = Vec::new();
        for provider in &self.providers {
            if provider.probe(manifest).is_available() {
                entries.push(provider.materialize(manifest)?);
            }
        }
        Ok(Lock::new(manifest, entries, forge_version))
    }

    /// Reserved manifest sections in use that no available provider covers.
    pub fn gaps(&self, manifest: &Manifest) -> Vec<Gap> {
        manifest
            .reserved_in_use()
            .into_iter()
            .filter_map(|section| {
                let covered = self.providers.iter().any(|provider| {
                    provider.capabilities().covers(section)
                        && provider.probe(manifest).is_available()
                });
                (!covered).then(|| Gap {
                    section,
                    reason: format!("no available provider supports [{section}] in this build"),
                })
            })
            .collect()
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Resolver::default_providers()
    }
}

/// The Forge version recorded in generated locks / reports.
fn forge_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Activate the environment for this process so any `cargo`/`rustc` it spawns
/// (notably evcxr's) uses the bundled offline runtime when present. Preserves the
/// previous behavior exactly: a no-op when no bundle is shipped. Called once at
/// runtime start, before evcxr spawns anything.
pub fn activate_runtime() {
    // The runtime activation uses the default (empty) manifest; a project's
    // forge.toml refines `forge env sync`/`doctor`, not the low-level toolchain
    // activation that must happen before any project is even open.
    let manifest = Manifest::default();
    if let Some(activation) = Resolver::default_providers().activate_available(&manifest) {
        activation.apply();
    }
}

/// Write `<project_root>/forge.lock` for the project's manifest (`forge env sync`).
/// Returns the lock path. Records the `Cargo.lock` hash when one is present.
pub fn sync_project(project_root: &Path) -> Result<PathBuf, String> {
    let manifest = Manifest::load(project_root)?.unwrap_or_default();
    let resolver = Resolver::default_providers();
    let mut lock = resolver.sync(&manifest, forge_version())?;
    if let Ok(cargo_lock) = std::fs::read(project_root.join("Cargo.lock")) {
        lock.cargo.lockfile_sha256 = sha256_hex(&cargo_lock);
    }
    let path = project_root.join("forge.lock");
    std::fs::write(&path, lock.to_toml()?).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}

/// A human-readable environment report for `forge doctor`: the manifest state,
/// which providers are available, and any unsatisfiable reserved sections.
pub fn doctor(project_root: &Path) -> String {
    let mut out = String::new();
    out.push_str(&format!("Forge environment · v{}\n", forge_version()));

    let manifest = match Manifest::load(project_root) {
        Ok(Some(manifest)) => {
            out.push_str(&format!(
                "manifest:   {} (schema {})\n",
                project_root.join(Manifest::FILE_NAME).display(),
                manifest.schema_version()
            ));
            if let Some(profile) = &manifest.environment.profile {
                out.push_str(&format!("profile:    {profile}\n"));
            }
            if let Some(rust) = &manifest.toolchain.rust {
                out.push_str(&format!("toolchain:  rust {rust}\n"));
            }
            manifest
        }
        Ok(None) => {
            out.push_str("manifest:   none (using defaults)\n");
            Manifest::default()
        }
        Err(error) => {
            out.push_str(&format!("manifest:   ERROR {error}\n"));
            Manifest::default()
        }
    };

    let resolver = Resolver::default_providers();
    out.push_str("providers:\n");
    for provider in resolver.providers() {
        let status = match provider.probe(&manifest) {
            Probe::Available => "available".to_owned(),
            Probe::Missing(reason) => format!("missing — {reason}"),
            Probe::Incompatible(reason) => format!("incompatible — {reason}"),
        };
        out.push_str(&format!("  {:<18} {status}\n", provider.id()));
    }

    out.push_str(&format!("runtime:    {}\n", crate::offline::status_line()));

    for warning in manifest.warnings() {
        out.push_str(&format!("warning:    {warning}\n"));
    }
    for gap in resolver.gaps(&manifest) {
        out.push_str(&format!("gap:        {}\n", gap.reason));
    }
    out
}

impl Resolver {
    /// Iterate the registered providers (for reporting).
    pub fn providers(&self) -> impl Iterator<Item = &dyn EnvironmentProvider> {
        self.providers.iter().map(|boxed| boxed.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::lock::LockEntry;
    use super::*;

    const RESERVED: &str = r#"
schema = 1
[environment]
profile = "classical-ml"
[toolchain]
rust = "1.98.0"
[gpu]
backend = "cuda"
cuda = "13"
"#;

    #[test]
    fn manifest_parses_and_flags_reserved_sections() {
        let manifest = Manifest::parse(RESERVED).expect("valid manifest");
        assert_eq!(manifest.environment.profile.as_deref(), Some("classical-ml"));
        assert_eq!(manifest.toolchain.rust.as_deref(), Some("1.98.0"));
        assert_eq!(manifest.channel(), "stable");
        assert_eq!(manifest.reserved_in_use(), vec!["gpu"]);
        assert!(manifest
            .warnings()
            .iter()
            .any(|w| w.contains("[gpu]") && w.contains("not yet active")));
    }

    #[test]
    fn manifest_is_forward_compatible() {
        // A newer schema and an unknown top-level section must still load.
        let text = "schema = 2\n[future_section]\nkey = 1\n[environment]\nname = \"x\"\n";
        let manifest = Manifest::parse(text).expect("tolerant parse");
        assert_eq!(manifest.schema_version(), 2);
        assert!(manifest.warnings().iter().any(|w| w.contains("schema 2")));
    }

    #[test]
    fn gaps_reports_unsupported_reserved_sections() {
        // The default resolver (bundled runtime only) covers no GPU section.
        let manifest = Manifest::parse(RESERVED).unwrap();
        let gaps = Resolver::default_providers().gaps(&manifest);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].section, "gpu");
    }

    #[test]
    fn lock_round_trips() {
        let manifest = Manifest::parse(RESERVED).unwrap();
        let entry = LockEntry {
            id: "bundled-runtime".into(),
            kind: "offline-bundle".into(),
            version: "rustc 1.98.0".into(),
            sha256: sha256_hex(b"x"),
            extra: toml::Table::new(),
        };
        let lock = Lock::new(&manifest, vec![entry], "1.1.0");
        let text = lock.to_toml().unwrap();
        assert!(text.contains("[[provider]]"));
        let restored = Lock::from_toml(&text).unwrap();
        assert_eq!(restored.forge_version, "1.1.0");
        assert_eq!(restored.providers.len(), 1);
        assert_eq!(restored.providers[0].id, "bundled-runtime");
        assert_eq!(restored.environment.channel, "stable");
    }

    #[test]
    fn sha256_matches_known_vector() {
        // SHA-256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
