//! Offline runtime bundle: locating and activating a self-contained Rust
//! toolchain + vendored dependency cache shipped inside the packaged installer,
//! so notebook cells and generated projects that use Millwright and Burn compile
//! and run **without any network access or a user-installed toolchain**.
//!
//! The bundle is produced by `packaging/build-offline-bundle.*` and laid out as:
//!
//! ```text
//! forge-runtime/
//!   bin/            cargo, rustc, rust-std (+ the sysroot they need)
//!   cargo-home/
//!     config.toml   [net] offline = true; crates.io → vendored-sources
//!     vendor/       every crate in the blessed lockfile, pre-downloaded
//!   evcxr-cache/    (optional) pre-built compilation cache for the blessed deps
//!   VERSION         the rustc version string, for display
//! ```
//!
//! When no bundle is present (development builds, or an installer built without
//! one) every function here is an inert no-op and the app falls back to whatever
//! `cargo`/`rustc` are on the system `PATH`, exactly as before.

use std::path::PathBuf;
use std::sync::OnceLock;

/// The directory name the packager ships the runtime bundle under, searched for
/// next to the executable and in the platform resource locations.
const BUNDLE_DIR_NAME: &str = "forge-runtime";

/// A located, usable offline runtime bundle.
#[derive(Debug, Clone)]
pub struct OfflineRuntime {
    /// Directory holding the bundled `cargo`/`rustc` binaries.
    pub bin: PathBuf,
    /// Directory of vendored crate sources (`cargo vendor` output).
    pub vendor: PathBuf,
    /// Reported toolchain version (contents of `VERSION`), for the UI.
    pub version: String,
}

fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            // Next to the executable (Windows/NSIS) and in the macOS bundle's
            // Resources dir, mirroring how the bundled rust-analyzer is found.
            roots.push(parent.join(BUNDLE_DIR_NAME));
            roots.push(parent.join("resources").join(BUNDLE_DIR_NAME));
            roots.push(parent.join("..").join("Resources").join(BUNDLE_DIR_NAME));
            // Debian and AppImage put the binary in <prefix>/bin and cargo-packager
            // resources in <prefix>/lib/forge_ide/, so look one level up from the
            // exe. Relative to the exe so it resolves inside a mounted AppImage too.
            roots.push(
                parent
                    .join("..")
                    .join("lib")
                    .join("forge_ide")
                    .join(BUNDLE_DIR_NAME),
            );
            roots.push(PathBuf::from("/usr/lib/forge_ide").join(BUNDLE_DIR_NAME));
        }
    }
    // An explicit override, primarily for testing the wiring end-to-end.
    if let Some(dir) = std::env::var_os("FORGE_RUNTIME_DIR") {
        roots.insert(0, PathBuf::from(dir));
    }
    roots
}

/// A bundle is valid only if it carries both the toolchain `bin` dir and a
/// `vendor` dir of crate sources — otherwise activating it would break the
/// runtime rather than sandbox it. (The offline cargo config is generated at
/// activation time with an absolute vendor path; cargo rejects a relative one.)
fn validate(root: PathBuf) -> Option<OfflineRuntime> {
    let bin = root.join("bin");
    let vendor = root.join("vendor");
    let has_cargo = bin.join(exe_name("cargo")).is_file();
    if !(has_cargo && vendor.is_dir()) {
        return None;
    }
    let version = std::fs::read_to_string(root.join("VERSION"))
        .map(|text| text.trim().to_owned())
        .unwrap_or_else(|_| "bundled".to_owned());
    Some(OfflineRuntime {
        bin,
        vendor,
        version,
    })
}

fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    }
}

/// Locate the offline runtime bundle, if this build ships one. Result is cached.
pub fn detect() -> Option<&'static OfflineRuntime> {
    static CACHE: OnceLock<Option<OfflineRuntime>> = OnceLock::new();
    CACHE
        .get_or_init(|| candidate_roots().into_iter().find_map(validate))
        .as_ref()
}

/// A writable per-user directory for evcxr's build scratch. The bundle itself may
/// live in a read-only install location (e.g. Program Files), so builds must go
/// elsewhere.
fn scratch_dir() -> PathBuf {
    user_base().join("forge-ml").join("evcxr")
}

/// Point this process's environment at the bundled toolchain and vendored,
/// offline dependency cache so any `cargo`/`rustc` it spawns — notably evcxr's —
/// resolves and builds entirely offline. Idempotent; a no-op without a bundle.
///
/// Returns the activated runtime (also retrievable later via [`detect`]).
pub fn activate() -> Option<&'static OfflineRuntime> {
    let runtime = detect()?;

    // Prepend the bundled bin dir so the bundled cargo/rustc win over any system
    // toolchain.
    let sep = if cfg!(windows) { ";" } else { ":" };
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut new_path = runtime.bin.clone().into_os_string();
    if !path.is_empty() {
        new_path.push(sep);
        new_path.push(path);
    }
    std::env::set_var("PATH", new_path);

    // The install location may be read-only (e.g. Program Files), so use a
    // writable per-user CARGO_HOME and write the offline config there. cargo
    // rejects a relative `directory`, and the vendor path isn't known until
    // install time, so we generate the config now with the absolute path.
    if let Some(cargo_home) = writable_cargo_home(runtime) {
        std::env::set_var("CARGO_HOME", cargo_home);
    }
    std::env::set_var("CARGO_NET_OFFLINE", "true");

    // A rustup shim on PATH would otherwise try to resolve a toolchain and reach
    // the network; pin evcxr's build scratch to a writable location.
    std::env::set_var("RUSTUP_TOOLCHAIN", "");
    let scratch = scratch_dir();
    let _ = std::fs::create_dir_all(&scratch);
    std::env::set_var("EVCXR_TMPDIR", &scratch);

    Some(runtime)
}

/// Create (once) a writable `CARGO_HOME` whose `config.toml` forces offline mode
/// and replaces crates.io with the bundle's absolute vendor directory. Returns
/// the directory, or `None` if it could not be written.
fn writable_cargo_home(runtime: &OfflineRuntime) -> Option<PathBuf> {
    let cargo_home = user_base().join("forge-ml").join("cargo-home");
    std::fs::create_dir_all(&cargo_home).ok()?;
    // TOML strings take forward slashes on every platform, avoiding backslash
    // escaping on Windows.
    let vendor = runtime.vendor.display().to_string().replace('\\', "/");
    let config = format!(
        "# Generated by Forge ML; points cargo at the bundled offline runtime.\n\
         [net]\noffline = true\n\n\
         [source.crates-io]\nreplace-with = \"vendored-sources\"\n\n\
         [source.vendored-sources]\ndirectory = \"{vendor}\"\n"
    );
    std::fs::write(cargo_home.join("config.toml"), config).ok()?;
    Some(cargo_home)
}

/// Base directory for Forge's writable per-user state (cargo home, evcxr scratch).
fn user_base() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
}

/// One-line description of the runtime state for the status bar / about box.
pub fn status_line() -> String {
    match detect() {
        Some(runtime) => format!("Offline Rust runtime bundled ({})", runtime.version),
        None => "System Rust toolchain (notebook `:dep` needs network)".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_bundle_is_a_no_op() {
        // With no bundle configured, detection yields nothing and activation is
        // inert. (No FORGE_RUNTIME_DIR set in the default test environment.)
        if std::env::var_os("FORGE_RUNTIME_DIR").is_none() {
            assert!(validate(PathBuf::from("/nonexistent/forge-runtime")).is_none());
        }
    }

    #[test]
    fn validate_requires_cargo_and_vendor() {
        let dir = std::env::temp_dir().join(format!(
            "forge-runtime-test-{}",
            std::process::id()
        ));
        let bin = dir.join("bin");
        let vendor = dir.join("vendor");
        std::fs::create_dir_all(&bin).unwrap();

        // Missing both cargo and vendor → invalid.
        assert!(validate(dir.clone()).is_none());

        std::fs::write(bin.join(exe_name("cargo")), b"").unwrap();
        // cargo present but no vendor dir → invalid.
        assert!(validate(dir.clone()).is_none());

        std::fs::create_dir_all(&vendor).unwrap();
        std::fs::write(dir.join("VERSION"), b"rustc 1.98.0\n").unwrap();
        let runtime = validate(dir.clone()).expect("valid bundle");
        assert_eq!(runtime.version, "rustc 1.98.0");
        assert_eq!(runtime.bin, bin);
        assert_eq!(runtime.vendor, vendor);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
