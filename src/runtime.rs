use forge_protocol::{parse_stdout_events, EventEnvelope};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
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
    /// Incremental output (e.g. cargo "Compiling …" lines) emitted while a cell
    /// is still running, so a long `:dep` build shows visible progress instead of
    /// a frozen spinner.
    Progress {
        cell_id: usize,
        line: String,
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

/// Apply Forge's evcxr defaults to a freshly created context.
///
/// - A large persistent compilation cache so a heavy `:dep` (Millwright, Burn) is
///   compiled from source only once, then reused across cells and sessions.
/// - Offline builds. The crates a notebook `:dep`s (Millwright, Burn, and their
///   whole dependency trees) are already cached from building the app, so offline
///   resolves them fine — and, crucially, an offline build never needs cargo's
///   *exclusive* package-cache lock to refresh the registry index. That is what
///   lets a notebook build run concurrently with rust-analyzer (which also runs
///   cargo); sharing the exclusive lock is what otherwise deadlocked them (two
///   idle cargo processes). A brand-new crate that isn't already cached will fail
///   to resolve offline — disable rust-analyzer for that one-off, or pre-fetch it.
fn configure_context(context: &mut evcxr::CommandContext) {
    // `:` commands are handled by CommandContext's command layer.
    let _ = context.execute(":cache 8192");
    let _ = context.execute(":offline 1");
}

/// Create a fresh evcxr context (evcxr re-execs the current binary as its
/// isolated runtime child).
fn new_context() -> Result<(evcxr::CommandContext, evcxr::EvalContextOutputs), evcxr::Error> {
    evcxr::CommandContext::new()
}

/// Forward the runtime child's stderr (cargo "Compiling …" progress and any
/// runtime warnings) to the UI live, tagged with whichever cell is currently
/// executing, so a long `:dep` build shows progress instead of a frozen spinner.
/// The thread exits when the channel closes (context dropped / reset).
fn spawn_stderr_pump<I>(stderr: I, results: Sender<CellResult>, current_cell: Arc<AtomicUsize>)
where
    I: IntoIterator<Item = String> + Send + 'static,
    I::IntoIter: Send,
{
    thread::spawn(move || {
        for line in stderr {
            let cell_id = current_cell.load(Ordering::Relaxed);
            if cell_id != NO_CELL {
                let _ = results.send(CellResult::Progress { cell_id, line });
            }
        }
    });
}

/// Sentinel for "no cell is currently running" so startup/reset build noise is
/// not attributed to a real cell.
const NO_CELL: usize = usize::MAX;

/// Continuously drain the runtime child's stdout into a shared buffer. This MUST
/// run concurrently with `execute()`: evcxr's `execute` blocks until the child's
/// stdout is consumed, so collecting stdout only *after* execute (as we used to)
/// deadlocked every cell that produced output — the notebook `:dep` hang.
fn spawn_stdout_pump<I>(stdout: I, buffer: Arc<Mutex<Vec<String>>>)
where
    I: IntoIterator<Item = String> + Send + 'static,
    I::IntoIter: Send,
{
    thread::spawn(move || {
        for line in stdout {
            if let Ok(mut buf) = buffer.lock() {
                buf.push(line);
            }
        }
    });
}

fn runtime_loop(
    commands: Receiver<CellCommand>,
    results: Sender<CellResult>,
    process: Arc<Mutex<Option<Arc<Mutex<std::process::Child>>>>>,
) {
    // If Forge itself was started by `cargo run`, that parent cargo exported its
    // build *jobserver* into our environment (CARGO_MAKEFLAGS). Any cargo that
    // evcxr spawns to build a `:dep` cell would inherit it and block forever
    // waiting for a job token the parent never releases — a nested-cargo
    // deadlock (two idle cargo processes, no rustc). Drop it so evcxr's cargo
    // manages its own jobserver.
    std::env::remove_var("CARGO_MAKEFLAGS");
    std::env::remove_var("MAKEFLAGS");

    // If this build ships an offline runtime bundle, point cargo/rustc (and thus
    // evcxr) at it before starting the kernel, so notebook `:dep` cells for
    // Millwright/Burn resolve and build with no network or system toolchain.
    crate::environment::activate_runtime();

    // CommandContext (not the lower-level EvalContext) is required so notebook
    // `:` commands — notably `:dep` — are honored; EvalContext would compile a
    // `:dep` line as Rust and never link the crate.
    let current_cell = Arc::new(AtomicUsize::new(NO_CELL));
    let (mut context, outputs) = match new_context() {
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
    // Drain BOTH streams concurrently: stdout into a buffer (collected per cell),
    // stderr streamed live. Draining stdout during execute is mandatory — see
    // spawn_stdout_pump.
    let stdout_buf = Arc::new(Mutex::new(Vec::<String>::new()));
    spawn_stdout_pump(outputs.stdout, Arc::clone(&stdout_buf));
    spawn_stderr_pump(outputs.stderr, results.clone(), Arc::clone(&current_cell));
    let _ = results.send(CellResult::Ready);

    while let Ok(command) = commands.recv() {
        match command {
            CellCommand::Execute { cell_id, code } => {
                let started = Instant::now();
                current_cell.store(cell_id, Ordering::Relaxed);
                // Start collecting this cell's stdout fresh.
                if let Ok(mut buf) = stdout_buf.lock() {
                    buf.clear();
                }
                let evaluation = context.execute(&code);
                current_cell.store(NO_CELL, Ordering::Relaxed);
                thread::sleep(Duration::from_millis(5));
                let stdout = stdout_buf
                    .lock()
                    .map(|buf| buf.join("\n"))
                    .unwrap_or_default();
                let elapsed_ms = started.elapsed().as_millis();

                match evaluation {
                    Ok(value) => {
                        let expression = value.get("text/plain").unwrap_or_default();
                        let output = [stdout.as_str(), expression]
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
                        // Compile/runtime stderr was already streamed live via
                        // Progress; the structured error carries the diagnostics.
                        let message = format!("{error:?}");
                        let _ = results.send(CellResult::Error {
                            cell_id,
                            message,
                            elapsed_ms,
                        });
                    }
                }
            }
            CellCommand::Reset => match new_context() {
                Ok((new_ctx, new_outputs)) => {
                    context = new_ctx;
                    // The old pumps end when the old context's channels close;
                    // start fresh ones on the same buffer for the new context.
                    if let Ok(mut buf) = stdout_buf.lock() {
                        buf.clear();
                    }
                    spawn_stdout_pump(new_outputs.stdout, Arc::clone(&stdout_buf));
                    spawn_stderr_pump(
                        new_outputs.stderr,
                        results.clone(),
                        Arc::clone(&current_cell),
                    );
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
