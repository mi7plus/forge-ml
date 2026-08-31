//! A named, portable workspace snapshot: the project root, open files, dock
//! layout, theme, key bindings, appearance, and database connection profiles.
//! Connection profiles carry only a keychain *reference* (`credential_key`),
//! never a secret, so a workspace file is safe to share.

use crate::database::ConnectionProfile;
use crate::keymap::ChordDto;
use crate::ui::theme::NamedTheme;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const WORKSPACE_SCHEMA: u32 = 1;

fn default_font_size() -> f32 {
    14.0
}

fn default_true() -> bool {
    true
}

/// Everything that defines a saved workspace/session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub schema: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub project_root: Option<PathBuf>,
    #[serde(default)]
    pub open_files: Vec<PathBuf>,
    #[serde(default)]
    pub active_file: Option<PathBuf>,
    /// Serialized `egui_tiles` dock layout (JSON), as stored in the session.
    #[serde(default)]
    pub dock_layout: Option<String>,
    #[serde(default)]
    pub dark_mode: bool,
    #[serde(default)]
    pub high_contrast: bool,
    #[serde(default)]
    pub reduced_motion: bool,
    #[serde(default = "default_font_size")]
    pub editor_font_size: f32,
    #[serde(default = "default_true")]
    pub caret_blink: bool,
    #[serde(default)]
    pub active_theme: Option<String>,
    #[serde(default)]
    pub custom_themes: Vec<NamedTheme>,
    #[serde(default)]
    pub keymap: Vec<ChordDto>,
    /// Database connection profiles (keychain references only — no secrets).
    #[serde(default)]
    pub connections: Vec<ConnectionProfile>,
}

impl Default for WorkspaceSnapshot {
    fn default() -> Self {
        Self {
            schema: WORKSPACE_SCHEMA,
            name: String::new(),
            project_root: None,
            open_files: Vec::new(),
            active_file: None,
            dock_layout: None,
            dark_mode: false,
            high_contrast: false,
            reduced_motion: false,
            editor_font_size: default_font_size(),
            caret_blink: true,
            active_theme: None,
            custom_themes: Vec::new(),
            keymap: Vec::new(),
            connections: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::ConnectionKind;

    #[test]
    fn round_trips_and_defaults_missing_fields() {
        let snap = WorkspaceSnapshot {
            name: "research".into(),
            project_root: Some(PathBuf::from("/home/user/proj")),
            open_files: vec![PathBuf::from("/home/user/proj/src/main.rs")],
            active_file: Some(PathBuf::from("/home/user/proj/src/main.rs")),
            dark_mode: true,
            editor_font_size: 16.0,
            connections: vec![ConnectionProfile {
                name: "warehouse".into(),
                kind: ConnectionKind::PostgreSql,
                location: "host=localhost dbname=app".into(),
                username: "analyst".into(),
                credential_key: "forge/warehouse".into(),
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: WorkspaceSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "research");
        assert_eq!(back.project_root, snap.project_root);
        assert_eq!(back.open_files, snap.open_files);
        assert!(back.dark_mode);
        assert_eq!(back.editor_font_size, 16.0);
        assert_eq!(back.connections.len(), 1);
        assert_eq!(back.connections[0].credential_key, "forge/warehouse");

        // A minimal file (only schema) fills every other field with a default.
        let minimal: WorkspaceSnapshot = serde_json::from_str(r#"{"schema":1}"#).unwrap();
        assert!(minimal.open_files.is_empty());
        assert_eq!(minimal.editor_font_size, 14.0);
        assert!(minimal.caret_blink);
    }

    #[test]
    fn connection_profiles_never_embed_secrets() {
        // The profile type has no password/secret field — only a keychain key.
        let json = serde_json::to_string(&ConnectionProfile {
            name: "db".into(),
            kind: ConnectionKind::SQLite,
            location: "data.db".into(),
            username: String::new(),
            credential_key: "forge/db".into(),
        })
        .unwrap();
        assert!(!json.to_lowercase().contains("password"));
        assert!(json.contains("credential_key"));
    }
}
