use serde::{Deserialize, Serialize};
use std::path::Path;

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
}
