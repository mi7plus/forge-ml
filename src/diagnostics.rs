use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

pub struct DiagnosticsHandle {
    request: Sender<PathBuf>,
    results: Receiver<Vec<String>>,
}

impl DiagnosticsHandle {
    pub fn spawn() -> Self {
        let (request, requests) = channel::<PathBuf>();
        let (result_tx, results) = channel();
        thread::spawn(move || {
            while let Ok(root) = requests.recv() {
                let output = Command::new("cargo")
                    .args(["check", "--message-format=short", "--quiet"])
                    .current_dir(root)
                    .output();
                let lines = match output {
                    Ok(output) => {
                        let text = String::from_utf8_lossy(&output.stderr);
                        let mut lines = text
                            .lines()
                            .filter(|line| !line.trim().is_empty())
                            .map(str::to_owned)
                            .collect::<Vec<_>>();
                        if lines.is_empty() {
                            lines.push("No Rust diagnostics.".to_owned());
                        }
                        lines
                    }
                    Err(error) => vec![format!("Could not run cargo check: {error}")],
                };
                let _ = result_tx.send(lines);
            }
        });
        Self { request, results }
    }

    pub fn check(&self, root: PathBuf) {
        let _ = self.request.send(root);
    }
    pub fn try_recv(&self) -> Option<Vec<String>> {
        self.results.try_recv().ok()
    }
}
