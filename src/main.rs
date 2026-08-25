mod diagnostics;
mod lsp;
mod project;
mod runtime;

use diagnostics::DiagnosticsHandle;
use eframe::egui;
use egui::{Color32, Frame, Margin, Panel, RichText, Stroke};
use egui_code_editor::{CodeEditor, ColorTheme, Syntax};
use egui_plot::{Bar, BarChart, Line, Plot, PlotPoints};
use lsp::{Diagnostic as LspDiagnostic, LspCommand, LspEvent, LspHandle};
use project::{FileNode, Project};
use runtime::{CellResult, RuntimeHandle, Telemetry, VariableMeta};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

const TEXT: Color32 = Color32::PLACEHOLDER;
const MUTED: Color32 = Color32::PLACEHOLDER;
const EMBER: Color32 = Color32::from_rgb(196, 119, 44);
const CYAN: Color32 = Color32::from_rgb(39, 141, 204);
const GREEN: Color32 = Color32::from_rgb(46, 157, 96);
const RED: Color32 = Color32::from_rgb(212, 72, 85);
const STORAGE_KEY: &str = "forge_ml_session_v1";
const CONSOLE_CELL_ID: usize = usize::MAX;

fn default_explorer_height() -> f32 {
    280.0
}

fn main() -> eframe::Result<()> {
    // Evcxr relaunches the current executable as its isolated evaluation runtime.
    // This hook turns that child into a headless runtime before eframe can open a window.
    evcxr::runtime_hook();
    eframe::run_native(
        "Forge ML",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Forge ML - Rust compute studio")
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
    Charts,
    Help,
    Problems,
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

#[derive(Default)]
struct CellRecord {
    state: Option<CellState>,
    output: String,
    elapsed_ms: Option<u128>,
}

struct EditorTab {
    path: Option<PathBuf>,
    title: String,
    content: String,
    dirty: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct SessionState {
    project_root: Option<PathBuf>,
    open_files: Vec<PathBuf>,
    active_file: Option<PathBuf>,
    #[serde(default)]
    dark_mode: bool,
    #[serde(default = "default_explorer_height")]
    explorer_height: f32,
}

struct ForgeApp {
    tabs: Vec<EditorTab>,
    active_tab: usize,
    project: Option<Project>,
    selected_cell: usize,
    run_state: RunState,
    run_queue: VecDeque<usize>,
    cell_records: HashMap<usize, CellRecord>,
    console: String,
    runtime: RuntimeHandle,
    variables: Vec<VariableMeta>,
    metrics: HashMap<String, Vec<[f64; 2]>>,
    vectors: HashMap<String, Vec<f64>>,
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
}

impl ForgeApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
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
        let project = session
            .project_root
            .and_then(|root| Project::open(root).ok());
        let mut tabs = session
            .open_files
            .into_iter()
            .filter_map(|path| {
                std::fs::read_to_string(&path)
                    .ok()
                    .map(|content| EditorTab {
                        title: file_title(&path),
                        path: Some(path),
                        content,
                        dirty: false,
                    })
            })
            .collect::<Vec<_>>();
        if tabs.is_empty() {
            tabs.push(welcome_tab());
        }
        let active_tab = session
            .active_file
            .and_then(|active| {
                tabs.iter()
                    .position(|tab| tab.path.as_ref() == Some(&active))
            })
            .unwrap_or(0);
        Self {
            tabs,
            active_tab,
            project,
            selected_cell: 0,
            run_state: RunState::Booting,
            run_queue: VecDeque::new(),
            cell_records: HashMap::new(),
            console: "Starting isolated Rust runtime...".to_owned(),
            runtime: RuntimeHandle::spawn(),
            variables: Vec::new(),
            metrics: HashMap::new(),
            vectors: HashMap::new(),
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
        }
    }

    fn active(&self) -> &EditorTab {
        &self.tabs[self.active_tab]
    }
    fn active_mut(&mut self) -> &mut EditorTab {
        &mut self.tabs[self.active_tab]
    }

    fn cells(&self) -> Vec<(String, String)> {
        self.active()
            .content
            .split("//# %%")
            .filter_map(|raw| {
                let raw = raw.trim();
                if raw.is_empty() {
                    return None;
                }
                let (title, body) = raw.split_once('\n').unwrap_or((raw, ""));
                Some((title.trim().to_owned(), body.trim().to_owned()))
            })
            .collect()
    }

    fn open_project(&mut self) {
        if self.tabs.iter().any(|tab| tab.dirty) {
            self.console = "Save modified tabs before switching projects.".to_owned();
            return;
        }
        let Some(root) = rfd::FileDialog::new()
            .set_title("Open Forge ML project")
            .pick_folder()
        else {
            return;
        };
        match Project::open(root) {
            Ok(project) => {
                self.console = format!("Opened {}", project.root.display());
                self.project = Some(project);
            }
            Err(error) => self.console = format!("Could not open project: {error}"),
        }
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
                    content,
                    dirty: false,
                });
                self.active_tab = self.tabs.len() - 1;
                self.selected_cell = 0;
                self.cell_records.clear();
            }
            Err(error) => self.console = format!("Could not open {}: {error}", path.display()),
        }
    }

    fn save_active(&mut self) {
        let path = self.active().path.clone().or_else(|| {
            let mut dialog = rfd::FileDialog::new().set_title("Save file");
            if let Some(project) = &self.project {
                dialog = dialog.set_directory(&project.root);
            }
            dialog.save_file()
        });
        let Some(path) = path else {
            return;
        };
        let content = self.active().content.clone();
        match std::fs::write(&path, content) {
            Ok(()) => {
                let tab = self.active_mut();
                tab.path = Some(path.clone());
                tab.title = file_title(&path);
                tab.dirty = false;
                self.console = format!("Saved {}", path.display());
                if let Some(project) = &mut self.project {
                    let _ = project.refresh();
                }
                self.run_diagnostics();
            }
            Err(error) => self.console = format!("Could not save {}: {error}", path.display()),
        }
    }

    fn close_tab(&mut self, index: usize) {
        if self.tabs[index].dirty {
            self.console = format!("Save {} before closing it.", self.tabs[index].title);
            return;
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.tabs.push(welcome_tab());
        }
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        self.selected_cell = 0;
        self.cell_records.clear();
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
        if self.runtime.execute(cell_id, code).is_ok() {
            self.run_state = RunState::Running(cell_id);
            let record = self.cell_records.entry(cell_id).or_default();
            record.state = Some(CellState::Running);
            record.output.clear();
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
        self.lsp.send(LspCommand::Sync {
            root,
            path,
            text: self.active().content.clone(),
            version: self.document_version,
        });
    }

    fn request_lsp(&mut self, action: &str) {
        let Some(path) = self.active().path.clone() else {
            self.lsp_status = "Save this buffer before requesting language features.".to_owned();
            return;
        };
        let text = self.active().content.clone();
        let command = match action {
            "complete" => LspCommand::Complete {
                path,
                text,
                char_offset: self.cursor_offset,
            },
            "hover" => LspCommand::Hover {
                path,
                text,
                char_offset: self.cursor_offset,
            },
            _ => LspCommand::Definition {
                path,
                text,
                char_offset: self.cursor_offset,
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
        self.lsp.send(LspCommand::ProbeDefinition {
            path,
            text: self.active().content.clone(),
            char_offset,
        });
    }

    fn poll_background(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.runtime.try_recv() {
            match result {
                CellResult::Ready => {
                    self.run_state = RunState::Ready;
                    self.console = "Runtime ready.".to_owned();
                }
                CellResult::Success {
                    cell_id,
                    output,
                    elapsed_ms,
                    variables,
                    telemetry,
                } => {
                    self.run_state = RunState::Ready;
                    self.execution_count += 1;
                    self.variables = variables;
                    if cell_id != CONSOLE_CELL_ID {
                        let record = self.cell_records.entry(cell_id).or_default();
                        record.state = Some(CellState::Passed);
                        record.output = output.clone();
                        record.elapsed_ms = Some(elapsed_ms);
                    }
                    self.console = if output.is_empty() {
                        format!("Cell {} completed in {elapsed_ms} ms.", cell_id + 1)
                    } else {
                        output
                    };
                    for item in telemetry {
                        match item {
                            Telemetry::Metric { name, value } => {
                                let series = self.metrics.entry(name).or_default();
                                series.push([series.len() as f64, value]);
                            }
                            Telemetry::Vector { name, values } => {
                                self.vectors.insert(name, values);
                            }
                        }
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
                    self.cell_records.clear();
                    self.console = "Runtime state cleared.".to_owned();
                }
                CellResult::RuntimeError(message) => {
                    self.run_state = RunState::Failed;
                    self.console = format!("Runtime unavailable\n\n{message}");
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
                LspEvent::Diagnostics { path, items } => {
                    self.lsp_diagnostics.insert(path, items);
                }
                LspEvent::Completions(items) => {
                    self.completions = items;
                    self.inspector_tab = InspectorTab::Help;
                    self.lsp_status = "Completion results ready.".to_owned();
                }
                LspEvent::Hover(text) => {
                    self.hover_text = text;
                    self.inspector_tab = InspectorTab::Help;
                    self.lsp_status = "Hover information ready.".to_owned();
                }
                LspEvent::Definition { path, line } => {
                    self.open_file(path.clone());
                    self.console = format!("Definition: {}:{}", path.display(), line + 1);
                }
                LspEvent::DefinitionProbe {
                    char_offset,
                    navigable,
                } => {
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
                if ui.button("Open project...").clicked() {
                    self.open_project();
                }
                if ui.button("Save   Ctrl+S").clicked() {
                    self.save_active();
                }
                if ui.button("Close editor tab").clicked() {
                    self.close_tab(self.active_tab);
                }
            });
            ui.menu_button("Edit", |ui| {
                ui.label("Editor commands use standard system shortcuts.");
            });
            ui.menu_button("Search", |ui| {
                ui.label("Project-wide search is the next navigation tool.");
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
            });
            ui.menu_button("Debug", |ui| {
                ui.label("Debugger integration is not connected yet.");
            });
            ui.menu_button("Tools", |ui| {
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
                ui.label("Forge ML - interactive Rust scientific environment");
            });
        });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Open").on_hover_text("Open project").clicked() {
                self.open_project();
            }
            if ui.button("Save").on_hover_text("Save file").clicked() {
                self.save_active();
            }
            ui.separator();
            if ui
                .button("Check")
                .on_hover_text("Run code analysis")
                .clicked()
            {
                self.run_diagnostics();
            }
            if ui
                .button("Complete")
                .on_hover_text("Request rust-analyzer completions at the cursor")
                .clicked()
            {
                self.request_lsp("complete");
            }
            if ui
                .button("Hover")
                .on_hover_text("Show type and documentation at the cursor")
                .clicked()
            {
                self.request_lsp("hover");
            }
            if ui
                .button("Go to")
                .on_hover_text("Open the definition at the cursor")
                .clicked()
            {
                self.request_lsp("definition");
            }
            ui.separator();
            let ready = !matches!(self.run_state, RunState::Running(_) | RunState::Booting);
            if ui
                .add_enabled(ready, egui::Button::new("Run cell"))
                .clicked()
            {
                self.enqueue_cells([self.selected_cell]);
            }
            if ui
                .add_enabled(ready, egui::Button::new("Run above"))
                .clicked()
            {
                self.enqueue_cells(0..=self.selected_cell);
            }
            if ui
                .add_enabled(ready, egui::Button::new("Run all"))
                .clicked()
            {
                self.enqueue_cells(0..self.cells().len());
            }
            if ui.button("Reset").clicked() {
                let _ = self.runtime.reset();
                self.run_state = RunState::Booting;
            }
            ui.add_space((ui.available_width() - 210.0).max(8.0));
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
    }

    fn file_explorer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("FILES").size(10.0).strong().color(MUTED));
            if ui.small_button("Open").clicked() {
                self.open_project();
            }
            if ui.small_button("Refresh").clicked() {
                if let Some(project) = &mut self.project {
                    let _ = project.refresh();
                }
            }
        });
        ui.add_space(5.0);
        let selected = self.active().path.clone();
        let clicked = self.project.as_ref().and_then(|project| {
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
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    draw_file_nodes(ui, &project.files, selected.as_deref())
                })
                .inner
        });
        if let Some(path) = clicked {
            self.open_file(path);
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
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
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

    fn outline(&self, ui: &mut egui::Ui) {
        ui.label(RichText::new(&self.active().title).strong().color(TEXT));
        for (line_no, line) in self.active().content.lines().enumerate() {
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
                ui.label(
                    RichText::new(format!("-  {symbol}  :{}", line_no + 1))
                        .monospace()
                        .size(11.0)
                        .color(CYAN),
                );
            }
        }
        ui.add_space(8.0);
        ui.separator();
    }

    fn editor_tabs(&mut self, ui: &mut egui::Ui) {
        let mut select = None;
        let mut close = None;
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                for (index, tab) in self.tabs.iter().enumerate() {
                    let title = if tab.dirty {
                        format!("* {}", tab.title)
                    } else {
                        tab.title.clone()
                    };
                    if ui
                        .selectable_label(
                            index == self.active_tab,
                            RichText::new(title).color(if tab.dirty { EMBER } else { TEXT }),
                        )
                        .clicked()
                    {
                        select = Some(index);
                    }
                    if index == self.active_tab && ui.small_button("x").clicked() {
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

    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (tab, label) in [
                (InspectorTab::Variables, "Variables"),
                (InspectorTab::Charts, "Plots"),
                (InspectorTab::Help, "Help"),
                (InspectorTab::Problems, "Problems"),
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
            InspectorTab::Charts => self.charts(ui),
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
                        .max_height(150.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(&self.hover_text).monospace().size(10.0));
                        });
                }
                if !self.completions.is_empty() {
                    ui.separator();
                    ui.label(RichText::new("Completions").strong().color(CYAN));
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for item in &self.completions {
                                ui.label(RichText::new(item).monospace().size(10.0));
                            }
                        });
                }
                ui.separator();
                ui.heading("Scientific console");
                ui.label("Execute `//# %%` cells in a persistent Evcxr session. Variables created by successful cells remain available to later cells and console commands.");
                ui.separator();
                ui.label(RichText::new("Telemetry").strong().color(CYAN));
                ui.code("println!(\"forge_metric:loss={}\", loss);\nprintln!(\"forge_vector:w=1,2,3\");");
                ui.separator();
                ui.label(RichText::new("Shortcuts").strong().color(CYAN));
                ui.label("Shift+Enter  Run cell\nCtrl+Shift+Enter  Run all\nCtrl+S  Save file");
            }
            InspectorTab::Problems => {
                if ui.button("Run cargo check").clicked() {
                    self.run_diagnostics();
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (path, diagnostics) in &self.lsp_diagnostics {
                        for diagnostic in diagnostics {
                            let color = if diagnostic.severity == 1 {
                                RED
                            } else if diagnostic.severity == 2 {
                                EMBER
                            } else {
                                TEXT
                            };
                            ui.label(
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
                            );
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
            }
        }
    }

    fn charts(&self, ui: &mut egui::Ui) {
        if self.metrics.is_empty() && self.vectors.is_empty() {
            ui.label(
                RichText::new(
                    "Emit `forge_metric:loss=0.42` or `forge_vector:weights=1,2,3` from a cell.",
                )
                .monospace()
                .size(10.0)
                .color(MUTED),
            );
        }
        for (name, values) in &self.metrics {
            ui.label(RichText::new(name).strong().color(EMBER));
            let points: PlotPoints = values.clone().into();
            Plot::new(format!("metric_{name}"))
                .height(175.0)
                .allow_drag(false)
                .show(ui, |p| {
                    p.line(Line::new(name, points).color(EMBER).width(2.0))
                });
        }
        for (name, values) in &self.vectors {
            ui.label(RichText::new(name).strong().color(CYAN));
            let bars = values
                .iter()
                .enumerate()
                .map(|(i, value)| Bar::new(i as f64, *value))
                .collect();
            Plot::new(format!("vector_{name}"))
                .height(175.0)
                .allow_drag(false)
                .show(ui, |p| p.bar_chart(BarChart::new(name, bars).color(CYAN)));
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
            if let Some(ms) = self
                .cell_records
                .get(&self.selected_cell)
                .and_then(|r| r.elapsed_ms)
            {
                ui.label(RichText::new(format!("{ms} ms")).size(10.0).color(CYAN));
            }
            if ui.small_button("Clear").clicked() {
                self.console.clear();
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
                    .max_height((ui.available_height() - 34.0).max(30.0))
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
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
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, command) in self.history.iter().enumerate() {
                        ui.label(
                            RichText::new(format!("In [{}]: {}", index + 1, command))
                                .monospace()
                                .color(TEXT),
                        );
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
        let run = ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::Enter));
        let run_all = ui
            .input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Enter));
        if save {
            self.save_active();
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
            .show(ui, |ui| self.inspector(ui));
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
                ui.add_space(5.0);
                let output = CodeEditor::default()
                    .id_source(format!("editor_{}", self.active_tab))
                    .with_rows(32)
                    .with_fontsize(14.0)
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
                if let Some(range) = output.cursor_range {
                    self.cursor_offset = range.primary.index.0;
                    if output.response.has_focus() {
                        paint_editor_caret(ui, &output, range.primary, self.dark_mode);
                    }
                }
                let ctrl_held = ui.input(|input| input.modifiers.ctrl);
                let hovered_offset = if output.response.hovered() {
                    ui.ctx()
                        .pointer_hover_pos()
                        .map(|pointer| {
                            let raw_offset = output
                                .galley
                                .cursor_from_pos(pointer - output.galley_pos)
                                .index
                                .0;
                            word_start_at(&self.tabs[self.active_tab].content, raw_offset)
                        })
                        .flatten()
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
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
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
        };
        eframe::set_value(storage, STORAGE_KEY, &state);
    }
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

fn paint_editor_caret(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    cursor: egui::text::CCursor,
    dark: bool,
) {
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(50));
    let blink_phase = ui.input(|input| input.time) % 1.0;
    let bright_phase = blink_phase < 0.65;
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

fn welcome_tab() -> EditorTab {
    EditorTab {
        path: None,
        title: "experiment.rs".to_owned(),
        dirty: false,
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
) -> Option<PathBuf> {
    let mut clicked = None;
    for node in nodes {
        if let Some(children) = &node.children {
            let shown = egui::CollapsingHeader::new(RichText::new(&node.name).color(TEXT))
                .show(ui, |ui| draw_file_nodes(ui, children, selected));
            if let Some(path) = shown.body_returned.flatten() {
                clicked = Some(path);
            }
        } else {
            let editable = project::is_editable(&node.path);
            let active = selected == Some(node.path.as_path());
            if ui
                .selectable_label(
                    active,
                    RichText::new(format!("  {}", node.name))
                        .monospace()
                        .size(11.0)
                        .color(if active {
                            CYAN
                        } else if editable {
                            TEXT
                        } else {
                            MUTED
                        }),
                )
                .clicked()
                && editable
            {
                clicked = Some(node.path.clone());
            }
        }
    }
    clicked
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
