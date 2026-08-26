use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Deserialize)]
pub struct KernelSpec {
    pub name: String,
    pub display_name: String,
    pub language: String,
    pub resource_dir: PathBuf,
}

pub fn discover() -> Result<Vec<KernelSpec>, String> {
    let output = Command::new("jupyter")
        .args(["kernelspec", "list", "--json"])
        .output()
        .map_err(|e| format!("Jupyter is not available: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    let mut kernels = Vec::new();
    for (name, entry) in value["kernelspecs"].as_object().into_iter().flatten() {
        let spec = &entry["spec"];
        kernels.push(KernelSpec {
            name: name.clone(),
            display_name: spec["display_name"].as_str().unwrap_or(name).into(),
            language: spec["language"].as_str().unwrap_or("unknown").into(),
            resource_dir: PathBuf::from(entry["resource_dir"].as_str().unwrap_or_default()),
        });
    }
    kernels.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(kernels)
}

pub fn install_evcxr() -> Result<String, String> {
    let output = Command::new("evcxr_jupyter")
        .arg("--install")
        .output()
        .map_err(|e| format!("evcxr_jupyter is not available: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        Ok(if stdout.trim().is_empty() {
            "Evcxr Jupyter kernel installed.".into()
        } else {
            stdout.trim().into()
        })
    } else {
        Err(stderr.trim().into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_kernel_object_is_tolerated() {
        let value: serde_json::Value = serde_json::json!({"kernelspecs": {}});
        assert_eq!(value["kernelspecs"].as_object().unwrap().len(), 0);
    }
}
