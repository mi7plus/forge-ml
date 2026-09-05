//! Execution orchestration: enqueueing and running notebook cells, the
//! interactive Rust and Python consoles, live-variable inspection, Python-
//! runtime discovery, and background Cargo diagnostics. Methods on the shared
//! [`crate::ForgeApp`], split out of `main.rs`.
use crate::*;

impl crate::ForgeApp {
    pub(crate) fn enqueue_cells(&mut self, ids: impl IntoIterator<Item = usize>) {
        if matches!(self.run_state, RunState::Running(_) | RunState::Booting) {
            return;
        }
        self.run_queue.clear();
        for id in ids {
            self.run_queue.push_back(id);
            self.cell_records.entry(id).or_default().state = Some(CellState::Queued);
        }
        self.run_next();
    }

    pub(crate) fn run_next(&mut self) {
        let Some(cell_id) = self.run_queue.pop_front() else {
            return;
        };
        let Some((_, code)) = self.cells().get(cell_id).cloned() else {
            return;
        };
        if code.trim().is_empty() {
            self.cell_records.entry(cell_id).or_default().state = Some(CellState::Passed);
            self.run_next();
            return;
        }
        if self.remote_notebook_execution {
            let Some(session) = self.remote_kernel_session.clone() else {
                self.run_queue.clear();
                self.run_state = RunState::Failed;
                self.cell_records.entry(cell_id).or_default().state = Some(CellState::Failed);
                self.console = "Start a remote kernel or disable remote notebook execution.".into();
                return;
            };
            let (input_tx, input_rx) = mpsc::channel();
            match self
                .integration_worker
                .submit(IntegrationRequest::RemoteExecute {
                    session,
                    code,
                    cell_id: Some(cell_id),
                    input: input_rx,
                }) {
                Ok(()) => {
                    self.remote_input_sender = Some(input_tx);
                    self.integration_pending += 1;
                    self.remote_execution_pending = true;
                    self.run_state = RunState::Running(cell_id);
                    let provenance = self.project_root().map(|root| git::provenance(&root));
                    let record = self.cell_records.entry(cell_id).or_default();
                    record.state = Some(CellState::Running);
                    record.output.clear();
                    record.rich_outputs.clear();
                    record.plots.clear();
                    record.elapsed_ms = None;
                    if let Some((commit, dirty)) = provenance {
                        record.git_commit = Some(commit);
                        record.git_dirty = dirty;
                    }
                    self.console = format!("Running cell {} on remote kernel…", cell_id + 1);
                }
                Err(error) => {
                    self.run_queue.clear();
                    self.run_state = RunState::Failed;
                    self.cell_records.entry(cell_id).or_default().state = Some(CellState::Failed);
                    self.console = error;
                }
            }
            return;
        }
        let code = prepare_runtime_code(&code, self.active().path.as_deref());
        if self.runtime.execute(cell_id, code).is_ok() {
            self.run_state = RunState::Running(cell_id);
            let provenance = self.project_root().map(|root| git::provenance(&root));
            let record = self.cell_records.entry(cell_id).or_default();
            record.state = Some(CellState::Running);
            record.output.clear();
            record.rich_outputs.clear();
            record.plots.clear();
            if let Some((commit, dirty)) = provenance {
                record.git_commit = Some(commit);
                record.git_dirty = dirty;
            }
            self.console = format!("Compiling cell {}...", cell_id + 1);
        }
    }

    pub(crate) fn run_console_input(&mut self) {
        let code = self.console_input.trim().to_owned();
        if code.is_empty() || matches!(self.run_state, RunState::Running(_) | RunState::Booting) {
            return;
        }
        if self.runtime.execute(CONSOLE_CELL_ID, code.clone()).is_ok() {
            self.history.push(code);
            self.console_input.clear();
            self.run_state = RunState::Running(CONSOLE_CELL_ID);
            self.console = "Evaluating console input...".to_owned();
        }
    }

    /// Surface a live variable's value: emit it into the Data viewer / Plots when
    /// it is tabular/numeric, else pretty-print it to the console. Runs a hidden
    /// snippet in the same Evcxr session.
    pub(crate) fn inspect_variable(&mut self, name: &str, type_name: &str) {
        if matches!(self.run_state, RunState::Running(_) | RunState::Booting) {
            self.console = "Runtime is busy — try again once the current cell finishes.".into();
            return;
        }
        let (code, dataset_key) = inspect_code(name, type_name);
        if self.runtime.execute(CONSOLE_CELL_ID, code).is_ok() {
            self.run_state = RunState::Running(CONSOLE_CELL_ID);
            self.console = format!("Inspecting `{name}`…");
            if let Some(key) = dataset_key {
                // The dataset arrives when the snippet finishes; pre-select it and
                // switch to the Data viewer so it shows up in place.
                self.open_dataset = Some(key);
                self.inspector_tab = InspectorTab::Data;
            }
        }
    }

    pub(crate) fn discover_python_runtimes(&mut self) {
        self.python_runtimes = python_runtime::discover();
        if self.selected_python.is_none() {
            self.selected_python = self
                .python_runtimes
                .first()
                .map(|runtime| runtime.executable.clone());
        }
        self.python_runtime_output = if self.python_runtimes.is_empty() {
            "No Python runtime found.".into()
        } else {
            self.python_runtimes
                .iter()
                .flat_map(python_runtime::compatibility)
                .collect::<Vec<_>>()
                .join("\n")
        };
    }

    pub(crate) fn start_python_kernel(&mut self) {
        let Some(executable) = self.selected_python.clone() else {
            self.python_console_output = "Discover and select a Python runtime first.".into();
            return;
        };
        if let Some(runtime) = self
            .python_runtimes
            .iter()
            .find(|runtime| runtime.executable == executable)
        {
            self.python_environment_fingerprint =
                experiment::stable_digest(runtime.packages.as_bytes());
        }
        match python_kernel::PythonKernel::spawn(&executable) {
            Ok(kernel) => {
                self.python_kernel = Some(kernel);
                self.python_console_output = format!(
                    "Python runtime ready: {}\nEnvironment: {}",
                    executable.display(),
                    self.python_environment_fingerprint
                );
            }
            Err(error) => self.python_console_output = format!("Could not start Python: {error}"),
        }
    }

    pub(crate) fn run_python_input(&mut self) {
        if self.python_kernel.is_none() {
            self.start_python_kernel();
        }
        let code = self.python_console_input.trim().to_owned();
        if code.is_empty() {
            return;
        }
        self.python_execution_id += 1;
        if let Some(kernel) = &self.python_kernel {
            if kernel.execute(self.python_execution_id, code).is_ok() {
                self.python_console_input.clear();
            }
        }
        if self.last_resource_poll.elapsed() >= Duration::from_secs(1) {
            self.resource_snapshot = deep_learning::resources(&mut self.resource_system);
            self.last_resource_poll = Instant::now();
        }
    }

    pub(crate) fn run_diagnostics(&mut self) {
        self.inspector_tab = InspectorTab::Problems;
        if let Some(project) = &self.project {
            self.diagnostics
                .check(project.root.clone(), diagnostics::Tool::Check);
            self.diagnostics_running = true;
            self.diagnostic_lines = vec![format!(
                "Checking {} with cargo check...",
                project.root.display()
            )];
            self.console = "Cargo check is running. Results will appear in Problems.".to_owned();
        } else {
            self.diagnostics_running = false;
            self.diagnostic_lines = vec![
                "No Cargo project is open. Use File > Open project, then click Check.".to_owned(),
            ];
            self.console = "Check needs an open Cargo project.".to_owned();
        }
    }
}
