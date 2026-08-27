use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct ModelVersion {
    pub model: String,
    pub version: String,
    pub format: String,
    pub artifact: String,
    pub created_at_unix: u64,
    pub tags: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct RegistryIndex {
    versions: Vec<ModelVersion>,
    aliases: BTreeMap<String, String>,
}

pub struct ModelRegistry {
    root: PathBuf,
}
impl ModelRegistry {
    pub fn open(project: &Path) -> Result<Self, String> {
        let root = project.join(".forge/models");
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        Ok(Self { root })
    }
    pub fn register(
        &self,
        model: &str,
        version: &str,
        format: &str,
        source: &Path,
        tags: Vec<String>,
    ) -> Result<ModelVersion, String> {
        validate(model)?;
        validate(version)?;
        let extension = source.extension().and_then(|v| v.to_str()).unwrap_or("bin");
        let relative = format!("{model}/{version}/model.{extension}");
        let destination = self.root.join(&relative);
        fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::copy(source, &destination).map_err(|e| e.to_string())?;
        let item = ModelVersion {
            model: model.into(),
            version: version.into(),
            format: format.into(),
            artifact: relative,
            created_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            tags,
        };
        let mut index = self.load()?;
        index
            .versions
            .retain(|v| v.model != model || v.version != version);
        index.versions.push(item.clone());
        self.save(&index)?;
        Ok(item)
    }
    pub fn versions(&self, model: &str) -> Result<Vec<ModelVersion>, String> {
        Ok(self
            .load()?
            .versions
            .into_iter()
            .filter(|v| v.model == model)
            .collect())
    }
    pub fn promote(&self, model: &str, alias: &str, version: &str) -> Result<(), String> {
        validate(alias)?;
        let mut index = self.load()?;
        if !index
            .versions
            .iter()
            .any(|v| v.model == model && v.version == version)
        {
            return Err("Model version is not registered".into());
        }
        index
            .aliases
            .insert(format!("{model}:{alias}"), version.into());
        self.save(&index)
    }
    pub fn resolve(&self, model: &str, version_or_alias: &str) -> Result<PathBuf, String> {
        let index = self.load()?;
        let version = index
            .aliases
            .get(&format!("{model}:{version_or_alias}"))
            .map(String::as_str)
            .unwrap_or(version_or_alias);
        index
            .versions
            .iter()
            .find(|v| v.model == model && v.version == version)
            .map(|v| self.root.join(&v.artifact))
            .ok_or_else(|| "Model version or alias was not found".into())
    }
    fn load(&self) -> Result<RegistryIndex, String> {
        let path = self.root.join("registry.json");
        if !path.exists() {
            return Ok(RegistryIndex::default());
        }
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }
    fn save(&self, index: &RegistryIndex) -> Result<(), String> {
        let path = self.root.join("registry.json");
        let temporary = self.root.join("registry.json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(index).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(temporary, path).map_err(|e| e.to_string())
    }
}
fn validate(value: &str) -> Result<(), String> {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        Ok(())
    } else {
        Err("Names and versions may contain letters, numbers, dots, dashes, and underscores".into())
    }
}

pub fn generate_inference_service(
    root: &Path,
    crate_name: &str,
    model_path: &Path,
) -> Result<PathBuf, String> {
    validate(crate_name)?;
    let destination = root.join(format!("{crate_name}-service"));
    if destination.exists() {
        return Err(format!("{} already exists", destination.display()));
    }
    fs::create_dir_all(destination.join("src")).map_err(|e| e.to_string())?;
    let cargo = format!("[package]\nname = \"{crate_name}-service\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\naxum = \"0.8\"\ntokio = {{ version = \"1\", features = [\"rt-multi-thread\", \"macros\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\n");
    let main = format!("use axum::{{routing::post, Json, Router}};\nuse serde::{{Deserialize, Serialize}};\nconst MODEL_PATH: &str = {:?};\n#[derive(Deserialize)] struct Request {{ values: Vec<f32> }}\n#[derive(Serialize)] struct Response {{ values: Vec<f32>, model: &'static str }}\nasync fn predict(Json(input): Json<Request>) -> Json<Response> {{ Json(Response {{ values: input.values, model: MODEL_PATH }}) }}\n#[tokio::main] async fn main() {{ let app = Router::new().route(\"/predict\", post(predict)); let listener = tokio::net::TcpListener::bind(\"0.0.0.0:3000\").await.unwrap(); axum::serve(listener, app).await.unwrap(); }}\n", model_path.display().to_string());
    fs::write(destination.join("Cargo.toml"), cargo).map_err(|e| e.to_string())?;
    fs::write(destination.join("src/main.rs"), main).map_err(|e| e.to_string())?;
    fs::write(destination.join("Dockerfile"), "FROM rust:1-slim AS build\nWORKDIR /app\nCOPY . .\nRUN cargo build --release\nFROM debian:bookworm-slim\nCOPY --from=build /app/target/release/*-service /usr/local/bin/model-service\nEXPOSE 3000\nCMD [\"model-service\"]\n").map_err(|e| e.to_string())?;
    fs::write(destination.join("compose.yaml"), "services:\n  model:\n    build: .\n    ports: [\"3000:3000\"]\n    restart: unless-stopped\n").map_err(|e| e.to_string())?;
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_supports_alias_rollback() {
        let root = std::env::temp_dir().join(format!("forge-registry-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let artifact = root.join("model.onnx");
        fs::write(&artifact, b"model").unwrap();
        let registry = ModelRegistry::open(&root).unwrap();
        registry
            .register("iris", "1.0.0", "onnx", &artifact, vec!["stable".into()])
            .unwrap();
        registry.promote("iris", "production", "1.0.0").unwrap();
        assert!(registry.resolve("iris", "production").unwrap().is_file());
        let _ = fs::remove_dir_all(root);
    }
}
