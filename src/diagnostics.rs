use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

/// Which cargo analysis to run for the Problems pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Check,
    Clippy,
}

impl Tool {
    fn args(self) -> &'static [&'static str] {
        match self {
            Tool::Check => &["check", "--message-format=short", "--quiet"],
            Tool::Clippy => &["clippy", "--message-format=short", "--quiet"],
        }
    }
}

pub struct DiagnosticsHandle {
    request: Sender<(PathBuf, Tool)>,
    results: Receiver<Vec<String>>,
}

impl DiagnosticsHandle {
    pub fn spawn() -> Self {
        let (request, requests) = channel::<(PathBuf, Tool)>();
        let (result_tx, results) = channel();
        thread::spawn(move || {
            while let Ok((root, tool)) = requests.recv() {
                let output = Command::new("cargo")
                    .args(tool.args())
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

    pub fn check(&self, root: PathBuf, tool: Tool) {
        let _ = self.request.send((root, tool));
    }
    pub fn try_recv(&self) -> Option<Vec<String>> {
        self.results.try_recv().ok()
    }
}
