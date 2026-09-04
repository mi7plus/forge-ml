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
        // A structural fingerprint over the toolchain version plus every bundle
        // file's relative path and size. This is content-sensitive (it changes if
        // a file is added, removed, or resized) yet stat-only, so it stays fast
        // enough for an on-demand `forge env sync` without reading ~1.6 GB. Only
        // materialize walks the tree; the startup activation path never does.
        let root = runtime.bin.parent().unwrap_or(&runtime.bin);
        let sha256 = sha256_hex(structural_fingerprint(root, &runtime.version).as_bytes());
        let mut extra = toml::Table::new();
        extra.insert(
            "vendor".to_owned(),
            toml::Value::String(runtime.vendor.display().to_string()),
        );
        Ok(LockEntry {
            id: self.id().to_owned(),
            kind: "offline-bundle".to_owned(),
            version: runtime.version.clone(),
            sha256,
            extra,
        })
    }
}

/// Build a stable, stat-only digest string of `root`: the version line followed
/// by every file's `relative/path:size`, sorted for determinism.
fn structural_fingerprint(root: &std::path::Path, version: &str) -> String {
    let mut entries: Vec<(String, u64)> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            match entry.file_type() {
                Ok(file_type) if file_type.is_dir() => stack.push(entry.path()),
                Ok(file_type) if file_type.is_file() => {
                    let path = entry.path();
                    let relative = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                    entries.push((relative, size));
                }
                _ => {}
            }
        }
    }
    entries.sort();
    let mut digest = format!("version={version}\n");
    for (relative, size) in &entries {
        digest.push_str(relative);
        digest.push(':');
        digest.push_str(&size.to_string());
        digest.push('\n');
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn structural_fingerprint_is_stable_and_content_sensitive() {
        let root = std::env::temp_dir().join(format!("forge-fp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("bin/cargo"), b"x").unwrap();
        fs::write(root.join("VERSION"), b"rustc 1.98.0").unwrap();

        let a = structural_fingerprint(&root, "v1");
        let b = structural_fingerprint(&root, "v1");
        assert_eq!(a, b, "same tree hashes the same");

        // A resized file changes the digest.
        fs::write(root.join("bin/cargo"), b"xxxx").unwrap();
        assert_ne!(a, structural_fingerprint(&root, "v1"));
        // A different version changes the digest.
        assert_ne!(a, structural_fingerprint(&root, "v2"));

        let _ = fs::remove_dir_all(&root);
    }
}
