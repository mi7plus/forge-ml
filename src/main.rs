mod data;
mod diagnostics;
mod experiment;
mod git;
mod github;
mod jobs;
mod jupyter;
mod lsp;
mod millwright_studio;
mod notebook;
mod packages;
mod plot;
mod project;
mod publishing;
mod python_kernel;
mod python_runtime;
mod runtime;
mod session;

use data::DataWorkspace;
use diagnostics::DiagnosticsHandle;
use eframe::egui;
use egui::{Color32, Frame, Margin, Panel, RichText, Stroke};
use egui_code_editor::{CodeEditor, ColorTheme, Syntax};
use egui_plot::{Line, Plot, PlotPoints};
use experiment::{capture_provenance, ExperimentRun};
#[cfg(test)]
use forge_protocol::RunId;
use forge_protocol::TableData;
use forge_storage::{WorkspaceRecovery, WorkspaceStore};
use jobs::{JobQueue, JobState};
use lsp::{Diagnostic as LspDiagnostic, LspCommand, LspEvent, LspHandle};
use millwright_studio::{
    ChannelObserver, EvaluationReport, LeaderboardEntry, PipelineDesign, PipelineStep,
    TrainingEvent, TrainingObserver,
};
use notebook::{
    cell_byte_ranges, is_notebook_document, lsp_document, notebook_lsp_prefix_chars,
    prepare_runtime_code, CellKind, NotebookDocument, RichOutput,
};
use plot::{metric_line, vector_bars};
use project::{FileNode, Project};
use runtime::{CellResult, RuntimeHandle, VariableMeta};
use session::SessionState;
use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const TEXT: Color32 = Color32::PLACEHOLDER;
const MUTED: Color32 = Color32::PLACEHOLDER;
const EMBER: Color32 = Color32::from_rgb(196, 119, 44);
const CYAN: Color32 = Color32::from_rgb(39, 141, 204);
const GREEN: Color32 = Color32::from_rgb(46, 157, 96);
const RED: Color32 = Color32::from_rgb(212, 72, 85);
const STORAGE_KEY: &str = "forge_ml_session_v1";
const CONSOLE_CELL_ID: usize = usize::MAX;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn default_explorer_height() -> f32 {
    280.0
}

fn default_editor_font_size() -> f32 {
    14.0
}

#[cfg(test)]
fn default_dataset_pane_height() -> f32 {
    280.0
}

fn default_true() -> bool {
    true
}

fn main() -> eframe::Result<()> {
    // Evcxr relaunches the current executable as its isolated evaluation runtime.
    // This hook turns that child into a headless runtime before eframe can open a window.
    evcxr::runtime_hook();
    let app_name = format!("Forge ML {APP_VERSION}");
    eframe::run_native(
        &app_name,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title(format!("Forge ML {APP_VERSION} - Rust compute studio"))
                .with_inner_size([1380.0, 860.0])
                .with_min_inner_size([980.0, 640.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(ForgeApp::new(cc)))),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum InspectorTab {
    Variables,
    Data,
    Charts,
    Experiments,
    Search,
    Help,
    Problems,
    Git,
    Packages,
    GitHub,
    Studio,
}

#[derive(Clone, Copy, PartialEq)]
enum LeftTab {
    Project,
    Outline,
}

#[derive(Clone, Copy, PartialEq)]
enum ConsoleTab {
    Console,
    History,
    Python,
}

#[derive(Clone, Copy, PartialEq)]
enum RunState {
    Booting,
    Ready,
    Running(usize),
    Failed,
}

#[derive(Clone, Copy, PartialEq)]
enum CellState {
    Idle,
    Queued,
    Running,
    Passed,
    Failed,
}

#[derive(Clone, Copy)]
enum EditorHistoryCommand {
    Undo,
    Redo,
}

#[derive(Default)]
struct CellRecord {
    state: Option<CellState>,
    output: String,
    elapsed_ms: Option<u128>,
    git_commit: Option<String>,
    git_dirty: bool,
    rich_outputs: Vec<RichOutput>,
}

struct ProjectSearchResult {
    path: PathBuf,
    line: usize,
    column: usize,
    preview: String,
}

struct EditorTab {
    path: Option<PathBuf>,
    title: String,
    content: String,
    dirty: bool,
    disk_hash: Option<u64>,
    external_change_pending: bool,
}

enum ExplorerAction {
    Open(PathBuf),
    NewFile(PathBuf),
    Delete(PathBuf),
}

#[derive(Clone)]
enum PendingUnsavedAction {
    CloseTab(usize),
    OpenProject(Option<PathBuf>),
}

struct ForgeApp {
    tabs: Vec<EditorTab>,
    active_tab: usize,
    project: Option<Project>,
    workspace_store: Option<WorkspaceStore>,
    selected_cell: usize,
    run_state: RunState,
    run_queue: VecDeque<usize>,
    cell_records: HashMap<usize, CellRecord>,
    console: String,
    runtime: RuntimeHandle,
    runtime_restart_attempts: usize,
    variables: Vec<VariableMeta>,
    data: DataWorkspace,
    open_dataset: Option<String>,
    dataset_filter: String,
    dataset_sort_column: Option<usize>,
    dataset_sort_descending: bool,
    dataset_viewer_docked: bool,
    dataset_pane_height: f32,
    inspector_tab: InspectorTab,
    diagnostics: DiagnosticsHandle,
    diagnostic_lines: Vec<String>,
    diagnostics_running: bool,
    execution_count: usize,
    left_tab: LeftTab,
    console_tab: ConsoleTab,
    console_input: String,
    history: Vec<String>,
    lsp: LspHandle,
    lsp_status: String,
    lsp_diagnostics: HashMap<PathBuf, Vec<LspDiagnostic>>,
    completions: Vec<String>,
    hover_text: String,
    cursor_offset: usize,
    document_version: i32,
    last_lsp_hash: u64,
    hover_probe_offset: Option<usize>,
    navigable_hover_offset: Option<usize>,
    definition_probe_pending: bool,
    dark_mode: bool,
    editor_needs_initial_focus: bool,
    explorer_height: f32,
    pending_delete: Option<PathBuf>,
    pending_unsaved_action: Option<PendingUnsavedAction>,
    find_visible: bool,
    find_query: String,
    replace_query: String,
    pending_editor_selection: Option<(usize, usize)>,
    run_all_after_reset: bool,
    experiment_name: String,
    experiment_tags: String,
    experiment_notes: String,
    experiment_github_issue: String,
    experiment_github_pr: String,
    experiment_github_action: String,
    saved_runs: Vec<ExperimentRun>,
    comparison_metric: String,
    project_search_query: String,
    project_search_case_sensitive: bool,
    project_search_results: Vec<ProjectSearchResult>,
    recent_projects: Vec<PathBuf>,
    settings_open: bool,
    editor_font_size: f32,
    caret_blink: bool,
    completion_popup_open: bool,
    git_output: String,
    git_commit_message: String,
    git_branch_name: String,
    package_query: String,
    package_output: String,
    github_input: String,
    github_output: String,
    jupyter_output: String,
    pipeline_design: PipelineDesign,
    training_events: Vec<TrainingEvent>,
    training_observer: ChannelObserver,
    evaluation_report: EvaluationReport,
    leaderboard: Vec<LeaderboardEntry>,
    python_runtime_output: String,
    python_runtimes: Vec<python_runtime::PythonRuntime>,
    selected_python: Option<PathBuf>,
    python_kernel: Option<python_kernel::PythonKernel>,
    python_console_input: String,
    python_console_output: String,
    python_execution_id: usize,
    python_mime_outputs: Vec<RichOutput>,
    selected_jupyter_kernel: String,
    jupyter_kernels: Vec<jupyter::KernelSpec>,
    python_environment_fingerprint: String,
    job_queue: JobQueue,
    job_command: String,
    last_file_poll: Instant,
    pending_editor_history: Option<EditorHistoryCommand>,
}

impl ForgeApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor_icons::add_fonts(&mut fonts);
        cc.egui_ctx.set_fonts(fonts);
        let session = cc
            .storage
            .and_then(|storage| eframe::get_value::<SessionState>(storage, STORAGE_KEY))
            .unwrap_or_default();
        configure_style(&cc.egui_ctx, session.dark_mode);
        let dark_mode = session.dark_mode;
        let explorer_height = if session.explorer_height > 0.0 {
            session.explorer_height
        } else {
            default_explorer_height()
        };
        let mut recent_projects = session.recent_projects.clone();
        let has_saved_editor_settings = session.editor_font_size > 0.0;
        let editor_font_size = if has_saved_editor_settings {
            session.editor_font_size
        } else {
            default_editor_font_size()
        };
        let caret_blink = if has_saved_editor_settings {
            session.caret_blink
        } else {
            default_true()
        };
        let project = session
            .project_root
            .clone()
            .and_then(|root| Project::open(root).ok());
        let workspace_store = project
            .as_ref()
            .and_then(|project| WorkspaceStore::open(&project.root).ok());
        let recovery = workspace_store
            .as_ref()
            .and_then(|store| store.load_recovery().ok())
            .unwrap_or_default();
        let explorer_height = recovery.explorer_height.unwrap_or(explorer_height);
        let dataset_pane_height = recovery
            .dataset_pane_height
            .unwrap_or(session.dataset_pane_height);
        let dataset_viewer_docked = recovery
            .dataset_viewer_docked
            .unwrap_or(session.dataset_viewer_docked);
        let open_files = if recovery.open_files.is_empty() {
            session.open_files.clone()
        } else {
            recovery.open_files
        };
        let active_file = recovery.active_file.or_else(|| session.active_file.clone());
        let persisted_runs = workspace_store
            .as_ref()
            .and_then(|store| store.load_experiments::<ExperimentRun>().ok())
            .unwrap_or_default();
        let saved_runs = if persisted_runs.is_empty() {
            session.saved_runs.clone()
        } else {
            persisted_runs
        };
        if let Some(root) = project.as_ref().map(|project| project.root.clone()) {
            recent_projects.retain(|path| path != &root);
            recent_projects.insert(0, root);
            recent_projects.truncate(10);
        }
        let mut tabs = open_files
            .into_iter()
            .filter_map(|path| {
                std::fs::read_to_string(&path)
                    .ok()
                    .map(|content| EditorTab {
                        title: file_title(&path),
                        path: Some(path),
                        disk_hash: Some(content_hash(&content)),
                        content,
                        dirty: false,
                        external_change_pending: false,
                    })
            })
            .collect::<Vec<_>>();
        if tabs.is_empty() {
            tabs.push(welcome_tab());
        }
        let active_tab = active_file
            .and_then(|active| {
                tabs.iter()
                    .position(|tab| tab.path.as_ref() == Some(&active))
            })
            .unwrap_or(0);
        Self {
            tabs,
            active_tab,
            project,
            workspace_store,
            selected_cell: 0,
            run_state: RunState::Booting,
            run_queue: VecDeque::new(),
            cell_records: HashMap::new(),
            console: "Starting isolated Rust runtime...".to_owned(),
            runtime: RuntimeHandle::spawn(),
            runtime_restart_attempts: 0,
            variables: Vec::new(),
            data: DataWorkspace::default(),
            open_dataset: None,
            dataset_filter: String::new(),
            dataset_sort_column: None,
            dataset_sort_descending: false,
            dataset_viewer_docked,
            dataset_pane_height,
            inspector_tab: InspectorTab::Variables,
            diagnostics: DiagnosticsHandle::spawn(),
            diagnostic_lines: vec!["Run diagnostics to check the current Cargo project.".to_owned()],
            diagnostics_running: false,
            execution_count: 0,
            left_tab: LeftTab::Project,
            console_tab: ConsoleTab::Console,
            console_input: String::new(),
            history: Vec::new(),
            lsp: LspHandle::spawn(),
            lsp_status: "rust-analyzer waiting for a Rust file.".to_owned(),
            lsp_diagnostics: HashMap::new(),
            completions: Vec::new(),
            hover_text: String::new(),
            cursor_offset: 0,
            document_version: 1,
            last_lsp_hash: 0,
            hover_probe_offset: None,
            navigable_hover_offset: None,
            definition_probe_pending: false,
            dark_mode,
            editor_needs_initial_focus: true,
            explorer_height,
            pending_delete: None,
            pending_unsaved_action: None,
            find_visible: false,
            find_query: String::new(),
            replace_query: String::new(),
            pending_editor_selection: None,
            run_all_after_reset: false,
            experiment_name: session.experiment_name,
            experiment_tags: String::new(),
            experiment_notes: String::new(),
            experiment_github_issue: String::new(),
            experiment_github_pr: String::new(),
            experiment_github_action: String::new(),
            saved_runs,
            comparison_metric: session.comparison_metric,
            project_search_query: String::new(),
            project_search_case_sensitive: false,
            project_search_results: Vec::new(),
            recent_projects,
            settings_open: false,
            editor_font_size,
            caret_blink,
            completion_popup_open: false,
            git_output: String::new(),
            git_commit_message: String::new(),
            git_branch_name: String::new(),
            package_query: String::new(),
            package_output: String::new(),
            github_input: String::new(),
            github_output: String::new(),
            jupyter_output: String::new(),
            pipeline_design: PipelineDesign {
                name: "pipeline".into(),
                target: "target".into(),
                steps: Vec::new(),
            },
            training_events: Vec::new(),
            training_observer: ChannelObserver::default(),
            evaluation_report: EvaluationReport::default(),
            leaderboard: Vec::new(),
            python_runtime_output: String::new(),
            python_runtimes: Vec::new(),
            selected_python: session.selected_python.clone(),
            python_kernel: None,
            python_console_input: String::new(),
            python_console_output: String::new(),
            python_execution_id: 0,
            python_mime_outputs: Vec::new(),
            selected_jupyter_kernel: session.selected_jupyter_kernel.clone(),
            jupyter_kernels: Vec::new(),
            python_environment_fingerprint: session.python_environment_fingerprint.clone(),
            job_queue: JobQueue::new(),
            job_command: "cargo run --release".into(),
            last_file_poll: Instant::now(),
            pending_editor_history: None,
        }
    }

    fn active(&self) -> &EditorTab {
        &self.tabs[self.active_tab]
    }
    fn active_mut(&mut self) -> &mut EditorTab {
        &mut self.tabs[self.active_tab]
    }

    fn cells(&self) -> Vec<(String, String)> {
        if !is_notebook_document(&self.active().content) {
            return if self.active().content.trim().is_empty() {
                Vec::new()
            } else {
                vec![(self.active().title.clone(), self.active().content.clone())]
            };
        }
        NotebookDocument::parse_rust(&self.active().content)
            .cells
            .into_iter()
            .enumerate()
            .map(|(index, cell)| match cell.kind {
                CellKind::Code => (format!("Cell {}", index + 1), cell.source),
                CellKind::Markdown => (format!("Markdown {}", index + 1), String::new()),
            })
            .collect()
    }

    fn select_cell_from_caret(&mut self) {
        let ranges = cell_byte_ranges(&self.active().content);
        if ranges.is_empty() {
            self.selected_cell = 0;
            return;
        }
        let cursor_byte = char_to_byte(&self.active().content, self.cursor_offset);
        self.selected_cell = ranges
            .iter()
            .position(|range| cursor_byte >= range.start && cursor_byte < range.end)
            .unwrap_or(ranges.len() - 1);
    }

    fn insert_cell_after(&mut self) {
        let ranges = cell_byte_ranges(&self.active().content);
        let insertion = ranges
            .get(self.selected_cell)
            .map(|range| range.end)
            .unwrap_or_else(|| self.active().content.len());
        let block = if insertion == 0 {
            "//# %% new cell\n"
        } else {
            "\n\n//# %% new cell\n"
        };
        self.active_mut().content.insert_str(insertion, block);
        self.active_mut().dirty = true;
        self.selected_cell = self.selected_cell.saturating_add(1);
        let offset = self.active().content[..insertion + block.len()]
            .chars()
            .count();
        self.pending_editor_selection = Some((offset, offset));
        self.cell_records.clear();
    }

    fn delete_selected_cell(&mut self) {
        let ranges = cell_byte_ranges(&self.active().content);
        let Some(range) = ranges.get(self.selected_cell).cloned() else {
            return;
        };
        if ranges.len() == 1 {
            self.active_mut().content = "//# %% new cell\n".to_owned();
            self.selected_cell = 0;
        } else {
            self.active_mut().content.replace_range(range, "");
            self.selected_cell = self.selected_cell.min(ranges.len() - 2);
        }
        self.active_mut().dirty = true;
        self.cell_records.clear();
        self.pending_editor_selection = Some((0, 0));
    }

    fn move_selected_cell(&mut self, direction: isize) {
        let ranges = cell_byte_ranges(&self.active().content);
        if ranges.len() < 2 {
            return;
        }
        let target = self.selected_cell as isize + direction;
        if target < 0 || target >= ranges.len() as isize {
            return;
        }
        let mut chunks = ranges
            .iter()
            .map(|range| self.active().content[range.clone()].to_owned())
            .collect::<Vec<_>>();
        chunks.swap(self.selected_cell, target as usize);
        self.active_mut().content = chunks.concat();
        self.active_mut().dirty = true;
        self.selected_cell = target as usize;
        self.cell_records.clear();
    }

    fn restart_and_run_all(&mut self) {
        self.run_queue.clear();
        self.run_all_after_reset = true;
        self.run_state = RunState::Booting;
        self.console = "Restarting runtime before running all cells...".to_owned();
        let _ = self.runtime.reset();
    }

    fn stop_execution(&mut self) {
        self.run_queue.clear();
        self.run_all_after_reset = false;
        self.run_state = RunState::Booting;
        self.console = "Stopping execution and restarting the Rust runtime...".to_owned();
        if let Err(error) = self.runtime.stop() {
            self.run_state = RunState::Failed;
            self.console = format!("Could not stop execution: {error}");
        }
    }

    fn open_project(&mut self) {
        if self.tabs.iter().any(|tab| tab.dirty) {
            self.pending_unsaved_action = Some(PendingUnsavedAction::OpenProject(None));
            return;
        }
        self.open_project_dialog();
    }

    fn open_project_dialog(&mut self) {
        let Some(root) = rfd::FileDialog::new()
            .set_title("Open Forge ML project")
            .pick_folder()
        else {
            return;
        };
        self.open_project_path(root);
    }

    fn request_open_project_path(&mut self, root: PathBuf) {
        if self.tabs.iter().any(|tab| tab.dirty) {
            self.pending_unsaved_action = Some(PendingUnsavedAction::OpenProject(Some(root)));
        } else {
            self.open_project_path(root);
        }
    }

    fn open_project_path(&mut self, root: PathBuf) {
        match Project::open(root.clone()) {
            Ok(project) => {
                self.console = format!("Opened {}", project.root.display());
                self.project = Some(project);
                self.workspace_store = WorkspaceStore::open(&root).ok();
                if let Some(store) = &self.workspace_store {
                    if let Ok(runs) = store.load_experiments::<ExperimentRun>() {
                        self.saved_runs = runs;
                    }
                    if let Ok(recovery) = store.load_recovery() {
                        if let Some(height) = recovery.explorer_height {
                            self.explorer_height = height;
                        }
                        if let Some(height) = recovery.dataset_pane_height {
                            self.dataset_pane_height = height;
                        }
                        if let Some(docked) = recovery.dataset_viewer_docked {
                            self.dataset_viewer_docked = docked;
                        }
                        let mut restored_tabs = recovery
                            .open_files
                            .into_iter()
                            .filter_map(|path| {
                                std::fs::read_to_string(&path)
                                    .ok()
                                    .map(|content| EditorTab {
                                        title: file_title(&path),
                                        path: Some(path),
                                        disk_hash: Some(content_hash(&content)),
                                        content,
                                        dirty: false,
                                        external_change_pending: false,
                                    })
                            })
                            .collect::<Vec<_>>();
                        if !restored_tabs.is_empty() {
                            self.active_tab = recovery
                                .active_file
                                .and_then(|active| {
                                    restored_tabs
                                        .iter()
                                        .position(|tab| tab.path.as_ref() == Some(&active))
                                })
                                .unwrap_or(0);
                            self.tabs.clear();
                            self.tabs.append(&mut restored_tabs);
                        }
                    }
                }
                self.recent_projects.retain(|path| path != &root);
                self.recent_projects.insert(0, root);
                self.recent_projects.truncate(10);
                self.last_lsp_hash = 0;
            }
            Err(error) => self.console = format!("Could not open project: {error}"),
        }
    }

    fn create_new_file(&mut self, directory: Option<PathBuf>) {
        let Some(project_root) = self.project.as_ref().map(|project| project.root.clone()) else {
            self.console = "Open a project before creating a file.".to_owned();
            return;
        };
        let directory = directory.unwrap_or_else(|| project_root.clone());
        let Some(path) = rfd::FileDialog::new()
            .set_title("Create file in Forge ML project")
            .set_directory(directory)
            .save_file()
        else {
            return;
        };
        let Some(parent) = path.parent() else {
            self.console = "Choose a valid file location.".to_owned();
            return;
        };
        let inside_project = project_root
            .canonicalize()
            .ok()
            .zip(parent.canonicalize().ok())
            .is_some_and(|(root, parent)| parent.starts_with(root));
        if !inside_project {
            self.console = "New files must be created inside the open project.".to_owned();
            return;
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => {
                if let Some(project) = &mut self.project {
                    let _ = project.refresh();
                }
                self.console = format!("Created {}", path.display());
                self.open_file(path);
                self.editor_needs_initial_focus = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.console = format!(
                    "{} already exists; no file was overwritten.",
                    path.display()
                );
            }
            Err(error) => {
                self.console = format!("Could not create {}: {error}", path.display());
            }
        }
    }

    fn delete_file(&mut self, path: PathBuf) {
        let Some(project_root) = self.project.as_ref().map(|project| project.root.clone()) else {
            return;
        };
        if self
            .tabs
            .iter()
            .any(|tab| tab.path.as_ref() == Some(&path) && tab.dirty)
        {
            self.console = format!(
                "Save or discard changes in {} before deleting it.",
                path.display()
            );
            return;
        }
        let inside_project = project_root
            .canonicalize()
            .ok()
            .zip(path.canonicalize().ok())
            .is_some_and(|(root, target)| target.starts_with(root) && target.is_file());
        if !inside_project {
            self.console = "Forge only deletes files inside the open project.".to_owned();
            return;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.tabs.retain(|tab| tab.path.as_ref() != Some(&path));
                if self.tabs.is_empty() {
                    self.tabs.push(blank_tab());
                }
                self.active_tab = self.active_tab.min(self.tabs.len() - 1);
                self.lsp_diagnostics.remove(&path);
                if let Some(project) = &mut self.project {
                    let _ = project.refresh();
                }
                self.console = format!("Deleted {}. This action cannot be undone.", path.display());
            }
            Err(error) => self.console = format!("Could not delete {}: {error}", path.display()),
        }
    }

    fn delete_confirmation(&mut self, ctx: &egui::Context) {
        let Some(path) = self.pending_delete.clone() else {
            return;
        };
        egui::Window::new("Delete file?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Delete {}?", path.display()));
                ui.label(RichText::new("This action cannot be undone.").color(RED));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.pending_delete = None;
                    }
                    if ui.button(RichText::new("Delete file").color(RED)).clicked() {
                        self.pending_delete = None;
                        self.delete_file(path.clone());
                    }
                });
            });
    }

    fn open_file(&mut self, path: PathBuf) {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.path.as_ref() == Some(&path))
        {
            self.active_tab = index;
            self.selected_cell = 0;
            return;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                self.tabs.push(EditorTab {
                    title: file_title(&path),
                    path: Some(path),
                    disk_hash: Some(content_hash(&content)),
                    content,
                    dirty: false,
                    external_change_pending: false,
                });
                self.active_tab = self.tabs.len() - 1;
                self.selected_cell = 0;
                self.cell_records.clear();
            }
            Err(error) => self.console = format!("Could not open {}: {error}", path.display()),
        }
    }

    fn save_active(&mut self) {
        let _ = self.save_tab(self.active_tab);
    }

    fn save_tab(&mut self, index: usize) -> bool {
        let path = self.tabs[index].path.clone().or_else(|| {
            let mut dialog = rfd::FileDialog::new().set_title("Save file");
            if let Some(project) = &self.project {
                dialog = dialog.set_directory(&project.root);
            }
            dialog.save_file()
        });
        let Some(path) = path else {
            return false;
        };
        let content = self.tabs[index].content.clone();
        match std::fs::write(&path, content) {
            Ok(()) => {
                let tab = &mut self.tabs[index];
                tab.path = Some(path.clone());
                tab.title = file_title(&path);
                tab.dirty = false;
                tab.disk_hash = std::fs::read_to_string(&path)
                    .ok()
                    .map(|content| content_hash(&content));
                tab.external_change_pending = false;
                self.console = format!("Saved {}", path.display());
                if let Some(project) = &mut self.project {
                    let _ = project.refresh();
                }
                self.run_diagnostics();
                true
            }
            Err(error) => {
                self.console = format!("Could not save {}: {error}", path.display());
                false
            }
        }
    }

    fn close_tab(&mut self, index: usize) {
        if self.tabs[index].dirty {
            self.pending_unsaved_action = Some(PendingUnsavedAction::CloseTab(index));
            return;
        }
        self.close_tab_now(index);
    }

    fn close_tab_now(&mut self, index: usize) {
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.tabs.push(blank_tab());
        }
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        self.selected_cell = 0;
        self.cell_records.clear();
    }

    fn unsaved_confirmation(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_unsaved_action.clone() else {
            return;
        };
        let dirty_names = self
            .tabs
            .iter()
            .filter(|tab| tab.dirty)
            .map(|tab| tab.title.clone())
            .collect::<Vec<_>>();
        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Save your changes before continuing?");
                ui.label(RichText::new(dirty_names.join(", ")).color(EMBER));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.pending_unsaved_action = None;
                    }
                    if ui.button("Discard").clicked() {
                        self.pending_unsaved_action = None;
                        match action.clone() {
                            PendingUnsavedAction::CloseTab(index) => self.close_tab_now(index),
                            PendingUnsavedAction::OpenProject(Some(path)) => {
                                self.open_project_path(path)
                            }
                            PendingUnsavedAction::OpenProject(None) => self.open_project_dialog(),
                        }
                    }
                    if ui.button("Save").clicked() {
                        let indices = match &action {
                            PendingUnsavedAction::CloseTab(index) => vec![*index],
                            PendingUnsavedAction::OpenProject(_) => self
                                .tabs
                                .iter()
                                .enumerate()
                                .filter_map(|(index, tab)| tab.dirty.then_some(index))
                                .collect(),
                        };
                        if indices.into_iter().all(|index| self.save_tab(index)) {
                            self.pending_unsaved_action = None;
                            match action.clone() {
                                PendingUnsavedAction::CloseTab(index) => self.close_tab_now(index),
                                PendingUnsavedAction::OpenProject(Some(path)) => {
                                    self.open_project_path(path)
                                }
                                PendingUnsavedAction::OpenProject(None) => {
                                    self.open_project_dialog()
                                }
                            }
                        }
                    }
                });
            });
    }

    fn navigate_to(&mut self, path: PathBuf, line: usize, column: usize) {
        self.open_file(path);
        let content = &self.active().content;
        let line_start = content
            .split_inclusive('\n')
            .take(line)
            .map(str::chars)
            .map(Iterator::count)
            .sum::<usize>();
        let line_length = content
            .lines()
            .nth(line)
            .map(str::chars)
            .map(Iterator::count)
            .unwrap_or(0);
        let offset = line_start + column.min(line_length);
        self.pending_editor_selection = Some((offset, offset));
        self.cursor_offset = offset;
        self.editor_needs_initial_focus = true;
    }

    fn find_next(&mut self) {
        if self.find_query.is_empty() {
            return;
        }
        let content = &self.active().content;
        let start_byte = char_to_byte(content, self.cursor_offset.saturating_add(1));
        let found = content[start_byte..]
            .find(&self.find_query)
            .map(|offset| start_byte + offset)
            .or_else(|| content[..start_byte].find(&self.find_query));
        if let Some(byte_start) = found {
            let byte_end = byte_start + self.find_query.len();
            let char_start = content[..byte_start].chars().count();
            let char_end = content[..byte_end].chars().count();
            self.pending_editor_selection = Some((char_start, char_end));
            self.cursor_offset = char_start;
        } else {
            self.console = format!("No matches for {:?}.", self.find_query);
        }
    }

    fn replace_current(&mut self) {
        let Some((start, end)) = self.pending_editor_selection else {
            self.find_next();
            return;
        };
        let start_byte = char_to_byte(&self.active().content, start);
        let end_byte = char_to_byte(&self.active().content, end);
        if self.active().content.get(start_byte..end_byte) == Some(self.find_query.as_str()) {
            let replacement = self.replace_query.clone();
            self.active_mut()
                .content
                .replace_range(start_byte..end_byte, &replacement);
            self.active_mut().dirty = true;
            self.cursor_offset = start + replacement.chars().count();
            self.pending_editor_selection = None;
        }
        self.find_next();
    }

    fn replace_all(&mut self) {
        if self.find_query.is_empty() {
            return;
        }
        let count = self.active().content.matches(&self.find_query).count();
        if count > 0 {
            let updated = self
                .active()
                .content
                .replace(&self.find_query, &self.replace_query);
            self.active_mut().content = updated;
            self.active_mut().dirty = true;
            self.pending_editor_selection = None;
        }
        self.console = format!("Replaced {count} occurrence(s).");
    }

    fn apply_completion(&mut self, completion: &str) {
        let cursor = self
            .cursor_offset
            .min(self.active().content.chars().count());
        let start = word_start_at(&self.active().content, cursor).unwrap_or(cursor);
        let start_byte = char_to_byte(&self.active().content, start);
        let end_byte = char_to_byte(&self.active().content, cursor);
        self.active_mut()
            .content
            .replace_range(start_byte..end_byte, completion);
        self.active_mut().dirty = true;
        let offset = start + completion.chars().count();
        self.cursor_offset = offset;
        self.pending_editor_selection = Some((offset, offset));
        self.completions.clear();
        self.completion_popup_open = false;
        self.lsp_status = format!("Inserted {completion}.");
    }

    fn save_experiment_run(&mut self) {
        if !self.data.has_telemetry() {
            self.console = "Run telemetry-producing cells before saving an experiment.".to_owned();
            return;
        }
        let name = if self.experiment_name.trim().is_empty() {
            format!("run_{}", self.saved_runs.len() + 1)
        } else {
            self.experiment_name.trim().to_owned()
        };
        let mut run = ExperimentRun::snapshot(
            name.clone(),
            &self.data.metrics,
            &self.data.vectors,
            self.execution_count,
        );
        run.tags = self
            .experiment_tags
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(str::to_owned)
            .collect();
        run.notes = self.experiment_notes.trim().to_owned();
        run.github.issue = (!self.experiment_github_issue.trim().is_empty())
            .then(|| self.experiment_github_issue.trim().to_owned());
        run.github.pull_request = (!self.experiment_github_pr.trim().is_empty())
            .then(|| self.experiment_github_pr.trim().to_owned());
        run.github.action_run = (!self.experiment_github_action.trim().is_empty())
            .then(|| self.experiment_github_action.trim().to_owned());
        run.provenance = capture_provenance(
            self.project.as_ref().map(|project| project.root.as_path()),
            self.data.fingerprints(),
        );
        if let Some(store) = &self.workspace_store {
            let artifact = PathBuf::from("runs").join(run.id.as_str()).join("run.json");
            run.artifacts.push(artifact.display().to_string());
            if let Ok(payload) = serde_json::to_vec_pretty(&run) {
                if let Err(error) = store.write_artifact(&artifact, &payload) {
                    self.console = format!("Could not persist run artifact: {error}");
                    return;
                }
            }
            if let Err(error) = store.save_experiment(&run.id, &run.name, &run) {
                self.console = format!("Could not persist experiment snapshot: {error}");
                return;
            }
        }
        self.saved_runs.push(run);
        self.experiment_name = format!("run_{}", self.saved_runs.len() + 1);
        self.inspector_tab = InspectorTab::Experiments;
        self.console = format!("Saved experiment snapshot {name}.");
    }

    fn export_telemetry_csv(&mut self) {
        if !self.data.has_telemetry() && self.saved_runs.is_empty() {
            self.console = "There is no telemetry to export.".to_owned();
            return;
        }
        let mut dialog = rfd::FileDialog::new()
            .set_title("Export Forge ML telemetry")
            .set_file_name("forge-ml-telemetry.csv");
        if let Some(project) = &self.project {
            dialog = dialog.set_directory(&project.root);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };
        let current = ExperimentRun::snapshot(
            "current".to_owned(),
            &self.data.metrics,
            &self.data.vectors,
            self.execution_count,
        );
        let runs = std::iter::once(&current).chain(self.saved_runs.iter());
        let mut csv = "run,kind,series,index,value,executions\n".to_owned();
        for run in runs {
            for (series, values) in &run.metrics {
                for point in values {
                    csv.push_str(&format!(
                        "{},{},{},{},{},{}\n",
                        csv_field(&run.name),
                        "metric",
                        csv_field(series),
                        point[0],
                        point[1],
                        run.execution_count
                    ));
                }
            }
            for (series, values) in &run.vectors {
                for (index, value) in values.iter().enumerate() {
                    csv.push_str(&format!(
                        "{},{},{},{},{},{}\n",
                        csv_field(&run.name),
                        "vector",
                        csv_field(series),
                        index,
                        value,
                        run.execution_count
                    ));
                }
            }
        }
        match std::fs::write(&path, csv) {
            Ok(()) => self.console = format!("Exported telemetry to {}", path.display()),
            Err(error) => self.console = format!("Could not export {}: {error}", path.display()),
        }
    }

    fn run_project_search(&mut self) {
        let query = self.project_search_query.clone();
        if query.is_empty() {
            self.project_search_results.clear();
            return;
        }
        let Some(project) = &self.project else {
            self.console = "Open a project before searching its files.".to_owned();
            return;
        };
        let mut paths = Vec::new();
        collect_editable_files(&project.files, &mut paths);
        let needle = if self.project_search_case_sensitive {
            query.clone()
        } else {
            query.to_ascii_lowercase()
        };
        let mut results = Vec::new();
        for path in paths {
            let content = self
                .tabs
                .iter()
                .find(|tab| tab.path.as_ref() == Some(&path))
                .map(|tab| tab.content.clone())
                .or_else(|| std::fs::read_to_string(&path).ok());
            let Some(content) = content else {
                continue;
            };
            for (line_index, line) in content.lines().enumerate() {
                let searchable = if self.project_search_case_sensitive {
                    line.to_owned()
                } else {
                    line.to_ascii_lowercase()
                };
                let mut byte_start = 0;
                while let Some(relative) = searchable[byte_start..].find(&needle) {
                    let byte_column = byte_start + relative;
                    results.push(ProjectSearchResult {
                        path: path.clone(),
                        line: line_index,
                        column: line[..byte_column].chars().count(),
                        preview: line.trim().to_owned(),
                    });
                    if results.len() >= 500 {
                        break;
                    }
                    byte_start = byte_column + needle.len().max(1);
                }
                if results.len() >= 500 {
                    break;
                }
            }
            if results.len() >= 500 {
                break;
            }
        }
        let count = results.len();
        self.project_search_results = results;
        self.inspector_tab = InspectorTab::Search;
        self.console = if count == 500 {
            "Project search reached the 500-result limit.".to_owned()
        } else {
            format!("Found {count} project result(s).")
        };
    }

    fn enqueue_cells(&mut self, ids: impl IntoIterator<Item = usize>) {
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

    fn run_next(&mut self) {
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
        let code = prepare_runtime_code(&code, self.active().path.as_deref());
        if self.runtime.execute(cell_id, code).is_ok() {
            self.run_state = RunState::Running(cell_id);
            let provenance = self.project_root().map(|root| git::provenance(&root));
            let record = self.cell_records.entry(cell_id).or_default();
            record.state = Some(CellState::Running);
            record.output.clear();
            record.rich_outputs.clear();
            if let Some((commit, dirty)) = provenance {
                record.git_commit = Some(commit);
                record.git_dirty = dirty;
            }
            self.console = format!("Compiling cell {}...", cell_id + 1);
        }
    }

    fn run_console_input(&mut self) {
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

    fn discover_python_runtimes(&mut self) {
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

    fn start_python_kernel(&mut self) {
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

    fn run_python_input(&mut self) {
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
    }

    fn run_diagnostics(&mut self) {
        self.inspector_tab = InspectorTab::Problems;
        if let Some(project) = &self.project {
            self.diagnostics.check(project.root.clone());
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

    fn sync_lsp(&mut self) {
        let (Some(root), Some(path)) = (
            self.project.as_ref().map(|project| project.root.clone()),
            self.active().path.clone(),
        ) else {
            return;
        };
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            return;
        }
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        self.active().content.hash(&mut hasher);
        let hash = hasher.finish();
        if hash == self.last_lsp_hash {
            return;
        }
        self.last_lsp_hash = hash;
        self.document_version += 1;
        let (text, _) = lsp_document(&self.active().content);
        self.lsp.send(LspCommand::Sync {
            root,
            path,
            text,
            version: self.document_version,
        });
    }

    fn request_lsp(&mut self, action: &str) {
        let Some(path) = self.active().path.clone() else {
            self.lsp_status = "Save this buffer before requesting language features.".to_owned();
            return;
        };
        let (text, prefix_chars) = lsp_document(&self.active().content);
        let char_offset = self.cursor_offset + prefix_chars;
        let command = match action {
            "complete" => LspCommand::Complete {
                path,
                text,
                char_offset,
            },
            "hover" => LspCommand::Hover {
                path,
                text,
                char_offset,
            },
            _ => LspCommand::Definition {
                path,
                text,
                char_offset,
            },
        };
        self.lsp.send(command);
    }

    fn probe_definition(&mut self, char_offset: usize) {
        let Some(path) = self.active().path.clone() else {
            return;
        };
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            return;
        }
        let (text, prefix_chars) = lsp_document(&self.active().content);
        self.lsp.send(LspCommand::ProbeDefinition {
            path,
            text,
            char_offset: char_offset + prefix_chars,
        });
    }

    fn poll_external_file_changes(&mut self) {
        if self.last_file_poll.elapsed() < Duration::from_millis(750) {
            return;
        }
        self.last_file_poll = Instant::now();
        let mut reloaded = Vec::new();
        let mut conflicted = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let Some(path) = tab.path.clone() else {
                continue;
            };
            let Ok(content) = std::fs::read_to_string(&path) else {
                if tab.disk_hash.is_some() && !tab.external_change_pending {
                    tab.external_change_pending = true;
                    conflicted.push(format!("{} is no longer readable on disk", tab.title));
                }
                continue;
            };
            let hash = content_hash(&content);
            if tab.disk_hash == Some(hash) {
                tab.external_change_pending = false;
                continue;
            }
            if tab.dirty {
                if !tab.external_change_pending {
                    tab.external_change_pending = true;
                    conflicted.push(format!(
                        "{} changed outside Forge; save or reopen it to resolve the conflict",
                        tab.title
                    ));
                }
                continue;
            }
            tab.content = content;
            tab.disk_hash = Some(hash);
            tab.external_change_pending = false;
            reloaded.push((index, tab.title.clone()));
        }
        if reloaded.iter().any(|(index, _)| *index == self.active_tab) {
            self.selected_cell = 0;
            self.cell_records.clear();
            self.pending_editor_selection = None;
            self.last_lsp_hash = 0;
        }
        if !conflicted.is_empty() {
            self.console = conflicted.join("\n");
        } else if !reloaded.is_empty() {
            let names = reloaded
                .into_iter()
                .map(|(_, name)| name)
                .collect::<Vec<_>>()
                .join(", ");
            self.console = format!("Reloaded external changes: {names}");
        }
    }

    fn external_change_banner(&mut self, ui: &mut egui::Ui) {
        if !self.active().external_change_pending {
            return;
        }
        let path_label = self
            .active()
            .path
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| self.active().title.clone());
        let mut reload = false;
        let mut keep = false;
        Frame::new()
            .fill(theme_colors(self.dark_mode).raised)
            .stroke(Stroke::new(1.0, RED))
            .inner_margin(Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("External change conflict")
                            .strong()
                            .color(RED),
                    );
                    ui.label(RichText::new(path_label).monospace().size(10.0));
                    reload = ui
                        .button("Reload from disk")
                        .on_hover_text("Discard unsaved Forge edits and load the disk version")
                        .clicked();
                    keep = ui
                        .button("Keep Forge version")
                        .on_hover_text("Keep the editor content and dismiss this disk change")
                        .clicked();
                });
            });
        if reload {
            let Some(path) = self.active().path.clone() else {
                return;
            };
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let hash = content_hash(&content);
                    let tab = self.active_mut();
                    tab.content = content;
                    tab.dirty = false;
                    tab.disk_hash = Some(hash);
                    tab.external_change_pending = false;
                    self.selected_cell = 0;
                    self.cell_records.clear();
                    self.pending_editor_selection = None;
                    self.last_lsp_hash = 0;
                    self.console = format!("Reloaded {} from disk.", path.display());
                }
                Err(error) => {
                    self.console = format!("Could not reload {}: {error}", path.display());
                }
            }
        } else if keep {
            let disk_hash = self
                .active()
                .path
                .as_deref()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .map(|content| content_hash(&content));
            let title = self.active().title.clone();
            let tab = self.active_mut();
            tab.disk_hash = disk_hash;
            tab.external_change_pending = false;
            tab.dirty = true;
            self.console = format!("Kept the Forge version of {title}.");
        }
    }

    fn poll_background(&mut self, ctx: &egui::Context) {
        self.poll_external_file_changes();
        if let Some(root) = self.project_root() {
            self.job_queue.poll(root);
        }
        if let Some(kernel) = &self.python_kernel {
            while let Some(result) = kernel.try_recv() {
                self.python_mime_outputs = result.mime;
                self.python_mime_outputs
                    .extend(python_kernel::mime_outputs(&result.output));
                self.python_console_output
                    .push_str(&format!("\nOut [{}]:\n{}", result.id, result.output));
            }
        }
        ctx.request_repaint_after(Duration::from_millis(750));
        while let Some(result) = self.runtime.try_recv() {
            match result {
                CellResult::Ready => {
                    self.run_state = RunState::Ready;
                    self.runtime_restart_attempts = 0;
                    self.console = "Runtime ready.".to_owned();
                }
                CellResult::Success {
                    cell_id,
                    output,
                    elapsed_ms,
                    variables,
                    events,
                } => {
                    self.run_state = RunState::Ready;
                    self.execution_count += 1;
                    self.variables = variables;
                    if cell_id != CONSOLE_CELL_ID {
                        let record = self.cell_records.entry(cell_id).or_default();
                        record.state = Some(CellState::Passed);
                        record.output = output.clone();
                        record.rich_outputs = if output.is_empty() {
                            Vec::new()
                        } else {
                            vec![RichOutput {
                                mime: "text/plain".into(),
                                data: output.clone(),
                            }]
                        };
                        record.elapsed_ms = Some(elapsed_ms);
                    }
                    self.console = if output.is_empty() {
                        format!("Cell {} completed in {elapsed_ms} ms.", cell_id + 1)
                    } else {
                        output
                    };
                    let (training_events, reports) =
                        millwright_studio::parse_runtime_output(&self.console);
                    for event in training_events {
                        self.training_observer.observe(event);
                    }
                    for event in self.training_observer.drain() {
                        if let TrainingEvent::TrialCompleted { trial, score } = &event {
                            self.leaderboard.push(LeaderboardEntry {
                                model: format!("trial {trial}"),
                                score: *score,
                                duration_ms: elapsed_ms,
                                parameters: String::new(),
                            });
                        }
                        self.training_events.push(event);
                    }
                    if let Some(report) = reports.into_iter().last() {
                        self.evaluation_report = report;
                    }
                    for envelope in events {
                        self.data.apply(envelope.event);
                    }
                    if cell_id != CONSOLE_CELL_ID {
                        self.run_next();
                    }
                }
                CellResult::Error {
                    cell_id,
                    message,
                    elapsed_ms,
                } => {
                    self.run_state = RunState::Failed;
                    self.run_queue.clear();
                    if cell_id != CONSOLE_CELL_ID {
                        let record = self.cell_records.entry(cell_id).or_default();
                        record.state = Some(CellState::Failed);
                        record.output = message.clone();
                        record.elapsed_ms = Some(elapsed_ms);
                        self.console = format!("Cell {} failed\n\n{message}", cell_id + 1);
                    } else {
                        self.console = format!("Console error\n\n{message}");
                    }
                }
                CellResult::Reset => {
                    self.run_state = RunState::Ready;
                    self.execution_count = 0;
                    self.variables.clear();
                    self.data.clear();
                    self.open_dataset = None;
                    self.cell_records.clear();
                    self.console = "Runtime state cleared.".to_owned();
                    if std::mem::take(&mut self.run_all_after_reset) {
                        self.enqueue_cells(0..self.cells().len());
                    }
                }
                CellResult::RuntimeError(message) => {
                    if self.runtime_restart_attempts < 2 {
                        self.runtime_restart_attempts += 1;
                        self.run_state = RunState::Booting;
                        self.run_queue.clear();
                        self.runtime = RuntimeHandle::spawn();
                        self.console = format!(
                            "Rust kernel failed and is restarting (attempt {}/2).\n\n{message}",
                            self.runtime_restart_attempts
                        );
                    } else {
                        self.run_state = RunState::Failed;
                        self.console =
                            format!("Runtime unavailable after recovery attempts\n\n{message}");
                    }
                }
            }
            ctx.request_repaint();
        }
        if let Some(lines) = self.diagnostics.try_recv() {
            self.diagnostic_lines = lines;
            self.diagnostics_running = false;
            ctx.request_repaint();
        }
        while let Some(event) = self.lsp.try_recv() {
            match event {
                LspEvent::Status(status) => self.lsp_status = status,
                LspEvent::Diagnostics { path, mut items } => {
                    if self.tabs.iter().any(|tab| {
                        tab.path.as_ref() == Some(&path) && is_notebook_document(&tab.content)
                    }) {
                        for diagnostic in &mut items {
                            diagnostic.line = diagnostic.line.saturating_sub(1);
                        }
                    }
                    self.lsp_diagnostics.insert(path, items);
                }
                LspEvent::Completions(items) => {
                    self.completions = items;
                    self.completion_popup_open = !self.completions.is_empty();
                    self.lsp_status = "Completion results ready.".to_owned();
                }
                LspEvent::Hover(text) => {
                    self.hover_text = text;
                    self.inspector_tab = InspectorTab::Help;
                    self.lsp_status = "Hover information ready.".to_owned();
                }
                LspEvent::Definition { path, line } => {
                    let notebook = self
                        .tabs
                        .iter()
                        .find(|tab| tab.path.as_ref() == Some(&path))
                        .map(|tab| is_notebook_document(&tab.content))
                        .or_else(|| {
                            std::fs::read_to_string(&path)
                                .ok()
                                .map(|text| is_notebook_document(&text))
                        })
                        .unwrap_or(false);
                    let editor_line = if notebook {
                        line.saturating_sub(1)
                    } else {
                        line
                    };
                    self.navigate_to(path.clone(), editor_line, 0);
                    self.console = format!("Definition: {}:{}", path.display(), editor_line + 1);
                }
                LspEvent::DefinitionProbe {
                    char_offset,
                    navigable,
                } => {
                    let char_offset = if is_notebook_document(&self.active().content) {
                        char_offset.saturating_sub(notebook_lsp_prefix_chars())
                    } else {
                        char_offset
                    };
                    self.definition_probe_pending = false;
                    if self.hover_probe_offset == Some(char_offset) {
                        self.navigable_hover_offset = navigable.then_some(char_offset);
                    }
                }
                LspEvent::Installed(success) => {
                    if success {
                        self.last_lsp_hash = 0;
                        self.sync_lsp();
                    }
                }
            }
            ctx.request_repaint();
        }
        if matches!(self.run_state, RunState::Running(_) | RunState::Booting)
            || self.diagnostics_running
            || self.definition_probe_pending
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("FORGE ML").strong().color(RED));
            ui.separator();
            ui.menu_button("File", |ui| {
                if ui.button("New file...   Ctrl+N").clicked() {
                    self.create_new_file(None);
                    ui.close();
                }
                if ui.button("Open project...").clicked() {
                    self.open_project();
                }
                if ui.button("Import Jupyter notebook...").clicked() {
                    self.import_ipynb();
                    ui.close();
                }
                if ui.button("Export as .ipynb...").clicked() {
                    self.export_ipynb();
                    ui.close();
                }
                let recent = self.recent_projects.clone();
                ui.menu_button("Open recent", |ui| {
                    if recent.is_empty() {
                        ui.label("No recent projects");
                    }
                    for path in recent {
                        if ui.button(path.display().to_string()).clicked() {
                            self.request_open_project_path(path);
                            ui.close();
                        }
                    }
                    if !self.recent_projects.is_empty() {
                        ui.separator();
                        if ui.button("Clear recent projects").clicked() {
                            self.recent_projects.clear();
                            ui.close();
                        }
                    }
                });
                if ui.button("Save   Ctrl+S").clicked() {
                    self.save_active();
                }
                if ui.button("Close editor tab").clicked() {
                    self.close_tab(self.active_tab);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Undo   Ctrl+Z").clicked() {
                    self.pending_editor_history = Some(EditorHistoryCommand::Undo);
                    ui.close();
                }
                if ui.button("Redo   Ctrl+Y / Ctrl+Shift+Z").clicked() {
                    self.pending_editor_history = Some(EditorHistoryCommand::Redo);
                    ui.close();
                }
            });
            ui.menu_button("Search", |ui| {
                if ui.button("Find in files   Ctrl+Shift+F").clicked() {
                    self.inspector_tab = InspectorTab::Search;
                    ui.close();
                }
            });
            ui.menu_button("Source", |ui| {
                if ui.button("Run code analysis").clicked() {
                    self.run_diagnostics();
                }
            });
            ui.menu_button("Run", |ui| {
                if ui.button("Run cell   Shift+Enter").clicked() {
                    self.enqueue_cells([self.selected_cell]);
                }
                if ui.button("Run cells above").clicked() {
                    self.enqueue_cells(0..=self.selected_cell);
                }
                if ui.button("Run all   Ctrl+Shift+Enter").clicked() {
                    self.enqueue_cells(0..self.cells().len());
                }
                if ui.button("Restart and run all").clicked() {
                    self.restart_and_run_all();
                }
                if ui
                    .add_enabled(
                        matches!(self.run_state, RunState::Running(_)),
                        egui::Button::new("Stop execution"),
                    )
                    .clicked()
                {
                    self.stop_execution();
                }
            });
            ui.menu_button("Debug", |ui| {
                ui.label("Debugger integration is not connected yet.");
            });
            ui.menu_button("Tools", |ui| {
                if ui.button("Import dataset...").clicked() {
                    self.import_dataset();
                    ui.close();
                }
                #[cfg(feature = "millwright")]
                if ui.button("Import via Millwright...").clicked() {
                    self.import_millwright_dataset();
                    ui.close();
                }
                if ui.button("Git workbench").clicked() {
                    self.inspector_tab = InspectorTab::Git;
                    ui.close();
                }
                if ui.button("Rust packages").clicked() {
                    self.inspector_tab = InspectorTab::Packages;
                    ui.close();
                }
                if ui.button("Discover Jupyter kernels").clicked() {
                    self.discover_jupyter();
                    ui.close();
                }
                if ui.button("Install Evcxr Jupyter kernel").clicked() {
                    self.jupyter_output = jupyter::install_evcxr().unwrap_or_else(|e| e);
                    self.hover_text = self.jupyter_output.clone();
                    self.inspector_tab = InspectorTab::Help;
                    ui.close();
                }
                if ui.button("Settings...").clicked() {
                    self.settings_open = true;
                    ui.close();
                }
                if ui.button("Restart Rust console").clicked() {
                    let _ = self.runtime.reset();
                    self.run_state = RunState::Booting;
                }
            });
            ui.menu_button("View", |ui| {
                let label = if self.dark_mode {
                    "Use light theme"
                } else {
                    "Use dark theme"
                };
                if ui.button(label).clicked() {
                    self.dark_mode = !self.dark_mode;
                    configure_style(ui.ctx(), self.dark_mode);
                    ui.close();
                }
                ui.label("Drag pane dividers to resize the workspace.");
            });
            ui.menu_button("Help", |ui| {
                ui.label(format!(
                    "Forge ML {APP_VERSION} - interactive Rust scientific environment"
                ));
            });
        });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            use egui_phosphor_icons::icons;
            if toolbar_icon_button(ui, icons::FOLDER_OPEN, "Open project").clicked() {
                self.open_project();
            }
            if toolbar_icon_button(ui, icons::FLOPPY_DISK, "Save file (Ctrl+S)").clicked() {
                self.save_active();
            }
            if toolbar_icon_button(
                ui,
                icons::ARROW_COUNTER_CLOCKWISE,
                "Undo editor change (Ctrl+Z)",
            )
            .clicked()
            {
                self.pending_editor_history = Some(EditorHistoryCommand::Undo);
            }
            if toolbar_icon_button(
                ui,
                icons::ARROW_CLOCKWISE,
                "Redo editor change (Ctrl+Y / Ctrl+Shift+Z)",
            )
            .clicked()
            {
                self.pending_editor_history = Some(EditorHistoryCommand::Redo);
            }
            ui.separator();
            if toolbar_icon_button(ui, icons::CHECK_CIRCLE, "Run code analysis").clicked() {
                self.run_diagnostics();
            }
            if toolbar_icon_button(
                ui,
                icons::MAGIC_WAND,
                "Request rust-analyzer completions at the cursor",
            )
            .clicked()
            {
                self.request_lsp("complete");
            }
            if toolbar_icon_button(ui, icons::INFO, "Show type and documentation at the cursor")
                .clicked()
            {
                self.request_lsp("hover");
            }
            if toolbar_icon_button(
                ui,
                icons::ARROW_SQUARE_OUT,
                "Open the definition at the cursor",
            )
            .clicked()
            {
                self.request_lsp("definition");
            }
            ui.separator();
            let ready = !matches!(self.run_state, RunState::Running(_) | RunState::Booting);
            if enabled_toolbar_icon_button(
                ui,
                ready,
                icons::PLAY,
                "Run selected cell (Shift+Enter)",
            )
            .clicked()
            {
                self.enqueue_cells([self.selected_cell]);
            }
            if enabled_toolbar_icon_button(ui, ready, icons::ARROW_LINE_UP, "Run cells above")
                .clicked()
            {
                self.enqueue_cells(0..=self.selected_cell);
            }
            if enabled_toolbar_icon_button(ui, ready, icons::PLAYLIST, "Run all cells").clicked() {
                self.enqueue_cells(0..self.cells().len());
            }
            if toolbar_icon_button(ui, icons::ARROWS_CLOCKWISE, "Reset Rust runtime").clicked() {
                let _ = self.runtime.reset();
                self.run_state = RunState::Booting;
            }
            if enabled_toolbar_icon_button(
                ui,
                matches!(self.run_state, RunState::Running(_)),
                icons::STOP,
                "Stop execution and restart the Rust runtime",
            )
            .clicked()
            {
                self.stop_execution();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(14.0);
                ui.label(
                    RichText::new(
                        self.project
                            .as_ref()
                            .map(|p| p.root.display().to_string())
                            .unwrap_or_else(|| "No project".to_owned()),
                    )
                    .size(11.0)
                    .color(MUTED),
                );
            });
        });
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        egui::Window::new("Forge ML settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.heading("Appearance");
                let mut dark = self.dark_mode;
                ui.horizontal(|ui| {
                    ui.label("Color theme");
                    ui.selectable_value(&mut dark, false, "Light");
                    ui.selectable_value(&mut dark, true, "Dark");
                });
                if dark != self.dark_mode {
                    self.dark_mode = dark;
                    configure_style(ctx, self.dark_mode);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Editor font size");
                    ui.add(
                        egui::Slider::new(&mut self.editor_font_size, 10.0..=24.0)
                            .suffix(" px")
                            .step_by(1.0),
                    );
                });
                ui.checkbox(&mut self.caret_blink, "Blink editor caret");
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Changes apply immediately and are saved for the next session.")
                        .size(10.0)
                        .color(MUTED),
                );
                if ui.button("Restore appearance defaults").clicked() {
                    self.dark_mode = false;
                    self.editor_font_size = default_editor_font_size();
                    self.caret_blink = default_true();
                    configure_style(ctx, self.dark_mode);
                }
            });
        self.settings_open = open;
    }

    fn file_explorer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("FILES").size(10.0).strong().color(MUTED));
            if ui.small_button("Open").clicked() {
                self.open_project();
            }
            if ui.small_button("New").clicked() {
                self.create_new_file(None);
            }
            let selected_file = self.active().path.clone().filter(|path| {
                self.project
                    .as_ref()
                    .is_some_and(|project| path.starts_with(&project.root) && path.is_file())
            });
            if ui
                .add_enabled(selected_file.is_some(), egui::Button::new("Delete").small())
                .clicked()
            {
                self.pending_delete = selected_file;
            }
            if ui.small_button("Refresh").clicked() {
                if let Some(project) = &mut self.project {
                    let _ = project.refresh();
                }
            }
        });
        ui.add_space(5.0);
        let selected = self.active().path.clone();
        let action = self.project.as_ref().and_then(|project| {
            ui.label(
                RichText::new(
                    project
                        .root
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("project"),
                )
                .strong()
                .color(TEXT),
            );
            egui::ScrollArea::vertical()
                .id_salt("project_file_tree")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    draw_file_nodes(ui, &project.files, selected.as_deref())
                })
                .inner
        });
        if let Some(action) = action {
            match action {
                ExplorerAction::Open(path) => self.open_file(path),
                ExplorerAction::NewFile(directory) => self.create_new_file(Some(directory)),
                ExplorerAction::Delete(path) => self.pending_delete = Some(path),
            }
        }
        if self.project.is_none() {
            ui.label(
                RichText::new("Open a Cargo project to begin.")
                    .size(11.0)
                    .color(MUTED),
            );
        }
    }

    fn cell_navigator(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.left_tab == LeftTab::Project, "Project")
                .clicked()
            {
                self.left_tab = LeftTab::Project;
            }
            if ui
                .selectable_label(self.left_tab == LeftTab::Outline, "Outline")
                .clicked()
            {
                self.left_tab = LeftTab::Outline;
            }
        });
        ui.separator();
        let available_height = ui.available_height();
        let max_explorer = (available_height - 110.0).max(90.0);
        self.explorer_height = self.explorer_height.clamp(90.0, max_explorer);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), self.explorer_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_width(ui.available_width());
                if self.left_tab == LeftTab::Project {
                    self.file_explorer(ui);
                } else {
                    self.outline(ui);
                }
            },
        );

        let (divider_rect, divider) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 8.0), egui::Sense::drag());
        if divider.hovered() || divider.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if divider.dragged() {
            self.explorer_height = (self.explorer_height
                + ui.input(|input| input.pointer.delta().y))
            .clamp(90.0, max_explorer);
            ui.ctx().request_repaint();
        }
        let divider_color = if divider.hovered() || divider.dragged() {
            CYAN
        } else {
            theme_colors(self.dark_mode).border
        };
        ui.painter().line_segment(
            [divider_rect.left_center(), divider_rect.right_center()],
            Stroke::new(if divider.dragged() { 2.0 } else { 1.0 }, divider_color),
        );

        ui.label(
            RichText::new("NOTEBOOK CELLS")
                .size(10.0)
                .strong()
                .color(MUTED),
        );
        ui.horizontal(|ui| {
            use egui_phosphor_icons::icons;
            if compact_icon_button(ui, icons::PLUS, "Insert cell after").clicked() {
                self.insert_cell_after();
            }
            if compact_icon_button(ui, icons::ARROW_UP, "Move cell up").clicked() {
                self.move_selected_cell(-1);
            }
            if compact_icon_button(ui, icons::ARROW_DOWN, "Move cell down").clicked() {
                self.move_selected_cell(1);
            }
            if compact_icon_button(ui, icons::TRASH, "Delete selected cell").clicked() {
                self.delete_selected_cell();
            }
            if compact_icon_button(ui, icons::BROOM, "Clear cell outputs").clicked() {
                self.cell_records.clear();
                self.console = "Cell outputs cleared.".to_owned();
            }
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("notebook_cell_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                for (index, (title, _)) in self.cells().iter().enumerate() {
                    let state = self
                        .cell_records
                        .get(&index)
                        .and_then(|r| r.state)
                        .unwrap_or(CellState::Idle);
                    let (mark, color) = match state {
                        CellState::Queued => ("[q]", MUTED),
                        CellState::Running => ("[>]", EMBER),
                        CellState::Passed => ("[ok]", GREEN),
                        CellState::Failed => ("[!]", RED),
                        CellState::Idle => ("[ ]", MUTED),
                    };
                    if ui
                        .selectable_label(
                            index == self.selected_cell,
                            RichText::new(format!("{mark}  {:02}  {title}", index + 1))
                                .monospace()
                                .color(if index == self.selected_cell {
                                    CYAN
                                } else {
                                    color
                                }),
                        )
                        .clicked()
                    {
                        self.selected_cell = index;
                    }
                }
            });
        ui.add_space(14.0);
        ui.label(RichText::new("SESSION").size(10.0).strong().color(MUTED));
        status_row(ui, "Engine", "Evcxr", GREEN);
        status_row(ui, "Runs", &self.execution_count.to_string(), CYAN);
    }

    fn outline(&mut self, ui: &mut egui::Ui) {
        let title = self.active().title.clone();
        let path = self.active().path.clone();
        let content = self.active().content.clone();
        ui.label(RichText::new(title).strong().color(TEXT));
        let mut selected_line = None;
        for (line_no, line) in content.lines().enumerate() {
            let line = line.trim();
            let symbol = ["fn ", "struct ", "enum ", "trait ", "impl ", "mod "]
                .iter()
                .find_map(|prefix| {
                    line.strip_prefix(prefix).map(|rest| {
                        format!(
                            "{}{}",
                            prefix.trim(),
                            rest.split(['(', '{', '<', ' ']).next().unwrap_or("")
                        )
                    })
                });
            if let Some(symbol) = symbol {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("-  {symbol}  :{}", line_no + 1))
                                .monospace()
                                .size(11.0)
                                .color(CYAN),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Go to symbol")
                    .clicked()
                {
                    selected_line = Some(line_no);
                }
            }
        }
        if let Some(line) = selected_line {
            if let Some(path) = path {
                self.navigate_to(path, line, 0);
            } else {
                let offset = content
                    .split_inclusive('\n')
                    .take(line)
                    .map(str::chars)
                    .map(Iterator::count)
                    .sum();
                self.pending_editor_selection = Some((offset, offset));
            }
        }
        ui.add_space(8.0);
        ui.separator();
    }

    fn editor_tabs(&mut self, ui: &mut egui::Ui) {
        let mut select = None;
        let mut close = None;
        egui::ScrollArea::horizontal()
            .id_salt("editor_tab_strip")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, tab) in self.tabs.iter().enumerate() {
                        let title = if tab.external_change_pending {
                            format!("! {}", tab.title)
                        } else if tab.dirty {
                            format!("* {}", tab.title)
                        } else {
                            tab.title.clone()
                        };
                        if ui
                            .selectable_label(
                                index == self.active_tab,
                                RichText::new(title).color(if tab.external_change_pending {
                                    RED
                                } else if tab.dirty {
                                    EMBER
                                } else {
                                    TEXT
                                }),
                            )
                            .clicked()
                        {
                            select = Some(index);
                        }
                        if index == self.active_tab
                            && compact_icon_button(
                                ui,
                                egui_phosphor_icons::icons::X,
                                "Close editor tab",
                            )
                            .clicked()
                        {
                            close = Some(index);
                        }
                        ui.separator();
                    }
                })
            });
        if let Some(index) = select {
            self.active_tab = index;
            self.selected_cell = 0;
            self.cell_records.clear();
        }
        if let Some(index) = close {
            self.close_tab(index);
        }
    }

    fn apply_pending_editor_history(&mut self, ui: &mut egui::Ui) {
        let Some(command) = self.pending_editor_history.take() else {
            return;
        };
        let id = ui.make_persistent_id(format!("editor_{}", self.active_tab));
        let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), id) else {
            return;
        };
        let cursor = state.cursor.char_range().unwrap_or_else(|| {
            egui::text::CCursorRange::one(egui::text::CCursor::new(self.cursor_offset))
        });
        let mut undoer = state.undoer();
        let current = (cursor, self.active().content.clone());
        let restored = match command {
            EditorHistoryCommand::Undo => undoer.undo(&current),
            EditorHistoryCommand::Redo => undoer.redo(&current),
        }
        .cloned();
        let Some((cursor, content)) = restored else {
            return;
        };
        state.set_undoer(undoer);
        state.cursor.set_char_range(Some(cursor));
        state.store(ui.ctx(), id);
        self.cursor_offset = cursor.primary.index.into();
        self.active_mut().content = content;
        self.active_mut().dirty = true;
        self.cell_records.clear();
        self.last_lsp_hash = 0;
    }

    fn right_sidebar(&mut self, ui: &mut egui::Ui) {
        if !self.dataset_viewer_docked || self.open_dataset.is_none() {
            self.inspector(ui);
            return;
        }

        let available_height = ui.available_height();
        let divider_height = 12.0;
        let min_pane_height = 120.0;
        let max_dataset_height =
            (available_height - min_pane_height - divider_height).max(min_pane_height);
        self.dataset_pane_height = self
            .dataset_pane_height
            .clamp(min_pane_height, max_dataset_height);
        let inspector_height =
            (available_height - self.dataset_pane_height - divider_height).max(min_pane_height);

        let (inspector_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), inspector_height),
            egui::Sense::hover(),
        );
        let mut inspector_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("right_inspector_top")
                .max_rect(inspector_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        inspector_ui.set_clip_rect(inspector_rect);
        self.inspector(&mut inspector_ui);

        let (divider_rect, divider) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), divider_height),
            egui::Sense::drag(),
        );
        if divider.hovered() || divider.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if divider.dragged() {
            self.dataset_pane_height = (self.dataset_pane_height
                - ui.input(|input| input.pointer.delta().y))
            .clamp(min_pane_height, max_dataset_height);
            ui.ctx().request_repaint();
        }
        let divider_color = if divider.hovered() || divider.dragged() {
            CYAN
        } else {
            theme_colors(self.dark_mode).border
        };
        if divider.hovered() || divider.dragged() {
            ui.painter().rect_filled(
                divider_rect,
                2.0,
                CYAN.gamma_multiply(if self.dark_mode { 0.14 } else { 0.09 }),
            );
        }
        ui.painter().line_segment(
            [divider_rect.left_center(), divider_rect.right_center()],
            Stroke::new(if divider.dragged() { 2.0 } else { 1.0 }, divider_color),
        );

        let (dataset_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), ui.available_height()),
            egui::Sense::hover(),
        );
        let mut dataset_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("right_dataset_bottom")
                .max_rect(dataset_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        dataset_ui.set_clip_rect(dataset_rect);
        self.docked_dataset_viewer(&mut dataset_ui);
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (InspectorTab::Variables, "Variables"),
                (InspectorTab::Data, "Data"),
                (InspectorTab::Charts, "Plots"),
                (InspectorTab::Experiments, "Runs"),
                (InspectorTab::Search, "Search"),
                (InspectorTab::Help, "Help"),
                (InspectorTab::Problems, "Problems"),
                (InspectorTab::Git, "Git"),
                (InspectorTab::Packages, "Crates"),
                (InspectorTab::GitHub, "GitHub"),
                (InspectorTab::Studio, "Studio"),
            ] {
                if ui
                    .selectable_label(self.inspector_tab == tab, label)
                    .clicked()
                {
                    self.inspector_tab = tab;
                }
            }
        });
        ui.separator();
        ui.add_space(7.0);
        match self.inspector_tab {
            InspectorTab::Variables => {
                ui.label(
                    RichText::new("LIVE EVCXR STATE")
                        .size(10.0)
                        .strong()
                        .color(MUTED),
                );
                egui::Grid::new("variables_table")
                    .striped(true)
                    .min_col_width(72.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Name").strong());
                        ui.label(RichText::new("Type").strong());
                        ui.label(RichText::new("Size").strong());
                        ui.end_row();
                        for variable in &self.variables {
                            ui.label(RichText::new(&variable.name).color(CYAN));
                            ui.label(RichText::new(&variable.type_name).monospace().size(10.0));
                            ui.label(
                                RichText::new(infer_size(&variable.type_name))
                                    .monospace()
                                    .size(10.0)
                                    .color(MUTED),
                            );
                            ui.end_row();
                        }
                    });
                if self.variables.is_empty() {
                    ui.label(RichText::new("Run a cell to inspect its variables.").color(MUTED));
                }
            }
            InspectorTab::Data => self.data_inspector(ui),
            InspectorTab::Charts => self.charts(ui),
            InspectorTab::Experiments => self.experiments(ui),
            InspectorTab::Search => self.project_search(ui),
            InspectorTab::Help => {
                ui.heading("Rust language help");
                ui.label(RichText::new(&self.lsp_status).color(MUTED));
                if ui.button("Install or repair language support").clicked() {
                    self.lsp.install();
                    self.lsp_status = "Installing rust-analyzer and rust-src...".to_owned();
                }
                if !self.hover_text.is_empty() {
                    ui.separator();
                    ui.label(RichText::new("Hover").strong().color(CYAN));
                    egui::ScrollArea::vertical()
                        .id_salt("help_hover_documentation")
                        .max_height(150.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(&self.hover_text).monospace().size(10.0));
                        });
                }
                if !self.completions.is_empty() {
                    ui.separator();
                    ui.label(RichText::new("Completions").strong().color(CYAN));
                    let mut selected = None;
                    egui::ScrollArea::vertical()
                        .id_salt("help_completion_results")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for item in &self.completions {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(item).monospace().size(10.0),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text("Insert completion")
                                    .clicked()
                                {
                                    selected = Some(item.clone());
                                }
                            }
                        });
                    if let Some(completion) = selected {
                        self.apply_completion(&completion);
                    }
                }
                ui.separator();
                ui.heading("Scientific console");
                ui.label("Execute `//# %%` cells in a persistent Evcxr session. Variables created by successful cells remain available to later cells and console commands.");
                ui.separator();
                ui.label(RichText::new("Telemetry").strong().color(CYAN));
                ui.code("println!(\"forge_metric:loss={}\", loss);\nprintln!(\"forge_vector:w=1,2,3\");\nprintln!(r#\"forge_table:samples={{\\\"columns\\\":[\\\"x\\\",\\\"label\\\"],\\\"rows\\\":[[1.0,\\\"cat\\\"]]}}\"#);");
                ui.separator();
                ui.label(RichText::new("Shortcuts").strong().color(CYAN));
                ui.label("Shift+Enter  Run cell\nCtrl+Shift+Enter  Run all\nCtrl+Space  Show completions\nCtrl+S  Save file\nCtrl+N  New file\nCtrl+F  Find and replace\nCtrl+Shift+F  Find in project");
            }
            InspectorTab::Problems => {
                if ui.button("Run cargo check").clicked() {
                    self.run_diagnostics();
                }
                let mut navigate = None;
                egui::ScrollArea::vertical()
                    .id_salt("problems_diagnostic_list")
                    .show(ui, |ui| {
                        for (path, diagnostics) in &self.lsp_diagnostics {
                            for diagnostic in diagnostics {
                                let color = if diagnostic.severity == 1 {
                                    RED
                                } else if diagnostic.severity == 2 {
                                    EMBER
                                } else {
                                    TEXT
                                };
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(format!(
                                                "{}:{}:{}  {}",
                                                file_title(path),
                                                diagnostic.line + 1,
                                                diagnostic.column + 1,
                                                diagnostic.message
                                            ))
                                            .monospace()
                                            .size(10.0)
                                            .color(color),
                                        )
                                        .frame(false)
                                        .wrap(),
                                    )
                                    .on_hover_text("Open this diagnostic")
                                    .clicked()
                                {
                                    navigate = Some((
                                        path.clone(),
                                        diagnostic.line as usize,
                                        diagnostic.column as usize,
                                    ));
                                }
                            }
                        }
                        for line in &self.diagnostic_lines {
                            let color = if line.contains("error") {
                                RED
                            } else if line.contains("warning") {
                                EMBER
                            } else {
                                TEXT
                            };
                            ui.label(RichText::new(line).monospace().size(10.0).color(color));
                        }
                    });
                if let Some((path, line, column)) = navigate {
                    self.navigate_to(path, line, column);
                }
            }
            InspectorTab::Git => self.git_inspector(ui),
            InspectorTab::Packages => self.packages_inspector(ui),
            InspectorTab::GitHub => self.github_inspector(ui),
            InspectorTab::Studio => self.millwright_studio(ui),
        }
    }

    fn millwright_studio(&mut self, ui: &mut egui::Ui) {
        ui.heading("Millwright Studio");
        ui.horizontal(|ui| {
            ui.label("Pipeline");
            ui.text_edit_singleline(&mut self.pipeline_design.name);
            ui.label("Target");
            ui.text_edit_singleline(&mut self.pipeline_design.target);
        });
        ui.horizontal_wrapped(|ui| {
            for step in [
                PipelineStep::Impute,
                PipelineStep::Standardize,
                PipelineStep::OneHotEncode,
                PipelineStep::RandomForest,
                PipelineStep::LogisticRegression,
                PipelineStep::LinearRegression,
            ] {
                if ui.small_button(format!("+ {}", step.label())).clicked() {
                    self.pipeline_design.steps.push(step);
                }
            }
            if ui.small_button("Remove last").clicked() {
                self.pipeline_design.steps.pop();
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (index, step) in self.pipeline_design.steps.iter().enumerate() {
                ui.label(RichText::new(format!("{}  {}", index + 1, step.label())).color(CYAN));
                if index + 1 < self.pipeline_design.steps.len() {
                    ui.label("→");
                }
            }
        });
        if ui.button("Generate Rust notebook cell").clicked() {
            let code = self.pipeline_design.rust_code();
            self.tabs.push(EditorTab {
                title: format!("{}.rs", self.pipeline_design.name),
                path: None,
                content: format!("//# %% generated Millwright pipeline\n{code}"),
                dirty: true,
                disk_hash: None,
                external_change_pending: false,
            });
            self.active_tab = self.tabs.len() - 1;
            self.selected_cell = 0;
        }
        ui.separator();
        ui.strong("Training progress");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.job_command)
                    .hint_text("background training command"),
            );
            if ui.button("Queue job").clicked() {
                if let Some(root) = self.project_root() {
                    match self.job_queue.enqueue(self.job_command.clone(), root) {
                        Ok(id) => self.console = format!("Queued training job {id}."),
                        Err(error) => self.console = error,
                    }
                }
            }
        });
        let active = self
            .job_queue
            .jobs
            .iter()
            .filter(|job| job.state == JobState::Running)
            .count();
        let queued = self
            .job_queue
            .jobs
            .iter()
            .filter(|job| job.state == JobState::Queued)
            .count();
        ui.label(format!(
            "Workers: {active}/1 active · {queued} queued · ETA {}",
            self.job_queue
                .eta()
                .map(|eta| format!("{:.1}s", eta.as_secs_f64()))
                .unwrap_or_else(|| "calculating".into())
        ));
        for job in self.job_queue.jobs.iter().rev().take(8) {
            ui.collapsing(
                format!(
                    "Job {} · {:?} · {:.1}s · queued {:.1}s ago",
                    job.id,
                    job.state,
                    job.elapsed.as_secs_f64(),
                    job.queued_at.elapsed().as_secs_f64()
                ),
                |ui| {
                    ui.code(&job.command);
                    if !job.output.is_empty() {
                        ui.code(&job.output);
                    }
                },
            );
        }
        if self.training_events.is_empty() {
            ui.label("Emit `forge_training:<json>` from a Rust cell to stream trials, folds, epochs, and scores.");
        }
        egui::ScrollArea::vertical()
            .max_height(130.0)
            .show(ui, |ui| {
                for event in self.training_events.iter().rev().take(100) {
                    ui.label(RichText::new(format!("{event:?}")).monospace().size(9.0));
                }
            });
        if !self.leaderboard.is_empty() {
            self.leaderboard.sort_by(|a, b| b.score.total_cmp(&a.score));
            ui.collapsing("AutoML leaderboard", |ui| {
                egui::Grid::new("automl_leaderboard")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Model");
                        ui.strong("Score");
                        ui.strong("Time");
                        ui.strong("Parameters");
                        ui.end_row();
                        for entry in &self.leaderboard {
                            ui.label(&entry.model);
                            ui.label(format!("{:.5}", entry.score));
                            ui.label(format!("{} ms", entry.duration_ms));
                            ui.label(&entry.parameters);
                            ui.end_row();
                        }
                    });
            });
        }
        ui.collapsing("Evaluation and explainability", |ui| {
            ui.label(format!(
                "Accuracy: {}   RMSE: {}",
                self.evaluation_report
                    .accuracy
                    .map(|v| format!("{v:.4}"))
                    .unwrap_or_else(|| "—".into()),
                self.evaluation_report
                    .rmse
                    .map(|v| format!("{v:.4}"))
                    .unwrap_or_else(|| "—".into())
            ));
            if !self.evaluation_report.confusion.is_empty() {
                egui::Grid::new("confusion_matrix")
                    .striped(true)
                    .show(ui, |ui| {
                        for row in &self.evaluation_report.confusion {
                            for value in row {
                                ui.label(value.to_string());
                            }
                            ui.end_row();
                        }
                    });
            }
            if !self.evaluation_report.roc.is_empty() {
                Plot::new("roc_curve").height(150.0).show(ui, |plot_ui| {
                    plot_ui.line(Line::new(
                        "ROC",
                        PlotPoints::from(self.evaluation_report.roc.clone()),
                    ));
                });
            }
            if !self.evaluation_report.residuals.is_empty() {
                Plot::new("residuals").height(150.0).show(ui, |plot_ui| {
                    let points = self
                        .evaluation_report
                        .residuals
                        .iter()
                        .enumerate()
                        .map(|(i, v)| [i as f64, *v])
                        .collect::<Vec<_>>();
                    plot_ui.line(Line::new("Residuals", PlotPoints::from(points)));
                });
            }
            for (feature, importance) in &self.evaluation_report.feature_importance {
                ui.add(
                    egui::ProgressBar::new(*importance as f32)
                        .text(format!("{feature}  {importance:.3}")),
                );
            }
        });
        if ui.button("Discover Python runtimes").clicked() {
            let runtimes = python_runtime::discover();
            self.python_runtime_output = if runtimes.is_empty() {
                "No Python runtime found.".into()
            } else {
                runtimes
                    .iter()
                    .flat_map(python_runtime::compatibility)
                    .collect::<Vec<_>>()
                    .join("\n")
            };
        }
        if !self.python_runtime_output.is_empty() {
            ui.collapsing("Python runtime compatibility", |ui| {
                ui.code(&self.python_runtime_output);
                ui.label(
                    "Packages remain user-managed; Forge does not bundle Python ML frameworks.",
                );
            });
        }
    }

    fn import_ipynb(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Jupyter notebook", &["ipynb"])
            .pick_file()
        else {
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| NotebookDocument::from_ipynb(&text));
        match result {
            Ok(notebook) => {
                self.tabs.push(EditorTab {
                    title: format!("{}.rs", file_title(&path)),
                    path: None,
                    content: notebook.to_rust(),
                    dirty: true,
                    disk_hash: None,
                    external_change_pending: false,
                });
                self.active_tab = self.tabs.len() - 1;
                self.selected_cell = 0;
                self.console = format!(
                    "Imported {} using kernel `{}`.",
                    path.display(),
                    notebook.kernel
                );
            }
            Err(error) => self.console = format!("Could not import notebook: {error}"),
        }
    }

    fn export_ipynb(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Jupyter notebook", &["ipynb"])
            .set_file_name("notebook.ipynb")
            .save_file()
        else {
            return;
        };
        let notebook = NotebookDocument::parse_rust(&self.active().content);
        match notebook
            .to_ipynb()
            .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()))
        {
            Ok(()) => self.console = format!("Exported {}.", path.display()),
            Err(error) => self.console = format!("Could not export notebook: {error}"),
        }
    }

    fn discover_jupyter(&mut self) {
        self.jupyter_output = jupyter::discover()
            .map(|kernels| {
                kernels
                    .into_iter()
                    .map(|kernel| {
                        format!(
                            "{} ({}) [{}]\n  {}",
                            kernel.display_name,
                            kernel.name,
                            kernel.language,
                            kernel.resource_dir.display()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|e| e);
        self.inspector_tab = InspectorTab::Help;
        self.hover_text = format!("Jupyter kernels\n\n{}", self.jupyter_output);
    }

    fn import_dataset(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import dataset")
            .add_filter(
                "Datasets",
                &["csv", "tsv", "jsonl", "ndjson", "parquet", "arrow", "ipc"],
            )
            .pick_file()
        else {
            return;
        };
        match self.data.import(&path) {
            Ok(name) => {
                self.open_dataset = Some(format!("table:{name}"));
                self.dataset_filter.clear();
                self.inspector_tab = InspectorTab::Data;
                self.console = format!("Imported {} as `{name}`.", path.display());
            }
            Err(error) => self.console = format!("Could not import dataset: {error}"),
        }
    }

    #[cfg(feature = "millwright")]
    fn import_millwright_dataset(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import Millwright table")
            .add_filter("Millwright tables", &["csv", "parquet"])
            .pick_file()
        else {
            return;
        };
        match self.data.import_millwright(&path) {
            Ok(name) => {
                self.open_dataset = Some(format!("table:{name}"));
                self.inspector_tab = InspectorTab::Data;
                self.console = format!("Loaded `{name}` through published Millwright 2.2.1.");
            }
            Err(error) => self.console = format!("Millwright import failed: {error}"),
        }
    }

    fn project_root(&self) -> Option<PathBuf> {
        self.project.as_ref().map(|project| project.root.clone())
    }

    fn git_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(root) = self.project_root() else {
            ui.label("Open a Git project first.");
            return;
        };
        ui.horizontal_wrapped(|ui| {
            if ui.button("Refresh").clicked() {
                self.git_output = git::snapshot(&root)
                    .map(|s| format!("Branch: {}\n{}", s.branch, s.summary))
                    .unwrap_or_else(|e| e);
                if let Some(p) = &mut self.project {
                    p.refresh_git_status();
                }
            }
            if ui.button("Diff").clicked() {
                self.git_output = git::diff(&root, false).unwrap_or_else(|e| e);
            }
            if ui.button("Staged diff").clicked() {
                self.git_output = git::diff(&root, true).unwrap_or_else(|e| e);
            }
            if ui.button("Stage all").clicked() {
                self.git_output = git::stage_all(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Unstage all").clicked() {
                self.git_output = git::unstage_all(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Branches").clicked() {
                self.git_output = git::branches(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Pull").clicked() {
                self.git_output = git::pull(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Push").clicked() {
                self.git_output = git::push(&root).unwrap_or_else(|e| e);
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.git_commit_message)
                    .hint_text("Commit message"),
            );
            if ui.button("Commit").clicked() {
                self.git_output =
                    git::commit(&root, &self.git_commit_message).unwrap_or_else(|e| e);
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.git_branch_name).hint_text("Branch name"));
            if ui.button("Switch").clicked() {
                self.git_output =
                    git::switch(&root, &self.git_branch_name, false).unwrap_or_else(|e| e);
            }
            if ui.button("Create branch").clicked() {
                self.git_output =
                    git::switch(&root, &self.git_branch_name, true).unwrap_or_else(|e| e);
            }
        });
        ui.separator();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.code(if self.git_output.is_empty() {
                "Refresh to inspect repository status."
            } else {
                &self.git_output
            });
        });
    }

    fn packages_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(root) = self.project_root() else {
            ui.label("Open a Cargo project first.");
            return;
        };
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.package_query)
                    .hint_text("crate or crate@version"),
            );
            if ui.button("Search").clicked() {
                self.package_output =
                    packages::search(&root, &self.package_query).unwrap_or_else(|e| e);
            }
            if ui.button("Info").clicked() {
                self.package_output =
                    packages::info(&root, &self.package_query).unwrap_or_else(|e| e);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Add").clicked() {
                self.package_output =
                    packages::add(&root, &self.package_query).unwrap_or_else(|e| e);
            }
            if ui.button("Remove").clicked() {
                self.package_output =
                    packages::remove(&root, &self.package_query).unwrap_or_else(|e| e);
            }
            if ui.button("Update lockfile").clicked() {
                self.package_output = packages::update(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Dependency tree").clicked() {
                self.package_output = packages::tree(&root, false).unwrap_or_else(|e| e);
            }
            if ui.button("Duplicate versions").clicked() {
                self.package_output = packages::tree(&root, true).unwrap_or_else(|e| e);
            }
            if ui.button("Audit").clicked() {
                self.package_output = packages::audit(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Licenses/metadata").clicked() {
                self.package_output = packages::licenses(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Cargo package check").clicked() {
                self.package_output = publishing::cargo_package(&root).unwrap_or_else(|e| e);
            }
            if ui.button("crates.io dry run").clicked() {
                self.package_output =
                    publishing::cargo_publish_dry_run(&root).unwrap_or_else(|e| e);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("PyPI details").clicked() {
                if self.python_runtimes.is_empty() {
                    self.discover_python_runtimes();
                }
                self.package_output = self
                    .python_runtimes
                    .first()
                    .map(|runtime| {
                        python_runtime::pypi(runtime, &self.package_query).unwrap_or_else(|e| e)
                    })
                    .unwrap_or_else(|| {
                        "No Python runtime is available for secure PyPI HTTPS discovery.".into()
                    });
            }
            if ui.button("Environment tools").clicked() {
                self.package_output = python_runtime::managers().join("\n");
            }
            if ui.button("Python build preview").clicked() {
                self.package_output = self
                    .selected_python
                    .as_ref()
                    .map(|python| publishing::python_build(&root, python).unwrap_or_else(|e| e))
                    .unwrap_or_else(|| "Select a Python runtime first.".into());
            }
            if ui.button("Python smoke test").clicked() {
                self.package_output = self
                    .selected_python
                    .as_ref()
                    .map(|python| {
                        publishing::python_smoke_test(&root, python).unwrap_or_else(|e| e)
                    })
                    .unwrap_or_else(|| "Select a Python runtime first.".into());
            }
        });
        ui.separator();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.code(if self.package_output.is_empty() {
                "Search crates.io or inspect this project's dependencies."
            } else {
                &self.package_output
            });
        });
    }

    fn github_inspector(&mut self, ui: &mut egui::Ui) {
        let root = self.project_root();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.github_input).hint_text("owner/repo or title"),
            );
            if ui.button("Auth status").clicked() {
                self.github_output = github::auth_status().unwrap_or_else(|e| e);
            }
            if ui.button("Clone...").clicked() {
                if let Some(destination) = rfd::FileDialog::new().pick_folder() {
                    self.github_output =
                        github::clone(&self.github_input, &destination).unwrap_or_else(|e| e);
                }
            }
        });
        if let Some(root) = root {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Repository").clicked() {
                    self.github_output = github::repos(&root).unwrap_or_else(|e| e);
                }
                if ui.button("Fork").clicked() {
                    self.github_output = github::fork(&root).unwrap_or_else(|e| e);
                }
                if ui.button("Publish").clicked() {
                    self.github_output =
                        github::publish(&root, &self.github_input).unwrap_or_else(|e| e);
                }
                if ui.button("Pull requests").clicked() {
                    self.github_output = github::prs(&root).unwrap_or_else(|e| e);
                }
                if ui.button("Create PR").clicked() {
                    self.github_output =
                        github::create_pr(&root, &self.github_input).unwrap_or_else(|e| e);
                }
                if ui.button("Issues").clicked() {
                    self.github_output = github::issues(&root).unwrap_or_else(|e| e);
                }
                if ui.button("Create issue").clicked() {
                    self.github_output =
                        github::create_issue(&root, &self.github_input).unwrap_or_else(|e| e);
                }
                if ui.button("Actions").clicked() {
                    self.github_output = github::actions(&root).unwrap_or_else(|e| e);
                }
            });
        } else {
            ui.label("Open a project for repository, PR, issue, and Actions operations.");
        }
        ui.label(RichText::new("Authentication is delegated to GitHub CLI's secure credential store (`gh auth login`).").size(9.0).color(MUTED));
        ui.separator();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.code(if self.github_output.is_empty() {
                "Check authentication or open a repository."
            } else {
                &self.github_output
            });
        });
    }

    fn data_inspector(&mut self, ui: &mut egui::Ui) {
        if self.data.vectors.is_empty() && self.data.tables.is_empty() {
            ui.label(
                RichText::new(
                    "Emit `forge_vector:name=1,2,3` or `forge_table:name={...}` to inspect data.",
                )
                .monospace()
                .size(10.0)
                .color(MUTED),
            );
            return;
        }
        if ui
            .small_button("Clear datasets")
            .on_hover_text("Remove all live datasets and their vector plots")
            .clicked()
        {
            self.data.vectors.clear();
            self.data.tables.clear();
            self.open_dataset = None;
            self.console = "Cleared all live datasets.".to_owned();
            return;
        }
        let mut dataset_to_delete: Option<(bool, String)> = None;
        egui::ScrollArea::vertical()
            .id_salt("data_inspector_vectors")
            .show(ui, |ui| {
                let mut table_names = self.data.tables.keys().cloned().collect::<Vec<_>>();
                table_names.sort();
                for name in table_names {
                    let data = &self.data.tables[&name];
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new(&name).strong().color(CYAN))
                            .on_hover_text("Open in the data viewer")
                            .clicked()
                        {
                            self.open_dataset = Some(format!("table:{name}"));
                            self.dataset_filter.clear();
                        }
                        ui.label(
                            RichText::new(format!(
                                "{} rows × {} columns",
                                data.rows.len(),
                                data.columns.len()
                            ))
                            .size(10.0)
                            .color(MUTED),
                        );
                        if compact_icon_button(
                            ui,
                            egui_phosphor_icons::icons::TRASH,
                            "Delete this dataset",
                        )
                        .clicked()
                        {
                            dataset_to_delete = Some((true, name.clone()));
                        }
                    });
                    if let Some(source) = &data.source {
                        ui.label(
                            RichText::new(format!(
                                "Source: {source} · Arrow batch: {} rows",
                                data.batch.num_rows()
                            ))
                            .size(9.0)
                            .color(MUTED),
                        );
                    }
                    egui::Grid::new(format!("table_preview_{name}"))
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("#").strong().color(MUTED));
                            for column in &data.columns {
                                ui.label(RichText::new(column).strong());
                            }
                            ui.end_row();
                            for (index, row) in data.rows.iter().take(5).enumerate() {
                                ui.label(RichText::new(index.to_string()).monospace().color(MUTED));
                                for value in row {
                                    ui.label(RichText::new(value).monospace().size(10.0));
                                }
                                ui.end_row();
                            }
                        });
                    if data.rows.len() > 5 {
                        ui.label(
                            RichText::new("Open the dataset to view and filter all rows.")
                                .size(9.0)
                                .color(MUTED),
                        );
                    }
                    egui::CollapsingHeader::new("Column profile").show(ui, |ui| {
                        egui::Grid::new(format!("profile_{name}"))
                            .striped(true)
                            .show(ui, |ui| {
                                for label in ["Column", "Missing", "Unique", "Min", "Max", "Mean"] {
                                    ui.strong(label);
                                }
                                ui.end_row();
                                for profile in data.profile() {
                                    ui.label(profile.name);
                                    ui.label(profile.missing.to_string());
                                    ui.label(profile.unique.to_string());
                                    ui.label(
                                        profile
                                            .min
                                            .map(|v| format!("{v:.4}"))
                                            .unwrap_or_else(|| "—".into()),
                                    );
                                    ui.label(
                                        profile
                                            .max
                                            .map(|v| format!("{v:.4}"))
                                            .unwrap_or_else(|| "—".into()),
                                    );
                                    ui.label(
                                        profile
                                            .mean
                                            .map(|v| format!("{v:.4}"))
                                            .unwrap_or_else(|| "—".into()),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
                    ui.separator();
                }
                for (name, values) in &self.data.vectors {
                    let min = values.iter().copied().reduce(f64::min).unwrap_or(0.0);
                    let max = values.iter().copied().reduce(f64::max).unwrap_or(0.0);
                    let mean = values.iter().sum::<f64>() / values.len().max(1) as f64;
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new(name).strong().color(CYAN))
                            .on_hover_text("Open in the data viewer")
                            .clicked()
                        {
                            self.open_dataset = Some(format!("vector:{name}"));
                            self.dataset_filter.clear();
                        }
                        if compact_icon_button(
                            ui,
                            egui_phosphor_icons::icons::TRASH,
                            "Delete this dataset and its plot",
                        )
                        .clicked()
                        {
                            dataset_to_delete = Some((false, name.clone()));
                        }
                    });
                    egui::Grid::new(format!("data_summary_{name}"))
                        .num_columns(4)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Shape");
                            ui.label(format!("[{}]", values.len()));
                            ui.label("dtype");
                            ui.label("f64");
                            ui.end_row();
                            ui.label("Min");
                            ui.label(format!("{min:.5}"));
                            ui.label("Max");
                            ui.label(format!("{max:.5}"));
                            ui.end_row();
                            ui.label("Mean");
                            ui.label(format!("{mean:.5}"));
                            ui.label("Count");
                            ui.label(values.len().to_string());
                            ui.end_row();
                        });
                    ui.label(RichText::new("Values").size(10.0).color(MUTED));
                    egui::ScrollArea::horizontal()
                        .id_salt(format!("data_values_{name}"))
                        .show(ui, |ui| {
                            egui::Grid::new(format!("data_grid_{name}"))
                                .striped(true)
                                .show(ui, |ui| {
                                    for (index, _) in values.iter().enumerate().take(128) {
                                        ui.label(
                                            RichText::new(index.to_string()).monospace().size(9.0),
                                        );
                                    }
                                    ui.end_row();
                                    for value in values.iter().take(128) {
                                        ui.label(
                                            RichText::new(format!("{value:.5}"))
                                                .monospace()
                                                .size(10.0),
                                        );
                                    }
                                    ui.end_row();
                                });
                        });
                    if values.len() > 128 {
                        ui.label(
                            RichText::new(format!("Showing 128 of {} values", values.len()))
                                .size(9.0)
                                .color(MUTED),
                        );
                    }
                    ui.separator();
                }
            });
        if let Some((is_table, name)) = dataset_to_delete {
            if is_table {
                self.data.tables.remove(&name);
            } else {
                self.data.vectors.remove(&name);
            }
            self.console = format!("Deleted dataset `{name}`.");
        }
    }

    fn selected_dataset(&self) -> Option<(String, TableData)> {
        let selection = self.open_dataset.as_ref()?;
        let (kind, name) = selection.split_once(':').unwrap_or(("", selection));
        let data = match kind {
            "table" => self
                .data
                .tables
                .get(name)
                .map(|dataset| dataset.table.clone()),
            "vector" => self.data.vectors.get(name).map(|values| TableData {
                columns: vec!["value".to_owned()],
                rows: values.iter().map(|value| vec![value.to_string()]).collect(),
            }),
            _ => None,
        }?;
        Some((name.to_owned(), data))
    }

    fn docked_dataset_viewer(&mut self, ui: &mut egui::Ui) {
        let Some((name, data)) = self.selected_dataset() else {
            self.open_dataset = None;
            return;
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("DATA VIEWER")
                    .size(10.0)
                    .strong()
                    .color(MUTED),
            );
            if ui.small_button("Close").clicked() {
                self.open_dataset = None;
                self.dataset_filter.clear();
            }
            ui.label(RichText::new(&name).strong().color(CYAN));
            if ui
                .small_button("Undock")
                .on_hover_text("Move this viewer to a floating window")
                .clicked()
            {
                self.dataset_viewer_docked = false;
            }
        });
        draw_dataset_table(
            ui,
            &data,
            &mut self.dataset_filter,
            &mut self.dataset_sort_column,
            &mut self.dataset_sort_descending,
            "docked",
        );
    }

    fn dataset_window(&mut self, ctx: &egui::Context) {
        if self.dataset_viewer_docked {
            return;
        }
        let Some((name, data)) = self.selected_dataset() else {
            self.open_dataset = None;
            return;
        };
        let mut open = true;
        let mut dock = false;
        egui::Window::new(format!("Data viewer — {name}"))
            .id(egui::Id::new("forge_dataset_viewer"))
            .open(&mut open)
            .default_size([760.0, 520.0])
            .min_size([420.0, 260.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .small_button("Dock right")
                        .on_hover_text("Return this viewer to the Data panel")
                        .clicked()
                    {
                        dock = true;
                    }
                });
                draw_dataset_table(
                    ui,
                    &data,
                    &mut self.dataset_filter,
                    &mut self.dataset_sort_column,
                    &mut self.dataset_sort_descending,
                    "floating",
                );
            });
        if dock {
            self.dataset_viewer_docked = true;
        }
        if !open {
            self.open_dataset = None;
            self.dataset_filter.clear();
        }
    }

    fn project_search(&mut self, ui: &mut egui::Ui) {
        let mut run_search = false;
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.project_search_query)
                    .desired_width(180.0)
                    .hint_text("Search project..."),
            );
            run_search = ui.button("Find").clicked()
                || (response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.project_search_case_sensitive, "Match case");
            if ui.button("Clear").clicked() {
                self.project_search_results.clear();
            }
        });
        if run_search {
            self.run_project_search();
        }
        ui.separator();
        ui.label(
            RichText::new(format!("{} result(s)", self.project_search_results.len()))
                .size(10.0)
                .color(MUTED),
        );
        let root = self.project.as_ref().map(|project| project.root.clone());
        let mut navigate = None;
        egui::ScrollArea::vertical()
            .id_salt("project_search_results")
            .show(ui, |ui| {
                for result in &self.project_search_results {
                    let shown_path = root
                        .as_ref()
                        .and_then(|root| result.path.strip_prefix(root).ok())
                        .unwrap_or(&result.path);
                    let label = format!(
                        "{}:{}:{}\n  {}",
                        shown_path.display(),
                        result.line + 1,
                        result.column + 1,
                        result.preview
                    );
                    if ui
                        .add(
                            egui::Button::new(RichText::new(label).monospace().size(10.0))
                                .frame(false)
                                .wrap(),
                        )
                        .on_hover_text("Open search result")
                        .clicked()
                    {
                        navigate = Some((result.path.clone(), result.line, result.column));
                    }
                    ui.separator();
                }
            });
        if let Some((path, line, column)) = navigate {
            self.navigate_to(path, line, column);
        }
    }

    fn experiments(&mut self, ui: &mut egui::Ui) {
        if self.saved_runs.is_empty() {
            ui.label(
                RichText::new("Save a snapshot from Plots to compare training runs.").color(MUTED),
            );
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Metric");
            egui::ComboBox::from_id_salt("comparison_metric")
                .selected_text(&self.comparison_metric)
                .show_ui(ui, |ui| {
                    let mut names = self
                        .saved_runs
                        .iter()
                        .flat_map(|run| run.metrics.keys().cloned())
                        .collect::<Vec<_>>();
                    names.sort();
                    names.dedup();
                    for name in names {
                        ui.selectable_value(&mut self.comparison_metric, name.clone(), name);
                    }
                });
            if ui.button("Export CSV").clicked() {
                self.export_telemetry_csv();
            }
        });
        let colors = [CYAN, EMBER, GREEN, RED, Color32::from_rgb(150, 105, 210)];
        Plot::new("experiment_comparison")
            .height(220.0)
            .allow_drag(false)
            .show(ui, |plot| {
                for (index, run) in self.saved_runs.iter().enumerate() {
                    if let Some(values) = run.metrics.get(&self.comparison_metric) {
                        let points: PlotPoints = values.clone().into();
                        plot.line(
                            Line::new(run.name.clone(), points)
                                .color(colors[index % colors.len()])
                                .width(2.0),
                        );
                    }
                }
            });
        let mut run_to_delete = None;
        let mut run_to_clone = None;
        let mut run_to_toggle_archive = None;
        egui::ScrollArea::vertical()
            .id_salt("experiment_run_table")
            .show(ui, |ui| {
                egui::Grid::new("experiment_table")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Run").strong());
                        ui.label(RichText::new("Final").strong());
                        ui.label(RichText::new("Steps").strong());
                        ui.label(RichText::new("Execs").strong());
                        ui.label(RichText::new("Status").strong());
                        ui.label("");
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                        for (index, run) in self.saved_runs.iter().enumerate() {
                            let values = run.metrics.get(&self.comparison_metric);
                            let final_value = values
                                .and_then(|values| values.last())
                                .map(|point| format!("{:.6}", point[1]))
                                .unwrap_or_else(|| "-".to_owned());
                            ui.label(&run.name);
                            ui.label(final_value);
                            ui.label(values.map_or(0, Vec::len).to_string());
                            ui.label(run.execution_count.to_string());
                            ui.label(if run.archived { "Archived" } else { "Active" });
                            if ui.small_button("Clone").clicked() {
                                run_to_clone = Some(index);
                            }
                            if ui
                                .small_button(if run.archived { "Restore" } else { "Archive" })
                                .clicked()
                            {
                                run_to_toggle_archive = Some(index);
                            }
                            if compact_icon_button(
                                ui,
                                egui_phosphor_icons::icons::TRASH,
                                "Delete this saved run",
                            )
                            .clicked()
                            {
                                run_to_delete = Some(index);
                            }
                            ui.end_row();
                            ui.label("");
                            ui.label(
                                RichText::new(format!("tags: {}", run.tags.join(", ")))
                                    .size(9.0)
                                    .color(MUTED),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "git: {}{}",
                                    run.provenance
                                        .git_commit
                                        .chars()
                                        .take(10)
                                        .collect::<String>(),
                                    if run.provenance.git_dirty {
                                        " dirty"
                                    } else {
                                        ""
                                    }
                                ))
                                .size(9.0)
                                .color(MUTED),
                            );
                            ui.label(
                                RichText::new(format!("artifacts: {}", run.artifacts.len()))
                                    .size(9.0)
                                    .color(MUTED),
                            );
                            ui.label(RichText::new(run.notes.clone()).size(9.0).color(MUTED));
                            ui.end_row();
                        }
                    });
            });
        if let Some(index) = run_to_clone {
            let child = self.saved_runs[index]
                .clone_as_child(format!("{}_clone", self.saved_runs[index].name));
            if let Some(store) = &self.workspace_store {
                let _ = store.save_experiment(&child.id, &child.name, &child);
            }
            self.saved_runs.push(child);
        }
        if let Some(index) = run_to_toggle_archive {
            self.saved_runs[index].archived = !self.saved_runs[index].archived;
            if let Some(store) = &self.workspace_store {
                let run = &self.saved_runs[index];
                let _ = store.save_experiment(&run.id, &run.name, run);
            }
        }
        if let Some(index) = run_to_delete {
            let run = self.saved_runs.remove(index);
            if let Some(store) = &self.workspace_store {
                if let Err(error) = store.delete_experiment(&run.id) {
                    self.console =
                        format!("Deleted run from the view, but storage failed: {error}");
                    return;
                }
            }
            self.console = format!("Deleted saved run `{}`.", run.name);
        }
    }

    fn charts(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_name)
                    .desired_width(110.0)
                    .hint_text("Run name"),
            );
            if ui.button("Save run").clicked() {
                self.save_experiment_run();
            }
            if ui.button("Export").clicked() {
                self.export_telemetry_csv();
            }
            if self.data.has_telemetry()
                && ui
                    .button("Clear current")
                    .on_hover_text("Clear all current datasets and plots")
                    .clicked()
            {
                self.data.metrics.clear();
                self.data.vectors.clear();
                self.console = "Cleared current datasets and plots.".to_owned();
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_tags)
                    .hint_text("tags, comma separated"),
            );
            ui.add(egui::TextEdit::singleline(&mut self.experiment_notes).hint_text("run notes"));
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_github_issue)
                    .hint_text("GitHub issue URL"),
            );
            ui.add(egui::TextEdit::singleline(&mut self.experiment_github_pr).hint_text("PR URL"));
            ui.add(
                egui::TextEdit::singleline(&mut self.experiment_github_action)
                    .hint_text("Actions run URL"),
            );
        });
        if !self.data.has_telemetry() {
            ui.label(
                RichText::new(
                    "Emit `forge_metric:loss=0.42` or `forge_vector:weights=1,2,3` from a cell.",
                )
                .monospace()
                .size(10.0)
                .color(MUTED),
            );
        }
        let mut metric_to_delete = None;
        for (name, values) in &self.data.metrics {
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).strong().color(EMBER));
                if compact_icon_button(
                    ui,
                    egui_phosphor_icons::icons::TRASH,
                    "Delete this metric plot",
                )
                .clicked()
                {
                    metric_to_delete = Some(name.clone());
                }
            });
            Plot::new(format!("metric_{name}"))
                .height(175.0)
                .allow_drag(false)
                .show(ui, |plot| plot.line(metric_line(name, values, EMBER)));
        }
        if let Some(name) = metric_to_delete {
            self.data.metrics.remove(&name);
            self.console = format!("Deleted plot `{name}`.");
        }
        let mut vector_to_delete = None;
        for (name, values) in &self.data.vectors {
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).strong().color(CYAN));
                if compact_icon_button(
                    ui,
                    egui_phosphor_icons::icons::TRASH,
                    "Delete this dataset and its plot",
                )
                .clicked()
                {
                    vector_to_delete = Some(name.clone());
                }
            });
            Plot::new(format!("vector_{name}"))
                .height(175.0)
                .allow_drag(false)
                .show(ui, |plot| plot.bar_chart(vector_bars(name, values, CYAN)));
        }
        if let Some(name) = vector_to_delete {
            self.data.vectors.remove(&name);
            self.console = format!("Deleted dataset and plot `{name}`.");
        }
    }

    fn console(&mut self, ui: &mut egui::Ui) {
        ui.set_min_height(ui.available_height());
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.console_tab == ConsoleTab::Console, "Rust console")
                .clicked()
            {
                self.console_tab = ConsoleTab::Console;
            }
            if ui
                .selectable_label(self.console_tab == ConsoleTab::History, "History log")
                .clicked()
            {
                self.console_tab = ConsoleTab::History;
            }
            if ui
                .selectable_label(self.console_tab == ConsoleTab::Python, "Python runtime")
                .clicked()
            {
                self.console_tab = ConsoleTab::Python;
            }
            if let Some(ms) = self
                .cell_records
                .get(&self.selected_cell)
                .and_then(|r| r.elapsed_ms)
            {
                ui.label(RichText::new(format!("{ms} ms")).size(10.0).color(CYAN));
            }
            if compact_icon_button(
                ui,
                egui_phosphor_icons::icons::BROOM,
                match self.console_tab {
                    ConsoleTab::Console => "Clear the visible console output",
                    ConsoleTab::History => "Clear the command history",
                    ConsoleTab::Python => "Clear Python runtime output",
                },
            )
            .clicked()
            {
                match self.console_tab {
                    ConsoleTab::Console => {
                        // Cell runs keep their output separately from the shared console.
                        // Clear both stores so the selected cell's errors do not reappear.
                        self.console.clear();
                        if let Some(record) = self.cell_records.get_mut(&self.selected_cell) {
                            record.output.clear();
                            record.elapsed_ms = None;
                        }
                    }
                    ConsoleTab::History => self.history.clear(),
                    ConsoleTab::Python => {
                        self.python_console_output.clear();
                        self.python_mime_outputs.clear();
                    }
                }
            }
        });
        ui.separator();
        match self.console_tab {
            ConsoleTab::Console => {
                let shown = self
                    .cell_records
                    .get(&self.selected_cell)
                    .filter(|r| !r.output.is_empty())
                    .map(|r| r.output.as_str())
                    .unwrap_or(&self.console);
                egui::ScrollArea::vertical()
                    .id_salt("rust_console_output")
                    .max_height((ui.available_height() - 34.0).max(30.0))
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if let Some(record) = self.cell_records.get(&self.selected_cell) {
                            if let Some(commit) = &record.git_commit {
                                ui.label(
                                    RichText::new(format!(
                                        "Git: {commit}{} · {} MIME output(s)",
                                        if record.git_dirty { " + dirty" } else { "" },
                                        record.rich_outputs.len()
                                    ))
                                    .size(9.0)
                                    .color(MUTED),
                                );
                            }
                        }
                        ui.label(RichText::new(shown).monospace().color(
                            if self.run_state == RunState::Failed {
                                RED
                            } else {
                                TEXT
                            },
                        ));
                    });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("In [ ]:").monospace().strong().color(CYAN));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.console_input)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .hint_text("Enter Rust expression..."),
                    );
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.run_console_input();
                    }
                    if ui.button("Run").clicked() {
                        self.run_console_input();
                    }
                });
            }
            ConsoleTab::History => {
                egui::ScrollArea::vertical()
                    .id_salt("rust_console_history")
                    .show(ui, |ui| {
                        for (index, command) in self.history.iter().enumerate() {
                            ui.label(
                                RichText::new(format!("In [{}]: {}", index + 1, command))
                                    .monospace()
                                    .color(TEXT),
                            );
                        }
                    });
            }
            ConsoleTab::Python => {
                ui.horizontal(|ui| {
                    if ui.button("Discover").clicked() {
                        self.discover_python_runtimes();
                    }
                    egui::ComboBox::from_id_salt("python_runtime_selector")
                        .selected_text(
                            self.selected_python
                                .as_ref()
                                .map(|path| path.display().to_string())
                                .unwrap_or_else(|| "No interpreter selected".into()),
                        )
                        .show_ui(ui, |ui| {
                            for runtime in &self.python_runtimes {
                                ui.selectable_value(
                                    &mut self.selected_python,
                                    Some(runtime.executable.clone()),
                                    format!(
                                        "{} · {}",
                                        runtime.version,
                                        runtime.executable.display()
                                    ),
                                );
                            }
                        });
                    if ui.button("Start/restart").clicked() {
                        self.start_python_kernel();
                    }
                    if ui.button("Create .venv").clicked() {
                        if let (Some(root), Some(runtime)) = (
                            self.project_root(),
                            self.python_runtimes.iter().find(|runtime| {
                                Some(&runtime.executable) == self.selected_python.as_ref()
                            }),
                        ) {
                            self.python_console_output =
                                python_runtime::create_venv(runtime, &root.join(".venv"))
                                    .unwrap_or_else(|e| e);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Jupyter kernels").clicked() {
                        self.jupyter_kernels = jupyter::discover().unwrap_or_default();
                    }
                    egui::ComboBox::from_id_salt("python_jupyter_kernel")
                        .selected_text(if self.selected_jupyter_kernel.is_empty() {
                            "No Jupyter kernel selected"
                        } else {
                            &self.selected_jupyter_kernel
                        })
                        .show_ui(ui, |ui| {
                            for kernel in &self.jupyter_kernels {
                                if kernel.language.eq_ignore_ascii_case("python") {
                                    ui.selectable_value(
                                        &mut self.selected_jupyter_kernel,
                                        kernel.name.clone(),
                                        &kernel.display_name,
                                    );
                                }
                            }
                        });
                });
                egui::ScrollArea::vertical()
                    .max_height((ui.available_height() - 62.0).max(40.0))
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new(&self.python_console_output).monospace());
                        for output in &self.python_mime_outputs {
                            ui.label(
                                RichText::new(format!("{}: {}", output.mime, output.data))
                                    .monospace()
                                    .color(CYAN),
                            );
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Py [ ]:").monospace().strong().color(CYAN));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.python_console_input)
                            .desired_width(f32::INFINITY)
                            .hint_text("Python code (persistent session)"),
                    );
                    if (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        || ui.button("Run").clicked()
                    {
                        self.run_python_input();
                    }
                });
            }
        }
        ui.take_available_space();
    }
}

impl eframe::App for ForgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_background(ui.ctx());
        let save = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S));
        let new_file = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N));
        let find = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F));
        let find_in_files =
            ui.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::F));
        let complete = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Space));
        let run = ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::Enter));
        let run_all = ui
            .input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Enter));
        if save {
            self.save_active();
        }
        if new_file {
            self.create_new_file(None);
        }
        if find_in_files {
            self.inspector_tab = InspectorTab::Search;
        } else if find {
            self.find_visible = true;
        }
        if complete {
            ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Space);
            });
            self.request_lsp("complete");
            self.lsp_status = "Requesting completions...".to_owned();
        }
        if self.completion_popup_open && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.completion_popup_open = false;
        }
        if run_all {
            self.enqueue_cells(0..self.cells().len());
        } else if run {
            self.enqueue_cells([self.selected_cell]);
        }
        Panel::top("menu_bar")
            .resizable(false)
            .default_size(28.0)
            .frame(compact_panel_frame(
                theme_colors(self.dark_mode).menu,
                self.dark_mode,
            ))
            .show(ui, |ui| self.menu_bar(ui));
        Panel::top("command_bar")
            .resizable(false)
            .default_size(42.0)
            .frame(compact_panel_frame(
                theme_colors(self.dark_mode).surface,
                self.dark_mode,
            ))
            .show(ui, |ui| self.top_bar(ui));
        Panel::left("workspace")
            .default_size(240.0)
            .frame(panel_frame(
                theme_colors(self.dark_mode).surface,
                self.dark_mode,
            ))
            .show(ui, |ui| self.cell_navigator(ui));
        Panel::right("inspector")
            .default_size(320.0)
            .frame(panel_frame(
                theme_colors(self.dark_mode).surface,
                self.dark_mode,
            ))
            .show(ui, |ui| self.right_sidebar(ui));
        Panel::bottom("console")
            .resizable(true)
            .show_separator_line(false)
            .default_size(190.0)
            .frame(console_panel_frame(self.dark_mode))
            .show(ui, |ui| self.console(ui));
        let mut ctrl_clicked_definition = false;
        let mut definition_probe = None;
        egui::CentralPanel::default()
            .frame(panel_frame(
                theme_colors(self.dark_mode).background,
                self.dark_mode,
            ))
            .show(ui, |ui| {
                self.editor_tabs(ui);
                self.external_change_banner(ui);
                self.apply_pending_editor_history(ui);
                if self.find_visible {
                    let mut next = false;
                    let mut replace = false;
                    let mut replace_all = false;
                    ui.horizontal(|ui| {
                        ui.label("Find");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.find_query).desired_width(150.0),
                        );
                        ui.label("Replace");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.replace_query)
                                .desired_width(150.0),
                        );
                        next = ui.button("Next").clicked()
                            || (response.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter)));
                        replace = ui.button("Replace").clicked();
                        replace_all = ui.button("All").clicked();
                        if compact_icon_button(
                            ui,
                            egui_phosphor_icons::icons::X,
                            "Close find and replace",
                        )
                        .clicked()
                        {
                            self.find_visible = false;
                        }
                    });
                    if next {
                        self.find_next();
                    }
                    if replace {
                        self.replace_current();
                    }
                    if replace_all {
                        self.replace_all();
                    }
                }
                ui.add_space(5.0);
                let output = CodeEditor::default()
                    .id_source(format!("editor_{}", self.active_tab))
                    .with_rows(32)
                    .with_fontsize(self.editor_font_size)
                    .with_theme(if self.dark_mode {
                        ColorTheme::GITHUB_DARK
                    } else {
                        ColorTheme::GITHUB_LIGHT
                    })
                    .with_numlines(true)
                    .show(ui, &mut self.tabs[self.active_tab].content, &Syntax::rust());
                if self.editor_needs_initial_focus {
                    output.response.request_focus();
                    self.editor_needs_initial_focus = false;
                    ui.ctx().request_repaint();
                }
                if output.response.changed() {
                    self.tabs[self.active_tab].dirty = true;
                    self.cell_records.clear();
                }
                if let Some(diagnostics) = self
                    .active()
                    .path
                    .as_ref()
                    .and_then(|path| self.lsp_diagnostics.get(path))
                {
                    paint_inline_diagnostics(
                        ui,
                        &output,
                        &self.tabs[self.active_tab].content,
                        diagnostics,
                    );
                }
                if let Some(range) = output.cursor_range {
                    self.cursor_offset = range.primary.index.0;
                    self.select_cell_from_caret();
                    if output.response.has_focus() {
                        paint_editor_caret(
                            ui,
                            &output,
                            range.primary,
                            self.dark_mode,
                            self.caret_blink,
                        );
                    }
                }
                let ctrl_held = ui.input(|input| input.modifiers.ctrl);
                let hovered_offset = if output.response.hovered() {
                    ui.ctx().pointer_hover_pos().and_then(|pointer| {
                        let raw_offset = output
                            .galley
                            .cursor_from_pos(pointer - output.galley_pos)
                            .index
                            .0;
                        word_start_at(&self.tabs[self.active_tab].content, raw_offset)
                    })
                } else {
                    None
                };
                if hovered_offset != self.hover_probe_offset {
                    self.hover_probe_offset = hovered_offset;
                    self.navigable_hover_offset = None;
                    definition_probe = hovered_offset;
                }
                if let Some(offset) = hovered_offset {
                    if self.navigable_hover_offset == Some(offset) {
                        if ctrl_held {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        paint_navigable_word(
                            ui,
                            &output,
                            &self.tabs[self.active_tab].content,
                            offset,
                        );
                        if ctrl_held && output.response.clicked_by(egui::PointerButton::Primary) {
                            self.cursor_offset = offset;
                            ctrl_clicked_definition = true;
                        }
                    }
                }
                if self.completion_popup_open {
                    if let Some(range) = output.cursor_range {
                        let caret = output.galley.pos_from_cursor(range.primary);
                        let popup_position =
                            output.galley_pos + egui::vec2(caret.min.x, caret.max.y + 3.0);
                        let completions = self
                            .completions
                            .iter()
                            .take(12)
                            .cloned()
                            .collect::<Vec<_>>();
                        let mut selected = None;
                        let popup = egui::Area::new(egui::Id::new("editor_completion_popup"))
                            .order(egui::Order::Foreground)
                            .fixed_pos(popup_position)
                            .show(ui.ctx(), |ui| {
                                egui::Frame::popup(ui.style()).show(ui, |ui| {
                                    ui.set_min_width(240.0);
                                    ui.label(
                                        RichText::new("RUST-ANALYZER COMPLETIONS")
                                            .size(9.0)
                                            .strong()
                                            .color(MUTED),
                                    );
                                    egui::ScrollArea::vertical()
                                        .id_salt("editor_completion_popup_list")
                                        .max_height(240.0)
                                        .show(ui, |ui| {
                                            for item in completions {
                                                if ui
                                                    .selectable_label(
                                                        false,
                                                        RichText::new(&item).monospace().size(11.0),
                                                    )
                                                    .clicked()
                                                {
                                                    selected = Some(item);
                                                }
                                            }
                                        });
                                });
                            });
                        if let Some(completion) = selected {
                            self.apply_completion(&completion);
                        } else if ui.input(|input| input.pointer.any_pressed())
                            && !popup.response.contains_pointer()
                            && !output.response.contains_pointer()
                        {
                            self.completion_popup_open = false;
                        }
                    }
                }
                if let Some((start, end)) = self.pending_editor_selection.take() {
                    let mut state = output.state.clone();
                    let target_cursor = egui::text::CCursor::new(start);
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            target_cursor,
                            egui::text::CCursor::new(end),
                        )));
                    state.store(ui.ctx(), output.response.id);
                    let caret = output.galley.pos_from_cursor(target_cursor);
                    let scroll_id =
                        ui.make_persistent_id(format!("editor_{}_outer_scroll", self.active_tab));
                    if let Some(mut scroll_state) =
                        egui::scroll_area::State::load(ui.ctx(), scroll_id)
                    {
                        let viewport_height = ui.available_height().max(120.0);
                        scroll_state.offset.y =
                            (caret.center().y - viewport_height * 0.45).max(0.0);
                        scroll_state.store(ui.ctx(), scroll_id);
                    }
                    output.response.request_focus();
                    ui.ctx().request_repaint();
                }
                let (line, column) =
                    line_column(&self.tabs[self.active_tab].content, self.cursor_offset);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("Ln {line}, Col {column}"))
                            .monospace()
                            .size(10.0)
                            .color(MUTED),
                    );
                });
            });
        self.sync_lsp();
        if let Some(offset) = definition_probe {
            self.definition_probe_pending = true;
            self.probe_definition(offset);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(40));
        }
        if ctrl_clicked_definition {
            self.request_lsp("definition");
            self.lsp_status = "Looking up definition...".to_owned();
        }
        self.delete_confirmation(ui.ctx());
        self.unsaved_confirmation(ui.ctx());
        self.settings_window(ui.ctx());
        self.dataset_window(ui.ctx());
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let recovery = WorkspaceRecovery {
            open_files: self
                .tabs
                .iter()
                .filter_map(|tab| tab.path.clone())
                .collect(),
            active_file: self.active().path.clone(),
            explorer_height: Some(self.explorer_height),
            dataset_pane_height: Some(self.dataset_pane_height),
            dataset_viewer_docked: Some(self.dataset_viewer_docked),
        };
        if let Some(store) = &self.workspace_store {
            if let Err(error) = store.save_recovery(&recovery) {
                self.console = format!("Could not save workspace recovery state: {error}");
            }
        }
        let state = SessionState {
            project_root: self.project.as_ref().map(|p| p.root.clone()),
            open_files: self
                .tabs
                .iter()
                .filter_map(|tab| tab.path.clone())
                .collect(),
            active_file: self.active().path.clone(),
            dark_mode: self.dark_mode,
            explorer_height: self.explorer_height,
            recent_projects: self.recent_projects.clone(),
            editor_font_size: self.editor_font_size,
            caret_blink: self.caret_blink,
            saved_runs: self.saved_runs.clone(),
            experiment_name: self.experiment_name.clone(),
            comparison_metric: self.comparison_metric.clone(),
            dataset_viewer_docked: self.dataset_viewer_docked,
            dataset_pane_height: self.dataset_pane_height,
            selected_python: self.selected_python.clone(),
            selected_jupyter_kernel: self.selected_jupyter_kernel.clone(),
            python_environment_fingerprint: self.python_environment_fingerprint.clone(),
        };
        eframe::set_value(storage, STORAGE_KEY, &state);
    }
}

fn draw_dataset_table(
    ui: &mut egui::Ui,
    data: &TableData,
    filter: &mut String,
    sort_column: &mut Option<usize>,
    sort_descending: &mut bool,
    id_salt: &str,
) {
    ui.horizontal(|ui| {
        ui.label(format!(
            "{} rows × {} columns",
            data.rows.len(),
            data.columns.len()
        ));
        ui.separator();
        ui.label("Filter");
        ui.add(
            egui::TextEdit::singleline(filter)
                .desired_width(180.0)
                .hint_text("Search values..."),
        );
        if ui.small_button("Clear").clicked() {
            filter.clear();
        }
    });
    ui.separator();
    let needle = filter.to_lowercase();
    let mut matching_rows = data
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            needle.is_empty()
                || row
                    .iter()
                    .any(|value| value.to_lowercase().contains(&needle))
        })
        .collect::<Vec<_>>();
    if let Some(column) = *sort_column {
        matching_rows.sort_by(|(_, left), (_, right)| {
            let left = left.get(column).map(String::as_str).unwrap_or_default();
            let right = right.get(column).map(String::as_str).unwrap_or_default();
            let ordering = match (left.parse::<f64>(), right.parse::<f64>()) {
                (Ok(left), Ok(right)) => left.total_cmp(&right),
                _ => left.to_lowercase().cmp(&right.to_lowercase()),
            };
            if *sort_descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }
    egui::ScrollArea::both()
        .id_salt(("dataset_table_scroll", id_salt))
        .auto_shrink([false, false])
        .show_rows(ui, 22.0, matching_rows.len() + 1, |ui, range| {
            egui::Grid::new(("dataset_viewer_grid", id_salt))
                .striped(true)
                .min_col_width(90.0)
                .show(ui, |ui| {
                    if range.start == 0 {
                        ui.label(RichText::new("#").strong().color(MUTED));
                        for (index, column) in data.columns.iter().enumerate() {
                            let arrow = if *sort_column == Some(index) {
                                if *sort_descending {
                                    " ↓"
                                } else {
                                    " ↑"
                                }
                            } else {
                                ""
                            };
                            if ui
                                .button(
                                    RichText::new(format!("{column}{arrow}"))
                                        .strong()
                                        .color(CYAN),
                                )
                                .clicked()
                            {
                                if *sort_column == Some(index) {
                                    *sort_descending = !*sort_descending;
                                } else {
                                    *sort_column = Some(index);
                                    *sort_descending = false;
                                }
                            }
                        }
                        ui.end_row();
                    }
                    for (index, row) in matching_rows
                        .iter()
                        .skip(range.start.saturating_sub(1))
                        .take(range.len())
                    {
                        ui.label(
                            RichText::new(index.to_string())
                                .monospace()
                                .size(10.0)
                                .color(MUTED),
                        );
                        for value in *row {
                            ui.label(RichText::new(value).monospace().size(10.0));
                        }
                        ui.end_row();
                    }
                });
        });
    ui.label(
        RichText::new(format!(
            "{} matching rows · virtualized rendering",
            matching_rows.len()
        ))
        .size(9.0)
        .color(MUTED),
    );
}

fn word_start_at(text: &str, offset: usize) -> Option<usize> {
    let chars = text.chars().collect::<Vec<_>>();
    let is_word = |character: char| character.is_alphanumeric() || character == '_';
    let mut index = offset.min(chars.len());
    if index == chars.len()
        || !chars
            .get(index)
            .is_some_and(|character| is_word(*character))
    {
        if index == 0 || !is_word(chars[index - 1]) {
            return None;
        }
        index -= 1;
    }
    while index > 0 && is_word(chars[index - 1]) {
        index -= 1;
    }
    Some(index)
}

fn char_to_byte(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

fn line_column(text: &str, char_offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for character in text.chars().take(char_offset) {
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn csv_field(value: &str) -> String {
    if value
        .chars()
        .any(|character| matches!(character, ',' | '"' | '\n' | '\r'))
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn paint_editor_caret(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    cursor: egui::text::CCursor,
    dark: bool,
    blink: bool,
) {
    if blink {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
    let bright_phase = !blink || ui.input(|input| input.time) % 1.0 < 0.65;
    let local = output.galley.pos_from_cursor(cursor);
    let x = output.galley_pos.x + local.min.x;
    let top = output.galley_pos.y + local.min.y - 1.0;
    let bottom = output.galley_pos.y + local.max.y + 1.0;
    let segment = [egui::pos2(x, top), egui::pos2(x, bottom)];
    let outline = if dark {
        Color32::from_rgb(3, 7, 12)
    } else {
        Color32::WHITE
    };
    let caret = if dark && bright_phase {
        Color32::from_rgb(118, 224, 255)
    } else if dark {
        Color32::from_rgb(59, 129, 153)
    } else if bright_phase {
        Color32::from_rgb(0, 45, 84)
    } else {
        Color32::from_rgb(60, 105, 135)
    };
    let outline_width = if bright_phase { 5.0 } else { 3.0 };
    let caret_width = if bright_phase { 3.0 } else { 1.5 };
    ui.painter()
        .line_segment(segment, Stroke::new(outline_width, outline));
    ui.painter()
        .line_segment(segment, Stroke::new(caret_width, caret));
}

fn paint_navigable_word(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    text: &str,
    offset: usize,
) {
    let chars = text.chars().collect::<Vec<_>>();
    let is_word = |character: char| character.is_alphanumeric() || character == '_';
    let mut start = offset.min(chars.len());
    let mut end = start;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    if start == end {
        return;
    }
    let start_rect = output
        .galley
        .pos_from_cursor(egui::text::CCursor::new(start));
    let end_rect = output.galley.pos_from_cursor(egui::text::CCursor::new(end));
    if start_rect.min.y == end_rect.min.y {
        let y = start_rect.max.y - 1.0;
        ui.painter().line_segment(
            [
                output.galley_pos + egui::vec2(start_rect.min.x, y),
                output.galley_pos + egui::vec2(end_rect.min.x, y),
            ],
            Stroke::new(1.5, CYAN),
        );
    }
}

fn paint_inline_diagnostics(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    text: &str,
    diagnostics: &[LspDiagnostic],
) {
    let chars = text.chars().collect::<Vec<_>>();
    let painter = ui.painter().with_clip_rect(output.text_clip_rect);
    for diagnostic in diagnostics {
        let line_start = text
            .split_inclusive('\n')
            .take(diagnostic.line as usize)
            .map(str::chars)
            .map(Iterator::count)
            .sum::<usize>();
        let mut start = (line_start + diagnostic.column as usize).min(chars.len());
        while start < chars.len() && chars[start].is_whitespace() && chars[start] != '\n' {
            start += 1;
        }
        let mut end = start;
        while end < chars.len()
            && (chars[end].is_alphanumeric() || chars[end] == '_' || chars[end] == ':')
        {
            end += 1;
        }
        if end == start {
            end = (start + 1).min(chars.len());
        }
        let start_rect = output
            .galley
            .pos_from_cursor(egui::text::CCursor::new(start));
        let end_rect = output.galley.pos_from_cursor(egui::text::CCursor::new(end));
        if start_rect.min.y != end_rect.min.y {
            continue;
        }
        let left = output.galley_pos.x + start_rect.min.x;
        let right = (output.galley_pos.x + end_rect.min.x).max(left + 5.0);
        let baseline = output.galley_pos.y + start_rect.max.y - 1.0;
        let color = match diagnostic.severity {
            1 => RED,
            2 => EMBER,
            _ => CYAN,
        };
        let mut points = Vec::new();
        let mut x = left;
        let mut high = true;
        while x <= right {
            points.push(egui::pos2(x, baseline + if high { -1.5 } else { 1.0 }));
            high = !high;
            x += 3.0;
        }
        points.push(egui::pos2(right, baseline));
        painter.add(egui::Shape::line(points, Stroke::new(1.4, color)));
        let hover_rect = egui::Rect::from_min_max(
            egui::pos2(left, output.galley_pos.y + start_rect.min.y),
            egui::pos2(right, output.galley_pos.y + start_rect.max.y + 3.0),
        );
        if ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|pointer| hover_rect.contains(pointer))
        {
            output
                .response
                .response
                .clone()
                .on_hover_text_at_pointer(&diagnostic.message);
        }
    }
}

fn welcome_tab() -> EditorTab {
    EditorTab {
        path: None,
        title: "experiment.rs".to_owned(),
        dirty: false,
        disk_hash: None,
        external_change_pending: false,
        content: r#"//# %% setup
let learning_rate = 0.03_f32;
let epochs = 12;

//# %% dataset
let samples = vec![0.2_f32, 0.7, 1.1, 1.8, 2.4];
println!("forge_vector:samples=0.2,0.7,1.1,1.8,2.4");

//# %% training
for epoch in 0..epochs {
    let loss = (-0.35 * epoch as f64).exp();
    println!("forge_metric:loss={}", loss);
}
"training complete""#
            .to_owned(),
    }
}

fn blank_tab() -> EditorTab {
    EditorTab {
        path: None,
        title: "Untitled.rs".to_owned(),
        content: String::new(),
        dirty: false,
        disk_hash: None,
        external_change_pending: false,
    }
}

fn content_hash(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

fn file_title(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("untitled")
        .to_owned()
}

fn infer_size(type_name: &str) -> &str {
    if type_name.contains("Vec<") {
        "dynamic"
    } else if type_name.starts_with('[') {
        "array"
    } else {
        "scalar"
    }
}

fn draw_file_nodes(
    ui: &mut egui::Ui,
    nodes: &[FileNode],
    selected: Option<&Path>,
) -> Option<ExplorerAction> {
    let mut action = None;
    for node in nodes {
        if let Some(children) = &node.children {
            let shown = egui::CollapsingHeader::new(RichText::new(&node.name).color(TEXT))
                .show(ui, |ui| draw_file_nodes(ui, children, selected));
            shown.header_response.context_menu(|ui| {
                if ui.button("New file here...").clicked() {
                    action = Some(ExplorerAction::NewFile(node.path.clone()));
                    ui.close();
                }
            });
            if let Some(child_action) = shown.body_returned.flatten() {
                action = Some(child_action);
            }
        } else {
            let editable = project::is_editable(&node.path);
            let active = selected == Some(node.path.as_path());
            let marker = node
                .git_status
                .as_deref()
                .map(|status| format!(" [{status}]"))
                .unwrap_or_default();
            let response = ui.selectable_label(
                active,
                RichText::new(format!("  {}{marker}", node.name))
                    .monospace()
                    .size(11.0)
                    .color(if active {
                        CYAN
                    } else if editable {
                        TEXT
                    } else {
                        MUTED
                    }),
            );
            if response.clicked() && editable {
                action = Some(ExplorerAction::Open(node.path.clone()));
            }
            response.context_menu(|ui| {
                if ui
                    .button(RichText::new("Delete file...").color(RED))
                    .clicked()
                {
                    action = Some(ExplorerAction::Delete(node.path.clone()));
                    ui.close();
                }
            });
        }
    }
    action
}

fn collect_editable_files(nodes: &[FileNode], paths: &mut Vec<PathBuf>) {
    for node in nodes {
        if let Some(children) = &node.children {
            collect_editable_files(children, paths);
        } else if project::is_editable(&node.path) {
            paths.push(node.path.clone());
        }
    }
}

#[derive(Clone, Copy)]
struct ThemeColors {
    background: Color32,
    surface: Color32,
    raised: Color32,
    menu: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
}

fn theme_colors(dark: bool) -> ThemeColors {
    if dark {
        ThemeColors {
            background: Color32::from_rgb(20, 24, 31),
            surface: Color32::from_rgb(27, 33, 42),
            raised: Color32::from_rgb(35, 42, 53),
            menu: Color32::from_rgb(16, 20, 26),
            border: Color32::from_rgb(62, 73, 88),
            text: Color32::from_rgb(226, 231, 238),
            muted: Color32::from_rgb(164, 174, 188),
        }
    } else {
        ThemeColors {
            background: Color32::from_rgb(248, 249, 251),
            surface: Color32::from_rgb(236, 239, 243),
            raised: Color32::from_rgb(255, 255, 255),
            menu: Color32::from_rgb(224, 228, 234),
            border: Color32::from_rgb(166, 174, 185),
            text: Color32::from_rgb(25, 30, 38),
            muted: Color32::from_rgb(76, 86, 99),
        }
    }
}

fn panel_frame(fill: Color32, dark: bool) -> Frame {
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, theme_colors(dark).border))
        .inner_margin(Margin::same(12))
}

fn compact_panel_frame(fill: Color32, dark: bool) -> Frame {
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, theme_colors(dark).border))
        .inner_margin(Margin::symmetric(8, 4))
}

fn console_panel_frame(dark: bool) -> Frame {
    Frame::new()
        .fill(theme_colors(dark).surface)
        .inner_margin(Margin::same(12))
}

fn toolbar_icon_button(
    ui: &mut egui::Ui,
    icon: egui_phosphor_icons::Icon,
    tooltip: &str,
) -> egui::Response {
    enabled_toolbar_icon_button(ui, true, icon, tooltip)
}

fn enabled_toolbar_icon_button(
    ui: &mut egui::Ui,
    enabled: bool,
    icon: egui_phosphor_icons::Icon,
    tooltip: &str,
) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(icon.regular().size(17.0)).min_size(egui::vec2(30.0, 28.0)),
    )
    .on_hover_text(tooltip)
}

fn compact_icon_button(
    ui: &mut egui::Ui,
    icon: egui_phosphor_icons::Icon,
    tooltip: &str,
) -> egui::Response {
    ui.add(
        egui::Button::new(icon.regular().size(13.0))
            .small()
            .min_size(egui::vec2(22.0, 20.0)),
    )
    .on_hover_text(tooltip)
}

fn status_row(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(11.0).color(MUTED));
        ui.add_space((ui.available_width() - 70.0).max(4.0));
        ui.label(RichText::new(value).size(11.0).monospace().color(color));
    });
}

fn configure_style(ctx: &egui::Context, dark: bool) {
    let theme = if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(theme);
    let colors = theme_colors(dark);
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = colors.background;
    visuals.window_fill = colors.surface;
    visuals.extreme_bg_color = colors.raised;
    visuals.faint_bg_color = colors.raised;
    visuals.selection.bg_fill = if dark {
        Color32::from_rgb(35, 86, 119)
    } else {
        Color32::from_rgb(184, 218, 240)
    };
    visuals.selection.stroke = Stroke::new(1.0, CYAN);
    visuals.text_cursor.stroke = Stroke::new(
        2.5,
        if dark {
            Color32::from_rgb(88, 211, 255)
        } else {
            Color32::from_rgb(0, 67, 112)
        },
    );
    visuals.text_cursor.blink = true;
    visuals.text_cursor.on_duration = 0.7;
    visuals.text_cursor.off_duration = 0.3;
    visuals.override_text_color = Some(colors.text);
    visuals.weak_text_color = Some(colors.muted);
    visuals.hyperlink_color = CYAN;
    visuals.warn_fg_color = EMBER;
    visuals.error_fg_color = RED;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, colors.text);
    visuals.widgets.inactive.bg_fill = colors.raised;
    visuals.widgets.inactive.weak_bg_fill = colors.raised;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, colors.text);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, colors.text);
    visuals.widgets.hovered.bg_fill = if dark {
        Color32::from_rgb(43, 61, 76)
    } else {
        Color32::from_rgb(211, 229, 241)
    };
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, CYAN);
    visuals.widgets.active.bg_fill = if dark {
        Color32::from_rgb(49, 72, 90)
    } else {
        Color32::from_rgb(196, 220, 236)
    };
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, EMBER);
    ctx.set_visuals_of(theme, visuals);
    let mut style = (*ctx.style_of(theme)).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style_of(theme, style);
    ctx.request_repaint();
}

#[cfg(test)]
mod editor_tests {
    use super::*;

    #[test]
    fn restores_experiment_snapshots_and_defaults_legacy_sessions() {
        let legacy: SessionState =
            serde_json::from_str(r#"{"project_root":null,"open_files":[],"active_file":null}"#)
                .unwrap();
        assert!(legacy.saved_runs.is_empty());
        assert_eq!(legacy.experiment_name, "run_1");
        assert_eq!(legacy.comparison_metric, "loss");
        assert!(legacy.dataset_viewer_docked);
        assert_eq!(legacy.dataset_pane_height, default_dataset_pane_height());

        let mut metrics = HashMap::new();
        metrics.insert("loss".to_owned(), vec![[0.0, 1.0], [1.0, 0.5]]);
        let state = SessionState {
            saved_runs: vec![ExperimentRun {
                id: RunId::new(),
                name: "baseline".to_owned(),
                metrics,
                vectors: HashMap::new(),
                execution_count: 2,
                created_at_unix: 0,
                tags: Vec::new(),
                notes: String::new(),
                archived: false,
                parent_id: None,
                artifacts: Vec::new(),
                provenance: Default::default(),
                github: Default::default(),
            }],
            experiment_name: "next_run".to_owned(),
            comparison_metric: "accuracy".to_owned(),
            ..SessionState::default()
        };
        let restored: SessionState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(restored.saved_runs.len(), 1);
        assert_eq!(restored.saved_runs[0].name, "baseline");
        assert_eq!(restored.saved_runs[0].metrics["loss"][1], [1.0, 0.5]);
        assert_eq!(restored.saved_runs[0].execution_count, 2);
        assert_eq!(restored.experiment_name, "next_run");
        assert_eq!(restored.comparison_metric, "accuracy");
    }
}
