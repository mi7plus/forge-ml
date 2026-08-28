use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteProfile {
    pub name: String,
    pub jupyter_url: String,
    pub agent_command: String,
    pub credential_key: String,
}

pub fn store_token(profile: &RemoteProfile, token: &str) -> Result<(), String> {
    crate::database::store_secret(&profile.credential_key, token)
}

pub fn validate_profile(profile: &RemoteProfile) -> Result<(), String> {
    if profile.name.trim().is_empty() {
        return Err("Remote profile name cannot be empty.".into());
    }
    if profile.credential_key.trim().is_empty() || profile.credential_key.contains('\0') {
        return Err("Remote profile requires a valid credential key.".into());
    }
    let url = url::Url::parse(&profile.jupyter_url)
        .map_err(|error| format!("Invalid Jupyter URL: {error}"))?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err("Remote Jupyter requires HTTPS; HTTP is allowed only for localhost.".into());
    }
    if !url.username().is_empty() || url.password().is_some() || url.query().is_some() {
        return Err("Store remote credentials in the OS credential manager, not the URL.".into());
    }
    if url.fragment().is_some() {
        return Err("Jupyter URLs cannot contain fragments.".into());
    }
    Ok(())
}

pub fn test_jupyter(profile: &RemoteProfile) -> Result<String, String> {
    validate_profile(profile)?;
    let endpoint = kernelspec_endpoint(&profile.jupyter_url)?;
    let token = crate::database::load_secret(&profile.credential_key).unwrap_or_default();
    let mut child = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--max-time",
            "10",
            "--max-filesize",
            "1048576",
            "--header",
            "@-",
        ])
        .arg(endpoint.as_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start curl: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        if !token.is_empty() {
            writeln!(stdin, "Authorization: token {token}").map_err(|e| e.to_string())?;
        }
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(redact(&error, &token));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Invalid Jupyter response: {e}"))?;
    let kernels = value["kernelspecs"].as_object().ok_or_else(|| {
        "Jupyter kernelspec response did not contain a kernelspecs object.".to_owned()
    })?;
    let mut names = kernels.keys().cloned().collect::<Vec<_>>();
    names.sort();
    Ok(format!(
        "Connected to `{}`: {} kernelspec(s){}.",
        profile.name,
        names.len(),
        if names.is_empty() {
            String::new()
        } else {
            format!(" ({})", names.join(", "))
        }
    ))
}

fn kernelspec_endpoint(base: &str) -> Result<url::Url, String> {
    let mut url = url::Url::parse(base).map_err(|e| e.to_string())?;
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    url.join("api/kernelspecs").map_err(|e| e.to_string())
}

fn redact(message: &str, token: &str) -> String {
    if token.is_empty() {
        message.to_owned()
    } else {
        message.replace(token, "[REDACTED]")
    }
}

pub fn generate_actions_workflow(root: &Path) -> Result<String, String> {
    let path = root.join(".github/workflows/remote-training.yml");
    if path.exists() {
        return Err(format!("{} already exists.", path.display()));
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, include_str!("../templates/remote-training.yml"))
        .map_err(|e| e.to_string())?;
    Ok(format!("Generated {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workflow_refuses_overwrite() {
        let root = std::env::temp_dir().join(format!("forge-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(generate_actions_workflow(&root).is_ok());
        assert!(generate_actions_workflow(&root).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn validates_secure_remote_urls_and_preserves_hub_paths() {
        let profile = RemoteProfile {
            name: "lab".into(),
            jupyter_url: "https://example.test/user/forge".into(),
            agent_command: String::new(),
            credential_key: "remote:test".into(),
        };
        validate_profile(&profile).unwrap();
        assert_eq!(
            kernelspec_endpoint(&profile.jupyter_url).unwrap().as_str(),
            "https://example.test/user/forge/api/kernelspecs"
        );

        let mut insecure = profile.clone();
        insecure.jupyter_url = "http://example.test".into();
        assert!(validate_profile(&insecure).is_err());
        insecure.jupyter_url = "http://localhost:8888".into();
        validate_profile(&insecure).unwrap();
        insecure.jupyter_url = "https://example.test/?token=secret".into();
        assert!(validate_profile(&insecure).is_err());
    }

    #[test]
    fn redacts_remote_tokens_from_errors() {
        assert_eq!(
            redact("request token-secret failed", "token-secret"),
            "request [REDACTED] failed"
        );
    }
}
