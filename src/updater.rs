use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Channel {
    Stable,
    Beta,
}
impl Channel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateArtifact {
    pub platform: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub schema: u32,
    pub channel: String,
    pub version: String,
    pub published_at: String,
    pub artifacts: Vec<UpdateArtifact>,
}

impl UpdateManifest {
    pub fn validate(&self, expected_channel: Channel) -> Result<&UpdateArtifact, String> {
        if self.schema != 1 {
            return Err(format!(
                "Unsupported update manifest schema {}",
                self.schema
            ));
        }
        if self.channel != expected_channel.label() {
            return Err("Update manifest channel does not match the selected channel".into());
        }
        if !valid_version(&self.version) {
            return Err("Update manifest contains an invalid version".into());
        }
        let platform = platform_key();
        let artifact = self
            .artifacts
            .iter()
            .find(|a| a.platform == platform)
            .ok_or_else(|| format!("No update is published for {platform}"))?;
        if artifact.sha256.len() != 64 || !artifact.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("Update artifact has an invalid SHA-256 digest".into());
        }
        let url = url::Url::parse(&artifact.url).map_err(|e| format!("Invalid update URL: {e}"))?;
        if url.scheme() != "https" {
            return Err("Update artifacts must use HTTPS".into());
        }
        Ok(artifact)
    }
}

pub fn check(root: &Path, repository: &str, channel: Channel) -> Result<String, String> {
    validate_repository(repository)?;
    let endpoint = format!("repos/{repository}/releases");
    let selector = if channel == Channel::Stable {
        ".[] | select(.draft == false and .prerelease == false) | .tag_name"
    } else {
        ".[] | select(.draft == false and .prerelease == true) | .tag_name"
    };
    let tags = crate::github::gh(None, &["api", &endpoint, "--jq", selector])?;
    let tag = tags
        .lines()
        .next()
        .ok_or_else(|| format!("No {} release is available", channel.label()))?;
    let directory = root.join(".forge/update-check");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let file_name = format!("update-{}.json", channel.label());
    let manifest_path = directory.join(&file_name);
    if manifest_path.exists() {
        fs::remove_file(&manifest_path).map_err(|e| e.to_string())?;
    }
    crate::github::gh(
        Some(root),
        &[
            "release",
            "download",
            tag,
            "--repo",
            repository,
            "--pattern",
            &file_name,
            "--dir",
            directory.to_string_lossy().as_ref(),
            "--clobber",
        ],
    )?;
    crate::github::gh(
        Some(root),
        &[
            "attestation",
            "verify",
            manifest_path.to_string_lossy().as_ref(),
            "--repo",
            repository,
        ],
    )?;
    let manifest: UpdateManifest =
        serde_json::from_slice(&fs::read(&manifest_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let artifact = manifest.validate(channel)?;
    Ok(format!(
        "Verified {} {}\n{}\nSHA-256: {}\nSize: {} bytes\nNo files were installed.",
        manifest.channel, manifest.version, artifact.url, artifact.sha256, artifact.size
    ))
}

fn valid_version(value: &str) -> bool {
    let value = value.strip_prefix('v').unwrap_or(value);
    !value.is_empty()
        && value.split('.').take(3).all(|p| {
            !p.is_empty()
                && p.split('-')
                    .next()
                    .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
        })
}
fn validate_repository(value: &str) -> Result<(), String> {
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    {
        Ok(())
    } else {
        Err("Repository must use owner/name form".into())
    }
}
fn platform_key() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "macos-aarch64"
    } else if cfg!(target_os = "macos") {
        "macos-x86_64"
    } else {
        "linux-x86_64"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_platform_and_https() {
        let manifest = UpdateManifest {
            schema: 1,
            channel: "stable".into(),
            version: "0.12.0".into(),
            published_at: "2026-01-01T00:00:00Z".into(),
            artifacts: vec![UpdateArtifact {
                platform: platform_key().into(),
                url: "https://example.com/forge".into(),
                sha256: "a".repeat(64),
                size: 1,
            }],
        };
        assert!(manifest.validate(Channel::Stable).is_ok());
    }
    #[test]
    fn rejects_bad_repository_and_channel() {
        assert!(validate_repository("owner/repo/path").is_err());
        let manifest = UpdateManifest {
            schema: 1,
            channel: "beta".into(),
            version: "1.0.0".into(),
            published_at: String::new(),
            artifacts: Vec::new(),
        };
        assert!(manifest.validate(Channel::Stable).is_err());
    }
}
