use forge_protocol::{parse_stdout_events, EventEnvelope};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CellCommand {
    Execute { cell_id: usize, code: String },
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CellResult {
    Ready,
    Success {
        cell_id: usize,
        output: String,
        elapsed_ms: u128,
        variables: Vec<VariableMeta>,
        events: Vec<EventEnvelope>,
    },
    Error {
        cell_id: usize,
        message: String,
        elapsed_ms: u128,
    },
    Reset,
    RuntimeError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableMeta {
    pub name: String,
    pub type_name: String,
}

pub struct RuntimeHandle {
    commands: Sender<CellCommand>,
    results: Receiver<CellResult>,
    process: Arc<Mutex<Option<Arc<Mutex<std::process::Child>>>>>,
}

impl RuntimeHandle {
    pub fn spawn() -> Self {
        let (commands, command_rx) = channel();
        let (result_tx, results) = channel();
        let process = Arc::new(Mutex::new(None));
        let runtime_process = Arc::clone(&process);
        thread::spawn(move || runtime_loop(command_rx, result_tx, runtime_process));
        Self {
            commands,
            results,
            process,
        }
    }

    pub fn execute(&self, cell_id: usize, code: String) -> Result<(), String> {
        self.commands
            .send(CellCommand::Execute { cell_id, code })
            .map_err(|e| e.to_string())
    }

    pub fn reset(&self) -> Result<(), String> {
        self.commands
            .send(CellCommand::Reset)
            .map_err(|e| e.to_string())
    }

    pub fn stop(&self) -> Result<(), String> {
        let process = self
            .process
            .lock()
            .map_err(|error| error.to_string())?
            .clone();
        if let Some(process) = process {
            let mut child = process.lock().map_err(|error| error.to_string())?;
            let _ = child.kill();
        }
        self.reset()
    }

    pub fn try_recv(&self) -> Option<CellResult> {
        self.results.try_recv().ok()
    }
}

/// Apply Forge's evcxr defaults to a freshly created context. When an offline
/// runtime bundle is active we turn on offline mode (so cargo never reaches for
/// the network) and grant a large compilation cache so the pre-built Millwright
/// and Burn artifacts are reused instead of recompiled on first use.
fn configure_context(context: &mut evcxr::EvalContext) {
    if crate::offline::detect().is_some() {
        let mut state = context.state();
        state.set_offline_mode(true);
        // 8 GiB is ample for the blessed dependency set and keeps prebuilt
        // Millwright/Burn artifacts resident across resets instead of recompiling.
        state.set_cache_bytes(8 * 1024 * 1024 * 1024);
        let _ = context.eval_with_state("", state);
    }
}

fn runtime_loop(
    commands: Receiver<CellCommand>,
    results: Sender<CellResult>,
    process: Arc<Mutex<Option<Arc<Mutex<std::process::Child>>>>>,
) {
    // If this build ships an offline runtime bundle, point cargo/rustc (and thus
    // evcxr) at it before starting the kernel, so notebook `:dep` cells for
    // Millwright/Burn resolve and build with no network or system toolchain.
    crate::offline::activate();

    let (mut context, mut streams) = match evcxr::EvalContext::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = results.send(CellResult::RuntimeError(format!("{error:?}")));
            return;
        }
    };
    configure_context(&mut context);
    if let Ok(mut slot) = process.lock() {
        *slot = Some(context.process_handle());
    }
    let _ = results.send(CellResult::Ready);

    while let Ok(command) = commands.recv() {
        match command {
            CellCommand::Execute { cell_id, code } => {
                let started = Instant::now();
                let evaluation = context.eval(&code);
                thread::sleep(Duration::from_millis(5));
                let stdout = streams.stdout.try_iter().collect::<Vec<_>>().join("\n");
                let stderr = streams.stderr.try_iter().collect::<Vec<_>>().join("\n");
                let elapsed_ms = started.elapsed().as_millis();

                match evaluation {
                    Ok(value) => {
                        let expression = value.get("text/plain").unwrap_or_default();
                        let output = [stdout.as_str(), stderr.as_str(), expression]
                            .into_iter()
                            .filter(|part| !part.trim().is_empty())
                            .collect::<Vec<_>>()
                            .join("\n");
                        let variables = context
                            .variables_and_types()
                            .map(|(name, type_name)| VariableMeta {
                                name: name.to_owned(),
                                type_name: type_name.to_owned(),
                            })
                            .collect();
                        let events = parse_stdout_events(&stdout);
                        let _ = results.send(CellResult::Success {
                            cell_id,
                            output,
                            elapsed_ms,
                            variables,
                            events,
                        });
                    }
                    Err(error) => {
                        let message = if stderr.is_empty() {
                            format!("{error:?}")
                        } else {
                            format!("{stderr}\n{error:?}")
                        };
                        let _ = results.send(CellResult::Error {
                            cell_id,
                            message,
                            elapsed_ms,
                        });
                    }
                }
            }
            CellCommand::Reset => match evcxr::EvalContext::new() {
                Ok((new_context, new_streams)) => {
                    context = new_context;
                    streams = new_streams;
                    configure_context(&mut context);
                    if let Ok(mut slot) = process.lock() {
                        *slot = Some(context.process_handle());
                    }
                    let _ = results.send(CellResult::Reset);
                }
                Err(error) => {
                    let _ = results.send(CellResult::RuntimeError(format!("{error:?}")));
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_uses_protocol_legacy_adapter() {
        use forge_protocol::ForgeEvent;

        let values = parse_stdout_events("forge_metric:loss=0.42\nforge_vector:w=1, 2, 3");
        assert_eq!(values.len(), 2);
        assert!(
            matches!(&values[0].event, ForgeEvent::Metric { name, value } if name == "loss" && *value == 0.42)
        );
        assert!(
            matches!(&values[1].event, ForgeEvent::Vector { name, values } if name == "w" && values == &[1.0, 2.0, 3.0])
        );
    }
}
