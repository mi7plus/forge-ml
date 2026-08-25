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
        telemetry: Vec<Telemetry>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableData {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Telemetry {
    Metric { name: String, value: f64 },
    Vector { name: String, values: Vec<f64> },
    Table { name: String, data: TableData },
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

fn runtime_loop(
    commands: Receiver<CellCommand>,
    results: Sender<CellResult>,
    process: Arc<Mutex<Option<Arc<Mutex<std::process::Child>>>>>,
) {
    let (mut context, mut streams) = match evcxr::EvalContext::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = results.send(CellResult::RuntimeError(format!("{error:?}")));
            return;
        }
    };
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
                        let telemetry = parse_telemetry(&stdout);
                        let _ = results.send(CellResult::Success {
                            cell_id,
                            output,
                            elapsed_ms,
                            variables,
                            telemetry,
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

fn parse_telemetry(output: &str) -> Vec<Telemetry> {
    output
        .lines()
        .filter_map(|line| {
            if let Some(payload) = line.trim().strip_prefix("forge_metric:") {
                let (name, value) = payload.split_once('=')?;
                return value.trim().parse().ok().map(|value| Telemetry::Metric {
                    name: name.trim().to_owned(),
                    value,
                });
            }
            if let Some(payload) = line.trim().strip_prefix("forge_vector:") {
                let (name, values) = payload.split_once('=')?;
                let values = values
                    .split(',')
                    .filter_map(|value| value.trim().parse().ok())
                    .collect::<Vec<_>>();
                return (!values.is_empty()).then(|| Telemetry::Vector {
                    name: name.trim().to_owned(),
                    values,
                });
            }
            if let Some(payload) = line.trim().strip_prefix("forge_table:") {
                let (name, json) = payload.split_once('=')?;
                let value: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
                let columns = value
                    .get("columns")?
                    .as_array()?
                    .iter()
                    .map(json_cell)
                    .collect::<Vec<_>>();
                let rows = value
                    .get("rows")?
                    .as_array()?
                    .iter()
                    .map(|row| {
                        row.as_array()
                            .map(|cells| cells.iter().map(json_cell).collect())
                    })
                    .collect::<Option<Vec<Vec<String>>>>()?;
                if columns.is_empty() || rows.iter().any(|row| row.len() != columns.len()) {
                    return None;
                }
                return Some(Telemetry::Table {
                    name: name.trim().to_owned(),
                    data: TableData { columns, rows },
                });
            }
            None
        })
        .collect()
}

fn json_cell(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_metric_and_vector_telemetry() {
        let values = parse_telemetry("forge_metric:loss=0.42\nforge_vector:w=1, 2, 3");
        assert_eq!(values.len(), 2);
        assert!(
            matches!(&values[0], Telemetry::Metric { name, value } if name == "loss" && *value == 0.42)
        );
        assert!(
            matches!(&values[1], Telemetry::Vector { name, values } if name == "w" && values == &[1.0, 2.0, 3.0])
        );
    }

    #[test]
    fn parses_rectangular_table_telemetry() {
        let values = parse_telemetry(
            r#"forge_table:samples={"columns":["feature","label"],"rows":[[1.25,"cat"],[2.5,"dog"]]}"#,
        );
        assert_eq!(values.len(), 1);
        assert!(matches!(
            &values[0],
            Telemetry::Table { name, data }
                if name == "samples"
                    && data.columns == ["feature", "label"]
                    && data.rows[1] == ["2.5", "dog"]
        ));
    }

    #[test]
    fn rejects_ragged_table_telemetry() {
        let values = parse_telemetry(r#"forge_table:bad={"columns":["x","y"],"rows":[[1],[2,3]]}"#);
        assert!(values.is_empty());
    }
}
