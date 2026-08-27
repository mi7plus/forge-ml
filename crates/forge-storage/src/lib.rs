use forge_protocol::RunId;
use rusqlite::{params, Connection};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceRecovery {
    pub open_files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
    #[serde(default)]
    pub explorer_height: Option<f32>,
    #[serde(default)]
    pub dataset_pane_height: Option<f32>,
    #[serde(default)]
    pub dataset_viewer_docked: Option<bool>,
}

pub struct WorkspaceStore {
    forge_dir: PathBuf,
    artifacts_dir: PathBuf,
    connection: Connection,
}

impl WorkspaceStore {
    pub fn open(project_root: &Path) -> Result<Self, String> {
        let forge_dir = project_root.join(".forge");
        let artifacts_dir = forge_dir.join("artifacts");
        fs::create_dir_all(&artifacts_dir).map_err(|error| error.to_string())?;
        let connection = Connection::open(forge_dir.join("workspace.sqlite3"))
            .map_err(|error| error.to_string())?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE IF NOT EXISTS experiments (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    payload TEXT NOT NULL,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 PRAGMA user_version = 3;",
            )
            .map_err(|error| error.to_string())?;
        Ok(Self {
            forge_dir,
            artifacts_dir,
            connection,
        })
    }

    pub fn save_experiment<T: Serialize>(
        &self,
        id: &RunId,
        name: &str,
        value: &T,
    ) -> Result<(), String> {
        let payload = serde_json::to_string(value).map_err(|error| error.to_string())?;
        self.connection
            .execute(
                "INSERT INTO experiments (id, name, payload, updated_at)
                 VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    payload = excluded.payload,
                    updated_at = CURRENT_TIMESTAMP",
                params![id.as_str(), name, payload],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn delete_experiment(&self, id: &RunId) -> Result<(), String> {
        self.connection
            .execute(
                "DELETE FROM experiments WHERE id = ?1",
                params![id.as_str()],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn load_experiments<T: DeserializeOwned>(&self) -> Result<Vec<T>, String> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM experiments ORDER BY updated_at, rowid")
            .map_err(|error| error.to_string())?;
        let payloads = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        payloads
            .into_iter()
            .map(|payload| serde_json::from_str(&payload).map_err(|error| error.to_string()))
            .collect()
    }

    pub fn save_recovery(&self, recovery: &WorkspaceRecovery) -> Result<(), String> {
        atomic_json_write(&self.forge_dir.join("recovery.json"), recovery)
            .map_err(|error| error.to_string())
    }

    pub fn load_recovery(&self) -> Result<WorkspaceRecovery, String> {
        let path = self.forge_dir.join("recovery.json");
        if !path.is_file() {
            return Ok(WorkspaceRecovery::default());
        }
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())
    }

    pub fn save_connections<T: Serialize>(&self, profiles: &T) -> Result<(), String> {
        atomic_json_write(&self.forge_dir.join("connections.json"), profiles)
            .map_err(|e| e.to_string())
    }

    pub fn load_connections<T: DeserializeOwned + Default>(&self) -> Result<T, String> {
        let path = self.forge_dir.join("connections.json");
        if !path.is_file() {
            return Ok(T::default());
        }
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    pub fn save_query_history<T: Serialize>(&self, history: &T) -> Result<(), String> {
        atomic_json_write(&self.forge_dir.join("query-history.json"), history)
            .map_err(|e| e.to_string())
    }

    pub fn load_query_history<T: DeserializeOwned + Default>(&self) -> Result<T, String> {
        let path = self.forge_dir.join("query-history.json");
        if !path.is_file() {
            return Ok(T::default());
        }
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    pub fn save_remote_profiles<T: Serialize>(&self, profiles: &T) -> Result<(), String> {
        atomic_json_write(&self.forge_dir.join("remote-profiles.json"), profiles)
            .map_err(|e| e.to_string())
    }

    pub fn load_remote_profiles<T: DeserializeOwned + Default>(&self) -> Result<T, String> {
        let path = self.forge_dir.join("remote-profiles.json");
        if !path.is_file() {
            return Ok(T::default());
        }
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    pub fn save_object_profiles<T: Serialize>(&self, profiles: &T) -> Result<(), String> {
        atomic_json_write(&self.forge_dir.join("object-profiles.json"), profiles)
            .map_err(|e| e.to_string())
    }

    pub fn load_object_profiles<T: DeserializeOwned + Default>(&self) -> Result<T, String> {
        let path = self.forge_dir.join("object-profiles.json");
        if !path.is_file() {
            return Ok(T::default());
        }
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    pub fn write_artifact(&self, relative_path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err("artifact path must be a safe relative path".into());
        }
        let path = self.artifacts_dir.join(relative_path);
        atomic_write(&path, bytes).map_err(|error| error.to_string())?;
        Ok(path)
    }
}

fn atomic_json_write<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    atomic_write(path, &bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("file has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("forge")
    ));
    fs::write(&temp, bytes)?;
    if !path.exists() {
        return fs::rename(temp, path);
    }
    let backup = parent.join(format!(
        ".{}.backup",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("forge")
    ));
    if backup.exists() {
        fs::remove_file(&backup)?;
    }
    fs::rename(path, &backup)?;
    match fs::rename(&temp, path) {
        Ok(()) => {
            fs::remove_file(backup)?;
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(backup, path);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("forge-storage-{name}-{nonce}"))
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct Experiment {
        value: usize,
    }

    #[test]
    fn persists_experiments_and_recovery() {
        let root = test_root("roundtrip");
        fs::create_dir_all(&root).unwrap();
        let store = WorkspaceStore::open(&root).unwrap();
        let id = RunId::new();
        store
            .save_experiment(&id, "baseline", &Experiment { value: 42 })
            .unwrap();
        assert_eq!(
            store.load_experiments::<Experiment>().unwrap(),
            vec![Experiment { value: 42 }]
        );
        let recovery = WorkspaceRecovery {
            open_files: vec![root.join("src/main.rs")],
            active_file: Some(root.join("src/main.rs")),
            explorer_height: Some(310.0),
            dataset_pane_height: Some(260.0),
            dataset_viewer_docked: Some(true),
        };
        store.save_recovery(&recovery).unwrap();
        assert_eq!(store.load_recovery().unwrap(), recovery);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn writes_artifacts_inside_workspace() {
        let root = test_root("artifact");
        fs::create_dir_all(&root).unwrap();
        let store = WorkspaceStore::open(&root).unwrap();
        let path = store
            .write_artifact(Path::new("models/model.bin"), b"model")
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), b"model");
        assert!(store
            .write_artifact(Path::new("../outside.bin"), b"bad")
            .is_err());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
