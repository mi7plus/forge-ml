use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelVersion {
    pub model: String,
    pub version: String,
    pub format: String,
    pub artifact: String,
    pub created_at_unix: u64,
    pub tags: Vec<String>,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size_bytes: u64,
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
        let (sha256, size_bytes) = artifact_identity(source)?;
        let mut index = self.load()?;
        if let Some(existing) = index
            .versions
            .iter()
            .find(|item| item.model == model && item.version == version)
        {
            let existing_path = self.root.join(&existing.artifact);
            let (existing_sha256, existing_size) =
                artifact_identity(&existing_path).map_err(|_| {
                    "Registered model metadata exists but its artifact is missing or unreadable"
                        .to_owned()
                })?;
            if existing_sha256 == sha256 && existing_size == size_bytes {
                return Ok(existing.clone());
            }
            return Err(format!(
                "Model {model} version {version} is immutable and already contains different bytes"
            ));
        }
        let extension = source.extension().and_then(|v| v.to_str()).unwrap_or("bin");
        let relative = format!("{model}/{version}/model.{extension}");
        let destination = self.root.join(&relative);
        fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
        if destination.exists() {
            return Err(
                "Artifact destination already exists without matching registry metadata".into(),
            );
        }
        let temporary = destination.with_extension(format!("{extension}.tmp"));
        fs::copy(source, &temporary).map_err(|e| e.to_string())?;
        fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
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
            sha256,
            size_bytes,
        };
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
        let version = self.resolve_version(model, version_or_alias)?;
        let path = self.root.join(&version.artifact);
        if !version.sha256.is_empty() {
            let (actual_sha256, actual_size) = artifact_identity(&path)?;
            if actual_sha256 != version.sha256 || actual_size != version.size_bytes {
                return Err(format!(
                    "Integrity check failed for {} {}: registered artifact was modified",
                    version.model, version.version
                ));
            }
        }
        Ok(path)
    }
    pub fn resolve_version(
        &self,
        model: &str,
        version_or_alias: &str,
    ) -> Result<ModelVersion, String> {
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
            .cloned()
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

fn artifact_identity(path: &Path) -> Result<(String, u64), String> {
    use std::io::Read;
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}

pub fn generate_inference_service(
    root: &Path,
    crate_name: &str,
    version: &ModelVersion,
    model_path: &Path,
) -> Result<PathBuf, String> {
    validate(crate_name)?;
    let destination = root.join(format!("{crate_name}-service"));
    if destination.exists() {
        return Err(format!("{} already exists", destination.display()));
    }
    fs::create_dir_all(destination.join("src")).map_err(|e| e.to_string())?;
    fs::create_dir_all(destination.join("models")).map_err(|e| e.to_string())?;
    let extension = model_path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("bin");
    let bundled_model = destination
        .join("models")
        .join(format!("model.{extension}"));
    fs::copy(model_path, &bundled_model).map_err(|e| e.to_string())?;
    let (service_sha256, service_size_bytes) = artifact_identity(&bundled_model)?;
    let onnx = version.format.eq_ignore_ascii_case("onnx");
    let cargo = if onnx {
        format!("[package]\nname = \"{crate_name}-service\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\naxum = \"0.8\"\ntokio = {{ version = \"1\", features = [\"rt-multi-thread\", \"macros\", \"net\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nsha2 = \"0.10\"\nmillwright = {{ version = \"2.2.1\", default-features = false, features = [\"serve\"] }}\n")
    } else {
        format!("[package]\nname = \"{crate_name}-service\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\naxum = \"0.8\"\ntokio = {{ version = \"1\", features = [\"rt-multi-thread\", \"macros\", \"net\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\nsha2 = \"0.10\"\n")
    };
    let main = if onnx {
        format!(
            r#"use axum::{{routing::get, Json, Router}};
use millwright::prelude::Server;
use serde::Serialize;
use sha2::{{Digest, Sha256}};
use std::io::Read;
use std::time::Duration;
const MODEL_PATH: &str = "models/model.{extension}";
const MODEL: &str = {model:?};
const VERSION: &str = {version_number:?};
const SHA256: &str = {sha256:?};
const SIZE_BYTES: u64 = {size_bytes};
#[derive(Serialize)] struct Metadata {{ model: &'static str, version: &'static str, format: &'static str, artifact: &'static str, runtime: &'static str, sha256: &'static str, size_bytes: u64 }}
fn integrity_ok() -> bool {{
    let Ok(mut file) = std::fs::File::open(MODEL_PATH) else {{ return false }};
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {{
        let Ok(read) = file.read(&mut buffer) else {{ return false }};
        if read == 0 {{ break }}
        hasher.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
    }}
    size == SIZE_BYTES && format!("{{:x}}", hasher.finalize()) == SHA256
}}
async fn health() -> &'static str {{ "ok" }}
async fn ready() -> Result<&'static str, (axum::http::StatusCode, &'static str)> {{
    integrity_ok().then_some("ready").ok_or((axum::http::StatusCode::SERVICE_UNAVAILABLE, "model integrity check failed"))
}}
async fn metadata() -> Json<Metadata> {{ Json(Metadata {{ model: MODEL, version: VERSION, format: "onnx", artifact: MODEL_PATH, runtime: "millwright-2.2.1", sha256: SHA256, size_bytes: SIZE_BYTES }}) }}
#[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {{
    if std::env::args().any(|arg| arg == "--healthcheck") {{
        std::process::exit(if integrity_ok() {{ 0 }} else {{ 1 }});
    }}
    let inference = Server::from_onnx(MODEL_PATH)?
        .request_limits(10_000, 10_000, 8 * 1024 * 1024)
        .max_concurrency(64)
        .inference_timeout(Duration::from_secs(30))
        .router();
    let operations = Router::new().route("/health", get(health)).route("/ready", get(ready)).route("/metadata", get(metadata));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, inference.merge(operations)).await?;
    Ok(())
}}
"#,
            model = version.model,
            version_number = version.version,
            sha256 = service_sha256,
            size_bytes = service_size_bytes,
        )
    } else {
        format!(
            r#"use axum::{{routing::{{get, post}}, Json, Router}};
use serde::{{Deserialize, Serialize}};
use sha2::{{Digest, Sha256}};
use std::io::Read;
use std::sync::atomic::{{AtomicU64, Ordering}};
const MODEL_PATH: &str = "models/model.{extension}";
const MODEL: &str = {model:?};
const VERSION: &str = {version_number:?};
const FORMAT: &str = {format:?};
const SHA256: &str = {sha256:?};
const SIZE_BYTES: u64 = {size_bytes};
static REQUESTS: AtomicU64 = AtomicU64::new(0);
#[derive(Deserialize)] struct Request {{ values: Vec<f32> }}
#[derive(Serialize)] struct Response {{ values: Vec<f32>, model: &'static str, version: &'static str }}
#[derive(Serialize)] struct Metadata {{ model: &'static str, version: &'static str, format: &'static str, artifact: &'static str, requests: u64, sha256: &'static str, size_bytes: u64 }}
fn integrity_ok() -> bool {{
    let Ok(mut file) = std::fs::File::open(MODEL_PATH) else {{ return false }};
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {{
        let Ok(read) = file.read(&mut buffer) else {{ return false }};
        if read == 0 {{ break }}
        hasher.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
    }}
    size == SIZE_BYTES && format!("{{:x}}", hasher.finalize()) == SHA256
}}
async fn health() -> &'static str {{ "ok" }}
async fn ready() -> Result<&'static str, (axum::http::StatusCode, &'static str)> {{
    integrity_ok().then_some("ready").ok_or((axum::http::StatusCode::SERVICE_UNAVAILABLE, "model integrity check failed"))
}}
async fn metadata() -> Json<Metadata> {{ Json(Metadata {{ model: MODEL, version: VERSION, format: FORMAT, artifact: MODEL_PATH, requests: REQUESTS.load(Ordering::Relaxed), sha256: SHA256, size_bytes: SIZE_BYTES }}) }}
async fn predict(Json(input): Json<Request>) -> Json<Response> {{
    let requests = REQUESTS.fetch_add(1, Ordering::Relaxed) + 1;
    println!("forge_service:{{}}", serde_json::json!({{"model": MODEL, "version": VERSION, "requests": requests, "errors": 0}}));
    Json(Response {{ values: input.values, model: MODEL, version: VERSION }})
}}
#[tokio::main] async fn main() {{
    if std::env::args().any(|arg| arg == "--healthcheck") {{
        std::process::exit(if integrity_ok() {{ 0 }} else {{ 1 }});
    }}
    let app = Router::new().route("/health", get(health)).route("/ready", get(ready)).route("/metadata", get(metadata)).route("/predict", post(predict));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}}
"#,
            model = version.model,
            version_number = version.version,
            format = version.format,
            sha256 = service_sha256,
            size_bytes = service_size_bytes,
        )
    };
    fs::write(destination.join("Cargo.toml"), cargo).map_err(|e| e.to_string())?;
    fs::write(destination.join("src/main.rs"), main).map_err(|e| e.to_string())?;
    fs::write(destination.join("Dockerfile"), "FROM rust:1-slim AS build\nWORKDIR /app\nCOPY . .\nRUN cargo build --release\nFROM debian:bookworm-slim\nWORKDIR /app\nCOPY --from=build /app/target/release/*-service /usr/local/bin/model-service\nCOPY --from=build /app/models ./models\nEXPOSE 3000\nHEALTHCHECK CMD [\"/usr/local/bin/model-service\", \"--healthcheck\"]\nCMD [\"model-service\"]\n").map_err(|e| e.to_string())?;
    fs::write(destination.join("compose.yaml"), "services:\n  model:\n    build: .\n    ports: [\"3000:3000\"]\n    restart: unless-stopped\n    healthcheck:\n      test: [\"CMD\", \"/usr/local/bin/model-service\", \"--healthcheck\"]\n      interval: 10s\n      timeout: 3s\n      retries: 3\n").map_err(|e| e.to_string())?;
    let prediction_note = if onnx {
        "`POST /predict` accepts `{\"rows\":[[...], ...]}` and performs bounded, timeout-protected inference through the published Millwright 2.2.1 ONNX runtime."
    } else {
        "The generated `POST /predict` handler is integration scaffolding; replace its pass-through body with the tensor adapter for the selected model format before production use."
    };
    fs::write(destination.join("README.md"), format!("# {crate_name} inference service\n\nGenerated from registered model `{}` version `{}` (`{}`). The artifact is copied into `models/` and will not change if a registry alias is later promoted or rolled back.\n\nEndpoints: `GET /health`, `GET /ready`, `GET /metadata`, and `POST /predict`. {prediction_note}\n", version.model, version.version, version.format)).map_err(|e| e.to_string())?;
    fs::write(destination.join("deployment.yaml"), format!("apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: {crate_name}\nspec:\n  replicas: 1\n  selector:\n    matchLabels: {{ app: {crate_name} }}\n  template:\n    metadata:\n      labels: {{ app: {crate_name} }}\n    spec:\n      containers:\n        - name: model\n          image: {crate_name}-service:0.1.0\n          ports: [{{ containerPort: 3000 }}]\n          readinessProbe:\n            httpGet: {{ path: /ready, port: 3000 }}\n          livenessProbe:\n            httpGet: {{ path: /health, port: 3000 }}\n---\napiVersion: v1\nkind: Service\nmetadata:\n  name: {crate_name}\nspec:\n  selector: {{ app: {crate_name} }}\n  ports: [{{ port: 80, targetPort: 3000 }}]\n")).map_err(|e| e.to_string())?;
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
        assert_eq!(
            registry
                .resolve_version("iris", "production")
                .unwrap()
                .version,
            "1.0.0"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_versions_are_idempotent_but_immutable() {
        let root = std::env::temp_dir().join(format!("forge-immutable-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.onnx");
        let same = root.join("same.onnx");
        let changed = root.join("changed.onnx");
        fs::write(&first, b"one").unwrap();
        fs::write(&same, b"one").unwrap();
        fs::write(&changed, b"two").unwrap();
        let registry = ModelRegistry::open(&root).unwrap();
        let registered = registry
            .register("iris", "1", "onnx", &first, vec![])
            .unwrap();
        let repeated = registry
            .register("iris", "1", "onnx", &same, vec![])
            .unwrap();
        assert_eq!(registered.sha256, repeated.sha256);
        assert!(registry
            .register("iris", "1", "onnx", &changed, vec![])
            .unwrap_err()
            .contains("immutable"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_rejects_modified_registered_artifacts() {
        let root = std::env::temp_dir().join(format!("forge-tamper-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.onnx");
        fs::write(&source, b"trusted").unwrap();
        let registry = ModelRegistry::open(&root).unwrap();
        registry
            .register("iris", "1", "onnx", &source, vec![])
            .unwrap();
        let registered = registry.resolve("iris", "1").unwrap();
        fs::write(&registered, b"tampered").unwrap();
        assert!(registry
            .resolve("iris", "1")
            .unwrap_err()
            .contains("Integrity check failed"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generated_service_bundles_immutable_model_and_operational_endpoints() {
        let root = std::env::temp_dir().join(format!("forge-service-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let artifact = root.join("source.onnx");
        fs::write(&artifact, b"portable model").unwrap();
        let (sha256, size_bytes) = artifact_identity(&artifact).unwrap();
        let version = ModelVersion {
            model: "iris".into(),
            version: "1.2.0".into(),
            format: "onnx".into(),
            artifact: "unused".into(),
            created_at_unix: 0,
            tags: vec![],
            sha256,
            size_bytes,
        };
        let service = generate_inference_service(&root, "iris", &version, &artifact).unwrap();
        assert_eq!(
            fs::read(service.join("models/model.onnx")).unwrap(),
            b"portable model"
        );
        let source = fs::read_to_string(service.join("src/main.rs")).unwrap();
        assert!(source.contains("/health"));
        assert!(source.contains("/ready"));
        assert!(source.contains("Server::from_onnx(MODEL_PATH)"));
        assert!(source.contains("request_limits(10_000, 10_000, 8 * 1024 * 1024)"));
        let cargo = fs::read_to_string(service.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("millwright = { version = \"2.2.1\""));
        assert!(service.join("deployment.yaml").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn non_onnx_service_remains_an_explicit_adapter() {
        let root = std::env::temp_dir().join(format!("forge-adapter-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let artifact = root.join("source.bin");
        fs::write(&artifact, b"model").unwrap();
        let (sha256, size_bytes) = artifact_identity(&artifact).unwrap();
        let version = ModelVersion {
            model: "custom".into(),
            version: "1".into(),
            format: "safetensors".into(),
            artifact: "unused".into(),
            created_at_unix: 0,
            tags: vec![],
            sha256,
            size_bytes,
        };
        let service = generate_inference_service(&root, "custom", &version, &artifact).unwrap();
        let source = fs::read_to_string(service.join("src/main.rs")).unwrap();
        assert!(source.contains("forge_service:"));
        assert!(!source.contains("Server::from_onnx"));
        let readme = fs::read_to_string(service.join("README.md")).unwrap();
        assert!(readme.contains("integration scaffolding"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "compiles a standalone generated Cargo project"]
    fn generated_onnx_service_compiles() {
        let root = std::env::temp_dir().join(format!("forge-service-check-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let artifact = root.join("source.onnx");
        fs::write(&artifact, b"compile-only placeholder").unwrap();
        let (sha256, size_bytes) = artifact_identity(&artifact).unwrap();
        let version = ModelVersion {
            model: "verified".into(),
            version: "1".into(),
            format: "onnx".into(),
            artifact: "unused".into(),
            created_at_unix: 0,
            tags: vec![],
            sha256,
            size_bytes,
        };
        let service = generate_inference_service(&root, "verified", &version, &artifact).unwrap();
        let status = std::process::Command::new("cargo")
            .args(["check", "--quiet"])
            .current_dir(&service)
            .env(
                "CARGO_TARGET_DIR",
                Path::new(env!("CARGO_MANIFEST_DIR")).join("target/generated-service-check"),
            )
            .status()
            .unwrap();
        let _ = fs::remove_dir_all(root);
        assert!(status.success());
    }
}
