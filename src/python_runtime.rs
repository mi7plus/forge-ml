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
                format!("✓ {package}")
            } else {
                format!("— {package} not installed (user-managed)")
            },
        );
    }
    notes
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
        assert!(notes.iter().any(|line| line == "✓ numpy"));
        assert!(notes
            .iter()
            .any(|line| line.contains("scikit-learn not installed")));
    }
}
