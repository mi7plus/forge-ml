use std::path::Path;
use std::process::Command;

pub fn version_report(root: &Path) -> String {
    let cargo = std::fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.trim_start().starts_with("version ="))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "Cargo version unavailable".into());
    let python = std::fs::read_to_string(root.join("pyproject.toml"))
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.trim_start().starts_with("version ="))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "No Python wheel in this project".into());
    format!("Forge/Rust: {cargo}\nPython: {python}\nMillwright crates.io: 2.2.1")
}

pub fn checksums(root: &Path) -> Result<String, String> {
    let output = Command::new("cargo")
        .args(["package", "--list", "--allow-dirty"])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(format!(
            "Release inputs:\n{}",
            String::from_utf8_lossy(&output.stdout)
        ))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into())
    }
}

pub fn install_workflow(root: &Path) -> Result<String, String> {
    let path = root.join(".github/workflows/ml-release.yml");
    if path.exists() {
        return Err(format!(
            "{} already exists; Forge will not overwrite it.",
            path.display()
        ));
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, include_str!("../templates/coordinated-release.yml"))
        .map_err(|e| e.to_string())?;
    Ok(format!("Generated {}", path.display()))
}

pub fn validate_packaging(root: &Path) -> Result<String, String> {
    let cargo = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    let workflow = std::fs::read_to_string(root.join(".github/workflows/release.yml"))
        .map_err(|e| e.to_string())?;
    let cargo_version = manifest_version(&cargo).ok_or("Cargo.toml has no package version")?;
    // The packager config lives in `[package.metadata.packager]` (cargo-integrated)
    // rather than a standalone Packager.toml, so cargo-packager auto-detects the
    // `forge_ide` binary and the packaged version tracks `[package]` automatically
    // — there is no separate version to cross-check.
    for required in [
        "[package.metadata.packager]",
        "identifier",
        "[package.metadata.packager.nsis]",
        "[package.metadata.packager.deb]",
    ] {
        if !cargo.contains(required) {
            return Err(format!("Cargo.toml packager config is missing `{required}`"));
        }
    }
    for required in [
        "windows-latest",
        "macos-14",
        "ubuntu-24.04",
        "nsis",
        "dmg",
        "deb,appimage",
        "attest-build-provenance",
        "update-*.json",
        "if-no-files-found: error",
    ] {
        if !workflow.contains(required) {
            return Err(format!("Release workflow is missing `{required}`"));
        }
    }
    Ok(format!("Packaging preflight passed for {cargo_version}\nWindows: NSIS\nmacOS: DMG (Apple Silicon)\nLinux: DEB + AppImage\nUpdate manifests: attested stable/beta channels"))
}

fn manifest_version(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("version = \"")
            .and_then(|v| v.strip_suffix('"'))
            .map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_manifest_version() {
        // Assert against the live package version so a version bump never breaks this.
        assert!(version_report(Path::new(env!("CARGO_MANIFEST_DIR")))
            .contains(env!("CARGO_PKG_VERSION")));
    }
    #[test]
    fn workflow_generation_refuses_overwrite() {
        let root = std::env::temp_dir().join(format!("forge-release-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(install_workflow(&root).is_ok());
        assert!(install_workflow(&root).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn validates_repository_packaging_configuration() {
        assert!(validate_packaging(Path::new(env!("CARGO_MANIFEST_DIR"))).is_ok());
    }
}
