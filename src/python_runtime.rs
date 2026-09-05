use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PythonRuntime {
    pub executable: PathBuf,
    pub version: String,
    pub packages: String,
}

pub fn discover() -> Vec<PythonRuntime> {
    let candidates: &[(&str, &[&str])] = if cfg!(windows) {
        &[("py", &["-3"]), ("python", &[])]
    } else {
        &[("python3", &[]), ("python", &[])]
    };
    let mut found = Vec::new();
    for (command, prefix) in candidates {
        let mut args = prefix.to_vec();
        args.extend([
            "-c",
            "import sys; print(sys.executable); print(sys.version.split()[0])",
        ]);
        let Ok(output) = Command::new(command).args(&args).output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut lines = text.lines();
        let Some(executable) = lines.next() else {
            continue;
        };
        let version = lines.next().unwrap_or("unknown").to_owned();
        let packages = Command::new(executable)
            .args([
                "-m",
                "pip",
                "list",
                "--format=freeze",
                "--disable-pip-version-check",
            ])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_else(|| "pip package discovery unavailable".into());
        if !found
            .iter()
            .any(|runtime: &PythonRuntime| runtime.executable.as_os_str() == OsStr::new(executable))
        {
            found.push(PythonRuntime {
                executable: PathBuf::from(executable),
                version,
                packages,
            });
        }
    }
    found
}

pub fn compatibility(runtime: &PythonRuntime) -> Vec<String> {
    let mut notes = vec![format!(
        "Python {} at {}",
        runtime.version,
        runtime.executable.display()
    )];
    for package in ["numpy", "pandas", "scikit-learn", "millwright"] {
        notes.push(
            if runtime
                .packages
                .lines()
                .any(|line| line.to_ascii_lowercase().starts_with(package))
            {
                format!("{} {package}", egui_phosphor_icons::icons::CHECK.as_str())
            } else {
                format!("- {package} not installed (user-managed)")
            },
        );
    }
    notes
}

pub fn pypi_index(runtime: &PythonRuntime, package: &str, index: &str) -> Result<String, String> {
    if package.trim().is_empty() {
        return Err("Enter a PyPI package name first.".into());
    }
    let base = url::Url::parse(index).map_err(|e| format!("Invalid Python registry URL: {e}"))?;
    if base.scheme() != "https"
        && !matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
    {
        return Err("Python registry URLs must use HTTPS unless they target localhost".into());
    }
    let script = r#"import json,sys,urllib.request
n=sys.argv[1]; base=sys.argv[2].rstrip('/')
with urllib.request.urlopen(base+'/pypi/'+n+'/json',timeout=10) as r: d=json.load(r)
i=d['info']; print(f"{i['name']} {i['version']}\n{i.get('summary','')}\nPython: {i.get('requires_python') or 'unspecified'}\nLicense: {i.get('license_expression') or i.get('license') or 'unspecified'}\nProject: {i.get('project_url') or i.get('home_page') or ''}")"#;
    let output = Command::new(&runtime.executable)
        .args(["-c", script, package.trim(), base.as_str()])
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().into())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().into())
    }
}

pub fn managers() -> Vec<String> {
    ["uv", "pip", "poetry", "conda"]
        .into_iter()
        .filter_map(|name| {
            let result = if name == "pip" {
                Command::new("python")
                    .args(["-m", "pip", "--version"])
                    .output()
            } else {
                Command::new(name).arg("--version").output()
            };
            result
                .ok()
                .filter(|output| output.status.success())
                .map(|output| format!("{name}: {}", String::from_utf8_lossy(&output.stdout).trim()))
        })
        .collect()
}

pub fn create_venv(runtime: &PythonRuntime, path: &std::path::Path) -> Result<String, String> {
    let output = Command::new(&runtime.executable)
        .args(["-m", "venv"])
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(format!("Created Python environment at {}", path.display()))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_user_managed_packages() {
        let runtime = PythonRuntime {
            executable: "python".into(),
            version: "3.12".into(),
            packages: "numpy==2.0".into(),
        };
        let notes = compatibility(&runtime);
        assert!(notes
            .iter()
            .any(|line| line.ends_with(" numpy") && !line.contains("not installed")));
        assert!(notes
            .iter()
            .any(|line| line.contains("scikit-learn not installed")));
    }
    #[test]
    fn rejects_insecure_python_registry() {
        let runtime = PythonRuntime {
            executable: "python".into(),
            version: String::new(),
            packages: String::new(),
        };
        assert!(pypi_index(&runtime, "demo", "http://packages.example.com").is_err());
    }
}
