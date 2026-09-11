//! Locating Forge's sibling helper binaries (`forge_cef`, `forge_webview`,
//! `forge_manager`) whether running from a dev build or an installed package.
//!
//! In a `cargo` build all workspace binaries sit in the same `target/<profile>`
//! directory, so the helper is simply next to the running exe. In an installer
//! the packager stages the helpers (and, for `forge_cef`, the CEF runtime) into
//! a `helpers/` resource directory, which lands in different places per format —
//! next to the exe on Windows/NSIS, under `Resources/` in a macOS bundle, under
//! `lib/forge_ide/` for deb/AppImage. This mirrors `offline.rs`'s search for the
//! bundled Rust runtime so the two stay consistent.

use std::path::PathBuf;

/// Absolute path to a helper binary named `name` (without extension), trying the
/// dev layout (next to the exe) first, then the packaged `helpers/` locations.
/// Falls back to the bare name (resolved via `PATH`) if none exist.
pub fn locate(name: &str) -> PathBuf {
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidates = [
                // Dev build / Windows NSIS: alongside the running exe.
                parent.join(&file),
                // Packaged resource dir, next to the exe (Windows) …
                parent.join("helpers").join(&file),
                parent.join("resources").join("helpers").join(&file),
                // … macOS app bundle Resources …
                parent.join("..").join("Resources").join("helpers").join(&file),
                // … deb / AppImage prefix.
                parent
                    .join("..")
                    .join("lib")
                    .join("forge_ide")
                    .join("helpers")
                    .join(&file),
            ];
            if let Some(found) = candidates.into_iter().find(|c| c.is_file()) {
                return found;
            }
        }
    }
    PathBuf::from(file)
}
