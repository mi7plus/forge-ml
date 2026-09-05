// Release builds are a GUI app: don't allocate a console window (which otherwise
// pops up a terminal alongside the IDE). Debug builds keep the console so the
// startup/runtime diagnostics printed to stderr stay visible during development.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_exec;
mod app_files;
mod app_lsp;
mod classification;
mod commands;
mod data;
mod database;
mod deep_learning;
mod diagnostics;
mod environment;
mod experiment;
mod export;
mod git;
mod github;
mod integration_worker;
mod jobs;
mod jupyter;
mod keymap;
mod lsp;
mod millwright_studio;
mod model_registry;
mod notebook;
mod object_storage;
mod offline;
mod packages;
mod performance;
mod plot;
mod prep;
mod privacy_diagnostics;
mod project;
mod publishing;
mod python_kernel;
mod python_runtime;
mod release;
mod remote;
mod result_ext;
mod runtime;
mod rust_kernel;
mod service_monitor;
mod session;
mod terminal;
mod ui;
mod updater;
mod workspace;

use data::DataWorkspace;
use database::{ConnectionKind, ConnectionProfile};
use deep_learning::{Backend as DeepBackend, DeepOutputs, NativeTrainingConfig, ResourceSnapshot};
use diagnostics::DiagnosticsHandle;
use eframe::egui;
use egui::{Color32, Frame, Margin, Panel, RichText, Stroke};
use egui_code_editor::{CodeEditor, Syntax};
use egui_plot::{Bar, BarChart, Line, Plot, PlotPoints, Points, Polygon};
use egui_tiles::{Container, Linear, LinearDir, SimplificationOptions, Tile, TileId, Tiles, Tree};
use experiment::{capture_provenance, ExperimentRun};
#[cfg(test)]
use forge_protocol::RunId;
use forge_protocol::TableData;
use forge_storage::{WorkspaceRecovery, WorkspaceStore};
use integration_worker::{IntegrationWorker, Request as IntegrationRequest, ResultEvent};
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
use plot::{metric_line, vector_bars, PlotKind, PlotSpec};
use project::Project;
use runtime::{CellResult, RuntimeHandle, VariableMeta};
use service_monitor::{DriftEvent, ServiceEvent};
use session::SessionState;
use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering as AtomicOrdering},
    mpsc::{self, Receiver, Sender},
    Arc,
};
use std::time::{Duration, Instant};
use ui::editing::{
    apply_edits_to, blank_tab, char_to_byte, collect_editable_files, content_hash, csv_field,
    draw_file_nodes, file_title, infer_size, line_column, paint_editor_caret,
    paint_inline_diagnostics, paint_navigable_word, run_rustfmt, safe_file_stem, welcome_tab,
    word_start_at,
};
use ui::grid::{
    build_row_index, build_row_index_cancellable, selected_table, visible_column_window,
    FilterMode, RowFilter, INDEX_SORT_COLUMN,
};
use ui::plotting::{
    box_stats, draw_box_summary, draw_heatmap, ecdf, histogram, kde, quartiles, transformed_points,
};
use ui::theme::{
    accent, compact_icon_button, compact_panel_frame, configure_style, panel_frame, theme_colors,
    EMBER, GREEN, MUTED, RED, TEXT,
};

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

/// Stop child processes (cargo, rustc, rust-analyzer, git, the Evcxr runtime, …)
/// from each popping up their own console window. As a GUI-subsystem binary we
/// have no console, so on Windows every console child would allocate a fresh,
/// *visible* one — briefly for quick commands, and for the whole build during a
/// notebook `:dep` compile. Attach to the launching terminal if there is one (dev
/// runs, where that output is wanted); otherwise allocate a console and hide it so
/// children inherit that hidden console instead of spawning windows. Evcxr and
/// rust-analyzer spawn their own subprocesses we can't flag individually, so a
/// shared hidden console is the only fix that covers the whole process tree.
#[cfg(windows)]
fn tame_child_consoles() {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(dw_process_id: u32) -> i32;
        fn AllocConsole() -> i32;
        fn GetConsoleWindow() -> *mut c_void;
    }
    #[link(name = "user32")]
    extern "system" {
        fn ShowWindow(h_wnd: *mut c_void, n_cmd_show: i32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    const SW_HIDE: i32 = 0;
    unsafe {
        // Launched from a terminal: reuse it and leave its window alone.
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            return;
        }
        // Launched from Explorer / the installed shortcut: no console exists.
        // Make one for children to inherit, then hide its window.
        if AllocConsole() != 0 {
            let hwnd = GetConsoleWindow();
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
    }
}

#[cfg(not(windows))]
fn tame_child_consoles() {}

/// The directory argument following an `--env-*` flag, or the current directory.
fn env_cli_dir(args: &[String], flag_pos: usize) -> std::path::PathBuf {
    args.get(flag_pos + 1)
        .filter(|value| !value.starts_with("--"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
}

fn main() -> eframe::Result<()> {
    // Evcxr relaunches the current executable as its isolated evaluation runtime.
    // This hook turns that child into a headless runtime before eframe can open a window.
    evcxr::runtime_hook();
    // Give descendant processes a (hidden) console to inherit so they don't each
    // pop up a window. Must run before anything is spawned (LSP, runtime, git).
    tame_child_consoles();
    // Hidden headless self-test: exercise the notebook runtime path (kernel child,
    // env, a `:dep millwright` cell) without opening the GUI, for diagnosis.
    if std::env::args().any(|a| a == "--notebook-selftest") {
        notebook_selftest();
        return Ok(());
    }
    // Forge environment CLI seams (see docs/FORGE_ENV.md). `--env-doctor [dir]`
    // reports manifest/provider/gap state; `--env-sync [dir]` writes forge.lock.
    let cli: Vec<String> = std::env::args().collect();
    if let Some(pos) = cli.iter().position(|a| a == "--env-doctor") {
        let root = env_cli_dir(&cli, pos);
        print!("{}", environment::doctor(&root));
        return Ok(());
    }
    if let Some(pos) = cli.iter().position(|a| a == "--env-sync") {
        let root = env_cli_dir(&cli, pos);
        match environment::sync_project(&root) {
            Ok(path) => println!("Wrote {}", path.display()),
            Err(error) => eprintln!("forge env sync failed: {error}"),
        }
        return Ok(());
    }
    let app_name = format!("Forge ML {APP_VERSION}");
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("Forge ML {APP_VERSION} - Rust compute studio"))
        .with_inner_size([1380.0, 860.0])
        .with_min_inner_size([980.0, 640.0]);
    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        &app_name,
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(ForgeApp::new(cc)))),
    )
}

/// Numeric scalar types we know how to serialize element-wise.
const NUMERIC_TYPES: [&str; 12] = [
    "f64", "f32", "i64", "i32", "i16", "i8", "u64", "u32", "u16", "u8", "usize", "isize",
];

/// Build a hidden Evcxr snippet that surfaces variable `name` (of `type_name`):
/// a numeric matrix or Millwright `Frame` becomes a `forge_table:` event, a
/// numeric vector a `forge_vector:` event, and anything else is pretty-printed.
/// Returns the snippet plus the Data-viewer key the event will land under (if any).
fn inspect_code(name: &str, type_name: &str) -> (String, Option<String>) {
    let t = type_name.replace(' ', "");
    let is_matrix = NUMERIC_TYPES
        .iter()
        .any(|n| t.contains(&format!("Vec<Vec<{n}>>")));
    let is_vector = NUMERIC_TYPES
        .iter()
        .any(|n| t.ends_with(&format!("Vec<{n}>")));
    if is_matrix {
        (
            MATRIX_INSPECT.replace("NAME", name),
            Some(format!("table:{name}")),
        )
    } else if is_vector {
        (
            VECTOR_INSPECT.replace("NAME", name),
            Some(format!("vector:{name}")),
        )
    } else if t.contains("Frame") {
        (
            FRAME_INSPECT.replace("NAME", name),
            Some(format!("table:{name}")),
        )
    } else {
        (DEBUG_INSPECT.replace("NAME", name), None)
    }
}

/// `NAME` is replaced with the variable name. Each emits the marker Forge parses
/// from cell stdout. Column names are `c0..cN`; numeric cells need no escaping.
const VECTOR_INSPECT: &str = r#"println!("forge_vector:NAME={}", NAME.iter().map(|__v| __v.to_string()).collect::<Vec<_>>().join(","));"#;
const MATRIX_INSPECT: &str = r#"{
    let __data = &NAME;
    let __ncols = __data.first().map(|__r| __r.len()).unwrap_or(0);
    let __cols: Vec<String> = (0..__ncols).map(|__i| format!("\"c{}\"", __i)).collect();
    let __rows: Vec<String> = __data.iter().map(|__r| {
        let __c: Vec<String> = __r.iter().map(|__v| __v.to_string()).collect();
        format!("[{}]", __c.join(","))
    }).collect();
    println!("forge_table:NAME={{\"columns\":[{}],\"rows\":[{}]}}", __cols.join(","), __rows.join(","));
}"#;
const FRAME_INSPECT: &str = r#"{
    let __f = &NAME;
    let __cols: Vec<String> = __f.columns().iter().map(|__c| format!("\"{}\"", __c)).collect();
    let __rows: Vec<String> = __f.as_rows().iter().map(|__r| {
        let __c: Vec<String> = __r.iter().map(|__v| __v.to_string()).collect();
        format!("[{}]", __c.join(","))
    }).collect();
    println!("forge_table:NAME={{\"columns\":[{}],\"rows\":[{}]}}", __cols.join(","), __rows.join(","));
}"#;
const DEBUG_INSPECT: &str = r#"println!("NAME = {:#?}", NAME);"#;

/// Headless reproduction of the notebook runtime: spawns the same runtime handle
/// forge_ide uses, waits for it to be ready, runs a `:dep millwright` cell, and
/// prints the outcome. Exits when the cell finishes, errors, or times out.
fn notebook_selftest() {
    use std::time::{Duration, Instant};
    eprintln!("[selftest] spawning runtime (real RuntimeHandle path)…");
    let rt = runtime::RuntimeHandle::spawn();
    let start = Instant::now();
    let mut ready = false;
    let cell = "\
:dep millwright = { version = \"2.2.1\", default-features = false, features = [\"smartcore-backend\"] }\n\
use millwright::prelude::*;\n\
let _ = Frame::from_rows(vec![vec![0.0]], vec![\"x\".into()]);\n\
println!(\"Millwright ready.\");";
    loop {
        while let Some(result) = rt.try_recv() {
            match result {
                runtime::CellResult::Ready if !ready => {
                    ready = true;
                    eprintln!(
                        "[selftest] ready in {:?}; sending :dep cell",
                        start.elapsed()
                    );
                    let _ = rt.execute(0, cell.to_owned());
                }
                runtime::CellResult::Success {
                    output, cell_id, ..
                } => {
                    eprintln!(
                        "[selftest] cell {cell_id} SUCCESS: {}",
                        output.replace('\n', " | ")
                    );
                    // After the :dep cell, exercise variable inspection.
                    if cell_id == 0 {
                        let (code, key) = inspect_code("__probe", "Vec<Vec<f64>>");
                        eprintln!("[selftest] inspecting a matrix (key={key:?})…");
                        let _ = rt.execute(
                            1,
                            format!(
                                "let __probe = vec![vec![1.0_f64, 2.0], vec![3.0, 4.0]];\n{code}"
                            ),
                        );
                    } else {
                        return;
                    }
                }
                runtime::CellResult::Error { message, .. } => {
                    eprintln!("[selftest] ERROR: {message}");
                    return;
                }
                runtime::CellResult::RuntimeError(m) => {
                    eprintln!("[selftest] RUNTIME ERROR: {m}");
                    return;
                }
                _ => {}
            }
        }
        if start.elapsed() > Duration::from_secs(200) {
            eprintln!("[selftest] TIMEOUT (still hanging)");
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The window/taskbar icon, decoded from the embedded badge PNG.
fn load_app_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).ok()
}

/// Decode the transparent Forge mark into a texture for the splash screen.
fn load_splash_logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let bytes = include_bytes!("../assets/mark-256.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(&bytes[..]));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [info.width as usize, info.height as usize],
        &buf[..info.buffer_size()],
    );
    Some(ctx.load_texture("forge_mark", image, egui::TextureOptions::LINEAR))
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
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
    Database,
    DeepLearning,
    Deploy,
    Storage,
}

impl InspectorTab {
    /// Stable ordering used to build the default dock layout and the View menu.
    const ALL: [InspectorTab; 15] = [
        InspectorTab::Variables,
        InspectorTab::Data,
        InspectorTab::Charts,
        InspectorTab::Experiments,
        InspectorTab::Search,
        InspectorTab::Help,
        InspectorTab::Problems,
        InspectorTab::Git,
        InspectorTab::Packages,
        InspectorTab::GitHub,
        InspectorTab::Studio,
        InspectorTab::Database,
        InspectorTab::DeepLearning,
        InspectorTab::Deploy,
        InspectorTab::Storage,
    ];

    fn label(self) -> &'static str {
        match self {
            InspectorTab::Variables => "Variables",
            InspectorTab::Data => "Data",
            InspectorTab::Charts => "Plots",
            InspectorTab::Experiments => "Runs",
            InspectorTab::Search => "Search",
            InspectorTab::Help => "Help",
            InspectorTab::Problems => "Problems",
            InspectorTab::Git => "Git",
            InspectorTab::Packages => "Crates",
            InspectorTab::GitHub => "GitHub",
            InspectorTab::Studio => "Studio",
            InspectorTab::Database => "SQL",
            InspectorTab::DeepLearning => "Deep",
            InspectorTab::Deploy => "Deploy",
            InspectorTab::Storage => "Storage",
        }
    }

    fn icon(self) -> &'static str {
        use egui_phosphor_icons::icons;
        match self {
            InspectorTab::Variables => icons::CUBE,
            InspectorTab::Data => icons::GRID_FOUR,
            InspectorTab::Charts => icons::CHART_LINE,
            InspectorTab::Experiments => icons::FLASK,
            InspectorTab::Search => icons::MAGNIFYING_GLASS,
            InspectorTab::Help => icons::QUESTION,
            InspectorTab::Problems => icons::BUG,
            InspectorTab::Git => icons::GIT_BRANCH,
            InspectorTab::Packages => icons::PACKAGE,
            InspectorTab::GitHub => icons::GITHUB_LOGO,
            InspectorTab::Studio => icons::FLOW_ARROW,
            InspectorTab::Database => icons::DATABASE,
            InspectorTab::DeepLearning => icons::BRAIN,
            InspectorTab::Deploy => icons::ROCKET_LAUNCH,
            InspectorTab::Storage => icons::CLOUD,
        }
        .as_str()
    }
}

/// An action requested from a dock tab's right-click menu, applied against the
/// full tree after layout (the tab hook only has access to `Tiles`).
#[derive(Clone, Copy, PartialEq)]
enum DockAction {
    Hide,
    Undock,
}

/// A dockable surface in the [`egui_tiles`] workspace tree. Each pane maps to an
/// existing render method; the tree owns their layout, so panes can be split,
/// re-docked between regions, floated, or hidden without bespoke panel code.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
enum PaneKind {
    Editor,
    Files,
    Outline,
    Cells,
    Console,
    History,
    Python,
    /// A read-oriented notebook view: every cell stacked with its captured
    /// output (text, rich MIME, and the plots it produced) shown inline.
    Notebook,
    /// A terminal instance. Each carries a unique id so several terminals can
    /// coexist as independent, dockable/floatable tiles.
    Terminal(u32),
    /// An independent Rust REPL kernel (its own Evcxr session), one per id.
    RustConsole(u32),
    DataViewer,
    Inspector(InspectorTab),
}

impl PaneKind {
    fn title(self) -> &'static str {
        match self {
            PaneKind::Editor => "Editor",
            PaneKind::Files => "Files",
            PaneKind::Outline => "Outline",
            PaneKind::Cells => "Cells",
            PaneKind::Console => "Console",
            PaneKind::History => "History",
            PaneKind::Python => "Python",
            PaneKind::Notebook => "Notebook",
            PaneKind::Terminal(_) => "Terminal",
            PaneKind::RustConsole(_) => "Rust kernel",
            PaneKind::DataViewer => "Data viewer",
            PaneKind::Inspector(tab) => tab.label(),
        }
    }

    fn icon(self) -> &'static str {
        use egui_phosphor_icons::icons;
        match self {
            PaneKind::Editor => icons::CODE.as_str(),
            PaneKind::Files => icons::TREE_STRUCTURE.as_str(),
            PaneKind::Outline => icons::LIST_BULLETS.as_str(),
            PaneKind::Cells => icons::ROWS.as_str(),
            PaneKind::Console => icons::TERMINAL_WINDOW.as_str(),
            PaneKind::History => icons::CLOCK_COUNTER_CLOCKWISE.as_str(),
            PaneKind::Python => icons::CODE_SIMPLE.as_str(),
            PaneKind::Notebook => icons::NOTEBOOK.as_str(),
            PaneKind::Terminal(_) => icons::TERMINAL.as_str(),
            PaneKind::RustConsole(_) => icons::CUBE.as_str(),
            PaneKind::DataViewer => icons::TABLE.as_str(),
            PaneKind::Inspector(tab) => tab.icon(),
        }
    }

    /// Tab label as shown in the dock: icon glyph followed by the pane name.
    fn tab_label(self) -> String {
        format!("{}  {}", self.icon(), self.title())
    }
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

/// The status-bar run-state chip: (icon glyph, label, color). Kept pure so it is
/// unit-testable — in particular the `CONSOLE_CELL_ID` sentinel (`usize::MAX`)
/// must render as "Running console" and never `cell + 1` into an overflow.
fn run_state_chip(run_state: RunState) -> (&'static str, String, egui::Color32) {
    use egui_phosphor_icons::icons;
    match run_state {
        RunState::Booting => (icons::CIRCLE_DASHED.as_str(), "Booting".to_owned(), EMBER),
        RunState::Ready => (icons::CHECK_CIRCLE.as_str(), "Ready".to_owned(), GREEN),
        RunState::Running(cell) => (
            icons::CIRCLE_NOTCH.as_str(),
            if cell == CONSOLE_CELL_ID {
                "Running console".to_owned()
            } else {
                format!("Running cell {}", cell + 1)
            },
            accent(),
        ),
        RunState::Failed => (icons::X_CIRCLE.as_str(), "Runtime failed".to_owned(), RED),
    }
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
    /// Names of the structured plots this cell emitted (`forge_plot:` events),
    /// so the Notebook pane can render them inline with the cell.
    plots: Vec<String>,
    /// Datasets this cell emitted, as `open_dataset` refs (`table:name` /
    /// `vector:name`), for inline preview + open in the Notebook pane.
    datasets: Vec<String>,
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

#[derive(Default)]
struct DatasetViewState {
    filter: RowFilter,
    sort_column: Option<usize>,
    sort_descending: bool,
    selected_rows: std::collections::BTreeSet<usize>,
    visible: Vec<bool>,
    pinned: Vec<bool>,
    widths: Vec<f32>,
    edit_draft: Option<TableData>,
    linked_x: usize,
    linked_y: usize,
    row_index_cache: Option<RowIndexCache>,
    row_index_pending: Option<RowIndexKey>,
    row_index_worker: RowIndexWorker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RowIndexKey {
    revision: u64,
    filter: RowFilter,
    sort_column: Option<usize>,
    sort_descending: bool,
}

struct RowIndexCache {
    revision: u64,
    filter: RowFilter,
    sort_column: Option<usize>,
    sort_descending: bool,
    rows: Vec<usize>,
}

struct RowIndexRequest {
    key: RowIndexKey,
    data: Arc<TableData>,
    generation: u64,
}

struct RowIndexWorker {
    sender: Sender<RowIndexRequest>,
    receiver: Receiver<RowIndexCache>,
    generation: Arc<AtomicU64>,
}

impl Default for RowIndexWorker {
    fn default() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<RowIndexRequest>();
        let (result_tx, result_rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let worker_generation = generation.clone();
        std::thread::spawn(move || {
            while let Ok(mut request) = request_rx.recv() {
                while let Ok(newer) = request_rx.try_recv() {
                    request = newer;
                }
                let key = request.key;
                let Some(rows) = build_row_index_cancellable(
                    &request.data,
                    &key.filter,
                    key.sort_column,
                    key.sort_descending,
                    || worker_generation.load(AtomicOrdering::Relaxed) != request.generation,
                ) else {
                    continue;
                };
                if result_tx
                    .send(RowIndexCache {
                        revision: key.revision,
                        filter: key.filter,
                        sort_column: key.sort_column,
                        sort_descending: key.sort_descending,
                        rows,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            sender: request_tx,
            receiver: result_rx,
            generation,
        }
    }
}

impl RowIndexWorker {
    fn submit(&self, key: RowIndexKey, data: Arc<TableData>) -> Result<(), String> {
        let generation = self.generation.fetch_add(1, AtomicOrdering::Relaxed) + 1;
        self.sender
            .send(RowIndexRequest {
                key,
                data,
                generation,
            })
            .map_err(|error| error.to_string())
    }
}

#[derive(Default)]
struct DatasetViewResult {
    committed: Option<TableData>,
    linked_plot: Option<PlotSpec>,
    message: Option<String>,
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
    structured_plots: Vec<PlotSpec>,
    open_dataset: Option<String>,
    dataset_views: HashMap<String, DatasetViewState>,
    dataset_viewer_docked: bool,
    dataset_pane_height: f32,
    inspector_tab: InspectorTab,
    diagnostics: DiagnosticsHandle,
    diagnostic_lines: Vec<String>,
    diagnostics_running: bool,
    execution_count: usize,
    console_input: String,
    history: Vec<String>,
    lsp: LspHandle,
    lsp_status: String,
    lsp_diagnostics: HashMap<PathBuf, Vec<LspDiagnostic>>,
    completions: Vec<(String, String)>,
    hover_text: String,
    lsp_references: Vec<lsp::Reference>,
    lsp_signature: String,
    rename_open: bool,
    rename_input: String,
    go_to_line_open: bool,
    go_to_line_input: String,
    code_actions: Vec<lsp::CodeAction>,
    cursor_offset: usize,
    document_version: i32,
    last_lsp_hash: u64,
    hover_probe_offset: Option<usize>,
    navigable_hover_offset: Option<usize>,
    definition_probe_pending: bool,
    dark_mode: bool,
    /// Active custom theme name (`None` = the built-in Dark/Light per `dark_mode`).
    active_theme: Option<String>,
    /// User-authored themes available in the theme builder.
    custom_themes: Vec<ui::theme::NamedTheme>,
    /// Draft palette the theme builder edits before saving.
    theme_draft: ui::theme::Palette,
    theme_new_name: String,
    /// Set when the theme changed without an egui context to hand; the render
    /// loop re-applies the palette on the next frame.
    theme_dirty: bool,
    /// Global UI zoom factor scaling all text and widgets (1.0 = 100%).
    ui_scale: f32,
    /// When the splash overlay started; `None` once it has been dismissed.
    splash_start: Option<Instant>,
    /// The Forge mark, decoded once for the splash screen.
    splash_logo: Option<egui::TextureHandle>,
    /// Set once rust-analyzer reports it has finished indexing.
    lsp_ready: bool,
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
    format_on_save: bool,
    keymap: keymap::Keymap,
    /// The action whose shortcut is currently being re-captured in Settings.
    rebinding: Option<keymap::KeyAction>,
    /// Whether the welcome / start window is showing.
    welcome_open: bool,
    high_contrast: bool,
    lsp_enabled: bool,
    reduced_motion: bool,
    command_palette_open: bool,
    command_query: String,
    command_selection: usize,
    /// Most-recently-run commands, newest first, shown when the palette is empty.
    recent_commands: Vec<commands::Command>,
    status_announcement: String,
    diagnostics_opt_in: bool,
    completion_popup_open: bool,
    git_output: String,
    git_commit_message: String,
    git_branch_name: String,
    git_conflicts: Vec<String>,
    git_branches: Vec<git::BranchInfo>,
    git_selected_branch: Option<String>,
    package_query: String,
    package_output: String,
    cargo_registry: String,
    python_registry: String,
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
    integration_worker: IntegrationWorker,
    integration_pending: usize,
    job_command: String,
    database_profiles: Vec<ConnectionProfile>,
    database_selected: usize,
    database_name: String,
    database_kind: ConnectionKind,
    database_location: String,
    database_username: String,
    database_secret: String,
    sql_editor: String,
    sql_output: String,
    sql_history: Vec<String>,
    deep_backend: DeepBackend,
    burn_training_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    burn_training_epochs: usize,
    burn_training_learning_rate: f64,
    burn_training_validation_fraction: f64,
    burn_training_use_dataset: bool,
    burn_training_feature: String,
    burn_training_target: String,
    class_features: String,
    class_target: String,
    class_epochs: usize,
    class_lr: f64,
    class_test_fraction: f64,
    class_result: String,
    class_model: Option<classification::Classifier>,
    class_playground: Vec<f64>,
    prep_categorical: String,
    prep_encoding: prep::Encoding,
    prep_missing: prep::Missing,
    prep_scaling: prep::Scaling,
    prep_result: String,
    onnx_model: Option<millwright::onnx::InferenceModel>,
    onnx_model_name: String,
    onnx_input: String,
    onnx_result: String,
    native_burn_artifact: Option<deep_learning::NativeRegressionArtifact>,
    native_burn_inference_feature: f64,
    drift_mean_shift_threshold: f64,
    drift_scale_ratio_lower: f64,
    drift_scale_ratio_upper: f64,
    deep_outputs: DeepOutputs,
    resource_system: sysinfo::System,
    resource_snapshot: ResourceSnapshot,
    last_resource_poll: Instant,
    early_stopping_patience: usize,
    resume_checkpoint: String,
    remote_profiles: Vec<remote::RemoteProfile>,
    remote_name: String,
    remote_url: String,
    remote_command: String,
    remote_token: String,
    remote_kernel_name: String,
    remote_kernel_session: Option<remote::RemoteKernelSession>,
    remote_code: String,
    remote_mime_outputs: Vec<RichOutput>,
    remote_execution_pending: bool,
    remote_interrupt_pending: bool,
    remote_notebook_execution: bool,
    remote_input_sender: Option<Sender<String>>,
    remote_input_prompt: Option<String>,
    remote_input_response: String,
    remote_input_password: bool,
    registry_model: String,
    registry_version: String,
    registry_format: String,
    registry_alias: String,
    registry_artifact: String,
    registry_output: String,
    service_events: Vec<ServiceEvent>,
    drift_events: Vec<DriftEvent>,
    object_profiles: Vec<object_storage::ObjectProfile>,
    object_name: String,
    object_provider: object_storage::Provider,
    object_bucket: String,
    object_prefix: String,
    object_endpoint: String,
    object_key: String,
    object_output: String,
    github_enterprise_host: String,
    update_repository: String,
    update_channel: updater::Channel,
    last_file_poll: Instant,
    pending_editor_history: Option<EditorHistoryCommand>,
    /// The dockable workspace layout. `None` only transiently while its `ui` is
    /// borrowed (it is `take`n so the tree and `self` can be borrowed at once).
    dock_tree: Option<Tree<PaneKind>>,
    /// A pane to bring to the front on the next frame (menu/command navigation).
    dock_focus: Option<PaneKind>,
    /// Last observed `inspector_tab`; when it changes, the matching dock pane is
    /// brought to the front so legacy `inspector_tab = ...` navigation still works.
    last_inspector_tab: InspectorTab,
    /// A tab context-menu action to apply against the tree after layout.
    pending_dock_action: Option<(TileId, DockAction)>,
    /// Panes popped out into floating windows; their tree tiles are hidden while
    /// they float, and shown again when docked back.
    floating_panes: Vec<PaneKind>,
    /// Live terminal sessions keyed by their pane id, spawned lazily on first show.
    terminals: HashMap<u32, terminal::Terminal>,
    /// A request to create a new terminal, optionally as a sibling of a tile.
    pending_new_terminal: Option<Option<TileId>>,
    /// Independent Rust REPL kernels keyed by pane id, spawned lazily on first show.
    kernels: HashMap<u32, rust_kernel::RustKernel>,
    /// A request to create a new Rust kernel, optionally as a sibling of a tile.
    pending_new_kernel: Option<Option<TileId>>,
    /// Deferred definition-probe offset produced while rendering the editor pane.
    dock_pending_definition_probe: Option<usize>,
    /// Whether a Ctrl+click go-to-definition fired inside the editor pane.
    dock_pending_ctrl_definition: bool,
}

/// Resolve the palette to apply: a named custom theme if one is active and
/// still exists, otherwise the built-in Dark/Light palette per `dark`.
fn resolve_palette(
    active: &Option<String>,
    customs: &[ui::theme::NamedTheme],
    dark: bool,
) -> ui::theme::Palette {
    if let Some(name) = active {
        if let Some(theme) = customs.iter().find(|theme| &theme.name == name) {
            return theme.palette.clone();
        }
        if let Some(theme) = ui::theme::extra_builtin_themes()
            .into_iter()
            .find(|theme| &theme.name == name)
        {
            return theme.palette;
        }
    }
    if dark {
        ui::theme::Palette::dark()
    } else {
        ui::theme::Palette::light()
    }
}

impl ForgeApp {
    /// Whether the workspace/rust-analyzer for the active tab is still loading.
    fn workspace_indexing(&self) -> bool {
        self.project.is_some() && self.active_plain_rust() && !self.lsp_ready
    }

    /// Whether the startup splash overlay should still be shown. It stays up for
    /// a brief minimum, then until the Rust runtime has booted and rust-analyzer
    /// has finished indexing the workspace (bounded by a hard cap), and can be
    /// dismissed early with a click or key press.
    fn splash_active(&self, ctx: &egui::Context) -> bool {
        let Some(start) = self.splash_start else {
            return false;
        };
        let elapsed = start.elapsed().as_secs_f32();
        if elapsed < 0.6 {
            return true;
        }
        // Safety cap so the splash can never hang forever if readiness never
        // arrives. Generous, because a cold-cache index of a large workspace can
        // take a few minutes; the user can always skip with a click or key.
        if elapsed >= 180.0 {
            return false;
        }
        let dismissed = ctx.input(|i| {
            i.pointer.any_pressed()
                || i.key_pressed(egui::Key::Escape)
                || i.key_pressed(egui::Key::Enter)
                || i.key_pressed(egui::Key::Space)
        });
        if dismissed {
            return false;
        }
        matches!(self.run_state, RunState::Booting) || self.workspace_indexing()
    }

    /// Paint the centered startup splash (brand, spinner, live phase, project)
    /// into the full-window `ui`.
    fn draw_splash(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.22).max(28.0));
            if let Some(logo) = &self.splash_logo {
                ui.add(egui::Image::new((logo.id(), egui::vec2(104.0, 104.0))));
                ui.add_space(8.0);
            }
            ui.label(RichText::new("FORGE ML").size(46.0).strong().color(RED));
            ui.label(RichText::new("Rust compute studio").size(14.0).color(MUTED));
            if let Some(name) = self
                .project
                .as_ref()
                .and_then(|project| project.root.file_name())
                .and_then(|name| name.to_str())
            {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("Loading {name}"))
                        .size(12.0)
                        .color(TEXT),
                );
            }
            ui.add_space(26.0);
            ui.add(egui::Spinner::new().size(30.0).color(accent()));
            ui.add_space(12.0);
            let status = if matches!(self.run_state, RunState::Booting) {
                "Starting the Rust runtime…".to_owned()
            } else if self.workspace_indexing() {
                let s = self.lsp_status.replace('\n', " ");
                if s.is_empty() {
                    "Initializing rust-analyzer…".to_owned()
                } else {
                    s
                }
            } else {
                "Ready".to_owned()
            };
            ui.label(RichText::new(status).size(12.0).color(MUTED));
            ui.add_space(6.0);
            ui.label(
                RichText::new("Click or press any key to skip")
                    .size(10.0)
                    .color(MUTED),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!("v{APP_VERSION}"))
                    .size(10.0)
                    .color(MUTED),
            );
        });
    }

    /// Resolve the active theme and apply its palette to the egui context.
    pub(crate) fn apply_theme(&self, ctx: &egui::Context) {
        let palette = resolve_palette(&self.active_theme, &self.custom_themes, self.dark_mode);
        configure_style(ctx, &palette, self.high_contrast);
    }

    /// Snapshot the current workspace (root, files, dock layout, theme, keymap,
    /// connections) under `name`.
    fn capture_workspace(&self, name: String) -> workspace::WorkspaceSnapshot {
        workspace::WorkspaceSnapshot {
            schema: workspace::WORKSPACE_SCHEMA,
            name,
            project_root: self.project.as_ref().map(|project| project.root.clone()),
            open_files: self
                .tabs
                .iter()
                .filter_map(|tab| tab.path.clone())
                .collect(),
            active_file: self
                .tabs
                .get(self.active_tab)
                .and_then(|tab| tab.path.clone()),
            dock_layout: self
                .dock_tree
                .as_ref()
                .and_then(|tree| serde_json::to_string(tree).ok()),
            dark_mode: self.dark_mode,
            high_contrast: self.high_contrast,
            reduced_motion: self.reduced_motion,
            editor_font_size: self.editor_font_size,
            caret_blink: self.caret_blink,
            active_theme: self.active_theme.clone(),
            custom_themes: self.custom_themes.clone(),
            keymap: self.keymap.to_dto(),
            connections: self.database_profiles.clone(),
        }
    }

    /// Restore a workspace snapshot: appearance and theme, key bindings, the
    /// project (and its files), connection profiles, and the dock layout.
    fn apply_workspace(&mut self, snap: workspace::WorkspaceSnapshot, ctx: &egui::Context) {
        self.dark_mode = snap.dark_mode;
        self.high_contrast = snap.high_contrast;
        self.reduced_motion = snap.reduced_motion;
        self.editor_font_size = snap.editor_font_size.clamp(10.0, 24.0);
        self.caret_blink = snap.caret_blink;
        self.custom_themes = snap.custom_themes;
        self.active_theme = snap.active_theme;
        self.theme_draft = resolve_palette(&self.active_theme, &self.custom_themes, self.dark_mode);
        self.keymap = keymap::Keymap::from_dto(&snap.keymap);

        if let Some(root) = snap.project_root.clone() {
            if root.is_dir() {
                self.open_project_path(root);
            }
        }
        // Snapshot connections take precedence over the project's stored set and
        // are persisted into the (now open) project's store.
        if !snap.connections.is_empty() {
            self.database_profiles = snap.connections;
            self.database_selected = 0;
            if let Some(store) = &self.workspace_store {
                let _ = store.save_connections(&self.database_profiles);
            }
        }

        if !snap.open_files.is_empty() {
            self.tabs.clear();
            for path in &snap.open_files {
                if path.is_file() {
                    self.open_file(path.clone());
                }
            }
            if self.tabs.is_empty() {
                self.tabs.push(welcome_tab());
            }
            self.active_tab = snap
                .active_file
                .and_then(|active| {
                    self.tabs
                        .iter()
                        .position(|tab| tab.path.as_ref() == Some(&active))
                })
                .unwrap_or(0)
                .min(self.tabs.len().saturating_sub(1));
        }

        self.dock_tree = Some(load_dock_tree(snap.dock_layout.as_deref()));
        self.last_lsp_hash = 0;
        self.apply_theme(ctx);
        self.console = if snap.name.is_empty() {
            "Loaded workspace.".to_owned()
        } else {
            format!("Loaded workspace '{}'.", snap.name)
        };
    }

    /// Write the current workspace to a `.json` file chosen by the user.
    fn save_workspace_as(&mut self) {
        let default_name = self
            .project
            .as_ref()
            .and_then(|project| project.root.file_name().and_then(|n| n.to_str()))
            .unwrap_or("workspace")
            .to_owned();
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Forge workspace", &["json"])
            .set_file_name(format!("{default_name}.forge-workspace.json"))
            .save_file()
        else {
            return;
        };
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("workspace")
            .trim_end_matches(".forge-workspace")
            .to_owned();
        let snap = self.capture_workspace(name);
        self.console = match serde_json::to_string_pretty(&snap)
            .map_err(|e| e.to_string())
            .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()))
        {
            Ok(()) => format!("Saved workspace to {}", path.display()),
            Err(error) => format!("Could not save workspace: {error}"),
        };
    }

    /// Load a workspace from a `.json` file chosen by the user.
    fn open_workspace(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Forge workspace", &["json"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                serde_json::from_str::<workspace::WorkspaceSnapshot>(&text)
                    .map_err(|e| e.to_string())
            }) {
            Ok(snap) => self.apply_workspace(snap, ctx),
            Err(error) => self.console = format!("Could not open workspace: {error}"),
        }
    }
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
        let custom_themes = session.custom_themes.clone();
        let active_theme = session.active_theme.clone();
        let theme_palette = resolve_palette(&active_theme, &custom_themes, session.dark_mode);
        configure_style(&cc.egui_ctx, &theme_palette, session.high_contrast);
        let ui_scale = session.ui_scale.clamp(0.7, 2.0);
        cc.egui_ctx.set_zoom_factor(ui_scale);
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
        let database_profiles = workspace_store
            .as_ref()
            .and_then(|store| store.load_connections().ok())
            .unwrap_or_default();
        let sql_history = database::bounded_query_history(
            workspace_store
                .as_ref()
                .and_then(|store| store.load_query_history().ok())
                .unwrap_or_default(),
        );
        let remote_profiles = workspace_store
            .as_ref()
            .and_then(|store| store.load_remote_profiles().ok())
            .unwrap_or_default();
        privacy_diagnostics::configure(
            session.diagnostics_opt_in,
            project.as_ref().map(|p| p.root.as_path()),
        );
        let object_profiles = workspace_store
            .as_ref()
            .and_then(|store| store.load_object_profiles().ok())
            .unwrap_or_default();
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
        let native_burn_artifact = session.validated_native_artifact();
        let drift_policy = session.validated_drift_policy();
        let native_burn_inference_feature = if session.native_inference_feature.is_finite() {
            session.native_inference_feature
        } else {
            0.0
        };
        let native_training_config = session.validated_native_training_config();
        let (native_training_feature, native_training_target) =
            session.validated_training_columns();
        let app = Self {
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
            structured_plots: session::bounded_plots(&session.structured_plots),
            open_dataset: None,
            dataset_views: HashMap::new(),
            dataset_viewer_docked,
            dataset_pane_height,
            inspector_tab: InspectorTab::Variables,
            diagnostics: DiagnosticsHandle::spawn(),
            diagnostic_lines: vec!["Run diagnostics to check the current Cargo project.".to_owned()],
            diagnostics_running: false,
            execution_count: 0,
            console_input: String::new(),
            history: Vec::new(),
            lsp: LspHandle::spawn(),
            lsp_status: "rust-analyzer waiting for a Rust file.".to_owned(),
            lsp_diagnostics: HashMap::new(),
            completions: Vec::new(),
            lsp_references: Vec::new(),
            lsp_signature: String::new(),
            rename_open: false,
            rename_input: String::new(),
            go_to_line_open: false,
            go_to_line_input: String::new(),
            code_actions: Vec::new(),
            hover_text: String::new(),
            cursor_offset: 0,
            document_version: 1,
            last_lsp_hash: 0,
            hover_probe_offset: None,
            navigable_hover_offset: None,
            definition_probe_pending: false,
            dark_mode,
            theme_draft: theme_palette.clone(),
            active_theme,
            custom_themes,
            theme_new_name: String::new(),
            theme_dirty: false,
            ui_scale,
            splash_start: Some(Instant::now()),
            splash_logo: load_splash_logo(&cc.egui_ctx),
            lsp_ready: false,
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
            format_on_save: session.format_on_save,
            keymap: keymap::Keymap::from_dto(&session.keymap),
            rebinding: None,
            welcome_open: session.show_welcome,
            high_contrast: session.high_contrast,
            lsp_enabled: session.lsp_enabled,
            reduced_motion: session.reduced_motion,
            command_palette_open: false,
            command_query: String::new(),
            command_selection: 0,
            recent_commands: Vec::new(),
            status_announcement: "Forge ML ready".into(),
            diagnostics_opt_in: session.diagnostics_opt_in,
            completion_popup_open: false,
            git_output: String::new(),
            git_conflicts: Vec::new(),
            git_branches: Vec::new(),
            git_selected_branch: None,
            git_commit_message: String::new(),
            git_branch_name: String::new(),
            package_query: String::new(),
            package_output: String::new(),
            cargo_registry: String::new(),
            python_registry: "https://pypi.org".into(),
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
            integration_worker: IntegrationWorker::new(),
            integration_pending: 0,
            job_command: "cargo run --release".into(),
            database_profiles,
            database_selected: 0,
            database_name: "local".into(),
            database_kind: ConnectionKind::SQLite,
            database_location: "data.sqlite3".into(),
            database_username: String::new(),
            database_secret: String::new(),
            sql_editor: "SELECT 1 AS value;".into(),
            sql_output: String::new(),
            sql_history,
            deep_backend: session.native_training_backend,
            burn_training_cancel: None,
            burn_training_epochs: native_training_config.epochs,
            burn_training_learning_rate: native_training_config.learning_rate,
            burn_training_validation_fraction: native_training_config.validation_fraction,
            burn_training_use_dataset: session.native_training_use_dataset,
            burn_training_feature: native_training_feature,
            burn_training_target: native_training_target,
            class_features: String::new(),
            class_target: String::new(),
            class_epochs: 300,
            class_lr: 0.5,
            class_test_fraction: 0.25,
            class_result: String::new(),
            class_model: None,
            class_playground: Vec::new(),
            prep_categorical: String::new(),
            prep_encoding: prep::Encoding::OneHot,
            prep_missing: prep::Missing::Mean,
            prep_scaling: prep::Scaling::None,
            prep_result: String::new(),
            onnx_model: None,
            onnx_model_name: String::new(),
            onnx_input: String::new(),
            onnx_result: String::new(),
            native_burn_artifact,
            drift_mean_shift_threshold: drift_policy.mean_shift_threshold,
            drift_scale_ratio_lower: drift_policy.scale_ratio_lower,
            drift_scale_ratio_upper: drift_policy.scale_ratio_upper,
            native_burn_inference_feature,
            deep_outputs: DeepOutputs::default(),
            resource_system: sysinfo::System::new_all(),
            resource_snapshot: ResourceSnapshot::default(),
            last_resource_poll: Instant::now(),
            early_stopping_patience: native_training_config.early_stopping_patience,
            resume_checkpoint: String::new(),
            remote_profiles,
            remote_name: "remote".into(),
            remote_url: String::new(),
            remote_command: "cargo run --release".into(),
            remote_token: String::new(),
            remote_kernel_name: "python3".into(),
            remote_kernel_session: None,
            remote_code: "print(\"hello from Forge ML\")".into(),
            remote_mime_outputs: Vec::new(),
            remote_execution_pending: false,
            remote_interrupt_pending: false,
            remote_notebook_execution: false,
            remote_input_sender: None,
            remote_input_prompt: None,
            remote_input_response: String::new(),
            remote_input_password: false,
            registry_model: "model".into(),
            registry_version: "0.1.0".into(),
            registry_format: "onnx".into(),
            registry_alias: "production".into(),
            registry_artifact: String::new(),
            registry_output: String::new(),
            service_events: Vec::new(),
            drift_events: Vec::new(),
            object_profiles,
            object_name: "datasets".into(),
            object_provider: object_storage::Provider::S3,
            object_bucket: String::new(),
            object_prefix: String::new(),
            object_endpoint: String::new(),
            object_key: String::new(),
            object_output: String::new(),
            github_enterprise_host: String::new(),
            update_repository: "mi7plus/forge-ml".into(),
            update_channel: updater::Channel::Stable,
            last_file_poll: Instant::now(),
            pending_editor_history: None,
            dock_tree: Some(load_dock_tree(session.dock_layout.as_deref())),
            dock_focus: None,
            pending_dock_action: None,
            floating_panes: Vec::new(),
            terminals: HashMap::new(),
            pending_new_terminal: None,
            kernels: HashMap::new(),
            pending_new_kernel: None,
            last_inspector_tab: InspectorTab::Variables,
            dock_pending_definition_probe: None,
            dock_pending_ctrl_definition: false,
        };
        // Honor the persisted rust-analyzer preference (default on).
        if !app.lsp_enabled {
            app.lsp.set_enabled(false);
        }
        app
    }

    fn active(&self) -> &EditorTab {
        &self.tabs[self.active_tab]
    }
    fn active_mut(&mut self) -> &mut EditorTab {
        &mut self.tabs[self.active_tab]
    }

    /// A compact bottom strip summarizing runtime, file, and language-server
    /// state so those signals live in one predictable place.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        use egui_phosphor_icons::icons;
        ui.horizontal(|ui| {
            let (glyph, text, color) = run_state_chip(self.run_state);
            ui.label(
                RichText::new(format!("{glyph}  {text}"))
                    .color(color)
                    .size(11.0),
            );
            ui.separator();

            let dirty = self.active().dirty;
            let name = self
                .active()
                .path
                .as_ref()
                .map(|path| file_title(path))
                .unwrap_or_else(|| "untitled".to_owned());
            if ui
                .add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{}  {}{}",
                            icons::FILE_CODE.as_str(),
                            name,
                            if dirty { " •" } else { "" }
                        ))
                        .color(if dirty { EMBER } else { MUTED })
                        .size(11.0),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_text("Reveal in the Files pane")
                .clicked()
            {
                self.dock_focus = Some(PaneKind::Files);
            }

            if self.integration_pending > 0 {
                ui.separator();
                if ui
                    .add(
                        egui::Label::new(
                            RichText::new(format!(
                                "{}  {} background task(s)",
                                icons::HOURGLASS_MEDIUM.as_str(),
                                self.integration_pending
                            ))
                            .color(accent())
                            .size(11.0),
                        )
                        .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Show the Problems pane")
                    .clicked()
                {
                    self.inspector_tab = InspectorTab::Problems;
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let full = self.lsp_status.replace('\n', " ");
                // egui truncates to the available width with an ellipsis; the
                // full text is always available on hover. Click restarts the
                // language server (forces a fresh sync).
                let response = ui
                    .add(
                        egui::Label::new(RichText::new(&full).size(11.0).color(MUTED))
                            .truncate()
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text(format!("{full}\n(click to restart rust-analyzer)"));
                if response.clicked() {
                    self.last_lsp_hash = 0;
                    self.lsp_status = "Restarting rust-analyzer…".to_owned();
                }
            });
        });
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

    /// Move the editor caret to the start of the `index`-th cell and scroll it
    /// into view. The inverse of [`Self::select_cell_from_caret`]: clicking a
    /// cell in the rail jumps the editor to that cell.
    fn focus_cell_in_editor(&mut self, index: usize) {
        let offset = {
            let content = &self.active().content;
            let ranges = cell_byte_ranges(content);
            let byte = ranges.get(index).map(|range| range.start).unwrap_or(0);
            content[..byte].chars().count()
        };
        self.pending_editor_selection = Some((offset, offset));
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
        if self.remote_notebook_execution {
            self.console =
                "Remote kernel restart is not available; running all cells in the active session."
                    .into();
            self.enqueue_cells(0..self.cells().len());
            return;
        }
        self.run_queue.clear();
        self.run_all_after_reset = true;
        self.run_state = RunState::Booting;
        self.console = "Restarting runtime before running all cells...".to_owned();
        let _ = self.runtime.reset();
    }

    /// Reset the Evcxr runtime to a clean session without running anything —
    /// the "Restart runtime" command. Clears the pending queue and forgets all
    /// live variables/state.
    fn restart_runtime(&mut self) {
        if self.remote_notebook_execution {
            self.console = "Remote kernel restart is not available.".into();
            return;
        }
        self.run_queue.clear();
        self.run_all_after_reset = false;
        self.run_state = RunState::Booting;
        self.console = "Restarting runtime…".to_owned();
        let _ = self.runtime.reset();
    }

    /// Toggle a `// ` line comment on the line under the caret (Ctrl+/). The
    /// string transformation lives in [`ui::editing::toggle_line_comment`] so it
    /// can be unit-tested; this just applies the result to the active buffer.
    fn toggle_line_comment(&mut self) {
        let (new_content, new_cursor) =
            ui::editing::toggle_line_comment(&self.active().content, self.cursor_offset);
        self.active_mut().content = new_content;
        self.active_mut().dirty = true;
        self.cursor_offset = new_cursor;
        self.pending_editor_selection = Some((new_cursor, new_cursor));
        self.cell_records.clear();
    }

    fn stop_execution(&mut self) {
        self.run_queue.clear();
        self.run_all_after_reset = false;
        if self.remote_execution_pending {
            let Some(session) = self.remote_kernel_session.clone() else {
                self.run_state = RunState::Failed;
                self.console = "The active remote kernel is no longer available.".into();
                return;
            };
            if self.remote_interrupt_pending {
                self.console = "Remote interrupt already requested…".into();
                return;
            }
            match self
                .integration_worker
                .submit(IntegrationRequest::RemoteKernelInterrupt(session))
            {
                Ok(()) => {
                    self.integration_pending += 1;
                    self.remote_interrupt_pending = true;
                    self.remote_input_sender = None;
                    self.remote_input_prompt = None;
                    self.remote_input_response.clear();
                    self.console = "Interrupting remote notebook execution…".into();
                }
                Err(error) => {
                    self.run_state = RunState::Failed;
                    self.console = error;
                }
            }
            return;
        }
        self.run_state = RunState::Booting;
        self.console = "Stopping execution and restarting the Rust runtime...".to_owned();
        if let Err(error) = self.runtime.stop() {
            self.run_state = RunState::Failed;
            self.console = format!("Could not stop execution: {error}");
        }
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
            self.data.source_fingerprints(),
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
        while let Some(event) = self.integration_worker.try_recv() {
            if !matches!(
                &event,
                ResultEvent::RemoteInputRequested { .. }
                    | ResultEvent::DataImportProgress { .. }
                    | ResultEvent::BurnTrainingProgress(_)
            ) {
                self.integration_pending = self.integration_pending.saturating_sub(1);
            }
            match event {
                ResultEvent::BurnTrainingProgress(event) => {
                    millwright_studio::record_training_event(&mut self.training_events, event);
                    self.inspector_tab = InspectorTab::Studio;
                }
                ResultEvent::BurnTrainingFinished(result) => {
                    self.burn_training_cancel = None;
                    self.sql_output = result
                        .map(|outcome| {
                            let count = outcome.events.len();
                            self.native_burn_artifact = Some(outcome.artifact);
                            format!(
                                "Completed embedded Burn training and recorded {count} typed event(s)."
                            )
                        })
                        .unwrap_or_else(|error| format!("Embedded Burn training failed: {error}"));
                }
                ResultEvent::NativeRegressionPredicted(result) => match result {
                    Ok((name, dataset, predicted, diagnostics, drift)) => {
                        let rows = dataset.rows.len();
                        self.data.insert_dataset(name.clone(), dataset);
                        self.open_dataset = Some(format!("table:{name}"));
                        let mut message = format!(
                            "Predicted {predicted} of {rows} row(s) into dataset `{name}`."
                        );
                        message.push_str(&format!(
                            " Feature drift over {} value(s): mean shift {:.3}σ, scale ratio {:.3}{}.",
                            drift.observed,
                            drift.standardized_mean_shift,
                            drift.scale_ratio,
                            if drift.breached { " (threshold breached)" } else { "" }
                        ));
                        service_monitor::record_drift(
                            &mut self.drift_events,
                            DriftEvent {
                                model: drift.model,
                                version: drift.version,
                                feature: drift.feature,
                                score: drift.score,
                                threshold: 1.0,
                                observed: Some(drift.observed),
                                standardized_mean_shift: Some(drift.standardized_mean_shift),
                                scale_ratio: Some(drift.scale_ratio),
                                mean_shift_threshold: Some(drift.policy.mean_shift_threshold),
                                scale_ratio_lower: Some(drift.policy.scale_ratio_lower),
                                scale_ratio_upper: Some(drift.policy.scale_ratio_upper),
                            },
                        );
                        if let Some(diagnostics) = diagnostics {
                            message.push_str(&format!(
                                " Evaluated {} row(s): MAE {:.6}, RMSE {:.6}, R² {}.",
                                diagnostics.evaluated,
                                diagnostics.mae,
                                diagnostics.rmse,
                                diagnostics
                                    .r_squared
                                    .map(|value| format!("{value:.6}"))
                                    .unwrap_or_else(|| "unavailable".into())
                            ));
                            for spec in diagnostics.plots(&name) {
                                self.structured_plots
                                    .retain(|existing| existing.name != spec.name);
                                self.structured_plots.push(spec);
                            }
                            self.inspector_tab = InspectorTab::Charts;
                        } else {
                            self.inspector_tab = InspectorTab::Data;
                        }
                        self.sql_output = message;
                    }
                    Err(error) => {
                        self.sql_output = format!("Native batch inference failed: {error}")
                    }
                },
                ResultEvent::DataImportProgress { path, rows } => {
                    self.console = format!("Importing {}… {rows} rows decoded", path.display());
                }
                ResultEvent::DataImport { path, result } => match result {
                    Ok((dataset_name, dataset)) => {
                        let rows = dataset.rows.len();
                        let columns = dataset.columns.len();
                        self.data.insert_dataset(dataset_name.clone(), dataset);
                        self.open_dataset = Some(format!("table:{dataset_name}"));
                        self.inspector_tab = InspectorTab::Data;
                        self.console = format!(
                            "Imported {} as `{dataset_name}` ({rows} rows × {columns} columns).",
                            path.display()
                        );
                    }
                    Err(error) => {
                        self.console = format!("Could not import {}: {error}", path.display())
                    }
                },
                ResultEvent::DataExport { name, path, result } => {
                    self.console = result
                        .map(|()| format!("Exported `{name}` to {}.", path.display()))
                        .unwrap_or_else(|error| format!("Dataset export failed: {error}"));
                }
                ResultEvent::DatabaseMessage(result) => {
                    self.sql_output = result.unwrap_or_else(|error| error);
                }
                ResultEvent::DatabaseTable {
                    dataset_name,
                    query,
                    result,
                } => match result {
                    Ok(dataset) => {
                        let rows = dataset.rows.len();
                        self.data.insert_dataset(dataset_name.clone(), dataset);
                        self.open_dataset = Some(format!("table:{dataset_name}"));
                        if let Some(query) = query {
                            database::record_query(&mut self.sql_history, query);
                            if let Some(store) = &self.workspace_store {
                                let _ = store.save_query_history(&self.sql_history);
                            }
                        }
                        self.sql_output = format!(
                            "Loaded {rows} rows into Arrow-backed dataset `{dataset_name}`."
                        );
                    }
                    Err(error) => self.sql_output = error,
                },
                ResultEvent::ObjectMessage(result) => {
                    self.object_output = result.unwrap_or_else(|error| error);
                }
                ResultEvent::ObjectDownload(result) => {
                    self.object_output = result
                        .map(|path| format!("Downloaded {}", path.display()))
                        .unwrap_or_else(|error| error);
                }
                ResultEvent::RemoteMessage(result) => {
                    self.sql_output = result.unwrap_or_else(|error| error);
                }
                ResultEvent::RemoteKernelStarted(result) => match result {
                    Ok(session) => {
                        self.sql_output = format!(
                            "Started remote kernel `{}` ({}) on `{}`.",
                            session.name, session.id, session.profile.name
                        );
                        self.remote_kernel_session = Some(session);
                    }
                    Err(error) => self.sql_output = error,
                },
                ResultEvent::RemoteKernelStopped(result) => match result {
                    Ok(message) => {
                        self.remote_kernel_session = None;
                        self.remote_notebook_execution = false;
                        self.sql_output = message;
                    }
                    Err(error) => self.sql_output = error,
                },
                ResultEvent::RemoteKernelInterrupted(result) => {
                    self.remote_interrupt_pending = false;
                    self.sql_output = result.unwrap_or_else(|error| error);
                }
                ResultEvent::RemoteInputRequested {
                    cell_id,
                    prompt,
                    password,
                } => {
                    self.remote_input_prompt = Some(if let Some(cell_id) = cell_id {
                        format!("Cell {}: {prompt}", cell_id + 1)
                    } else {
                        prompt
                    });
                    self.remote_input_password = password;
                    self.remote_input_response.clear();
                }
                ResultEvent::RemoteExecuted { cell_id, result } => {
                    self.remote_execution_pending = false;
                    self.remote_input_sender = None;
                    self.remote_input_prompt = None;
                    self.remote_input_response.clear();
                    self.remote_input_password = false;
                    match result {
                        Ok(execution) => {
                            let message = format!(
                                "Remote execution {}{}\n{}",
                                execution.status,
                                execution
                                    .execution_count
                                    .map(|count| format!(" · In [{count}]"))
                                    .unwrap_or_default(),
                                execution.output
                            );
                            if let Some(cell_id) = cell_id {
                                let succeeded = execution.status == "ok";
                                let record = self.cell_records.entry(cell_id).or_default();
                                record.state = Some(if succeeded {
                                    CellState::Passed
                                } else {
                                    CellState::Failed
                                });
                                record.output = execution.output;
                                record.rich_outputs = execution.mime;
                                self.console = message;
                                if succeeded {
                                    self.run_state = RunState::Ready;
                                    self.execution_count += 1;
                                    self.run_next();
                                } else {
                                    self.run_state = RunState::Failed;
                                    self.run_queue.clear();
                                }
                            } else {
                                self.remote_mime_outputs = execution.mime;
                                self.sql_output = message;
                            }
                        }
                        Err(error) => {
                            if let Some(cell_id) = cell_id {
                                let record = self.cell_records.entry(cell_id).or_default();
                                record.state = Some(CellState::Failed);
                                record.output = error.clone();
                                self.run_state = RunState::Failed;
                                self.run_queue.clear();
                                self.console = format!("Cell {} failed\n\n{error}", cell_id + 1);
                            } else {
                                self.sql_output = error;
                            }
                        }
                    }
                }
            }
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
        ctx.request_repaint_after(Duration::from_millis(if self.integration_pending > 0 {
            100
        } else {
            750
        }));
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
                        millwright_studio::record_training_event(&mut self.training_events, event);
                    }
                    if let Some(report) = reports.into_iter().last() {
                        self.evaluation_report = report;
                    }
                    let (service_events, drift_events) =
                        service_monitor::parse_runtime_output(&self.console);
                    for event in service_events {
                        service_monitor::record_service(&mut self.service_events, event);
                    }
                    for event in drift_events {
                        service_monitor::record_drift(&mut self.drift_events, event);
                    }
                    deep_learning::parse_output(&self.console, &mut self.deep_outputs);
                    for spec in plot::parse_output(&self.console) {
                        self.structured_plots
                            .retain(|existing| existing.name != spec.name);
                        if cell_id != CONSOLE_CELL_ID {
                            let record = self.cell_records.entry(cell_id).or_default();
                            if !record.plots.contains(&spec.name) {
                                record.plots.push(spec.name.clone());
                            }
                        }
                        self.structured_plots.push(spec);
                    }
                    for envelope in events {
                        if cell_id != CONSOLE_CELL_ID {
                            let dataset_ref = match &envelope.event {
                                forge_protocol::ForgeEvent::Table { name, .. } => {
                                    Some(format!("table:{name}"))
                                }
                                forge_protocol::ForgeEvent::Vector { name, .. } => {
                                    Some(format!("vector:{name}"))
                                }
                                forge_protocol::ForgeEvent::Metric { .. } => None,
                            };
                            if let Some(dataset_ref) = dataset_ref {
                                let record = self.cell_records.entry(cell_id).or_default();
                                if !record.datasets.contains(&dataset_ref) {
                                    record.datasets.push(dataset_ref);
                                }
                            }
                        }
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
                CellResult::Progress { cell_id, line } => {
                    // Live build/compile output while a cell runs (e.g. a long
                    // `:dep` build). Shown transiently; replaced by the final
                    // output on Success/Error.
                    if cell_id != CONSOLE_CELL_ID {
                        let record = self.cell_records.entry(cell_id).or_default();
                        if !record.output.is_empty() {
                            record.output.push('\n');
                        }
                        record.output.push_str(&line);
                    }
                    self.console = line;
                    ctx.request_repaint();
                }
                CellResult::Reset => {
                    self.run_state = RunState::Ready;
                    self.execution_count = 0;
                    self.variables.clear();
                    self.data.clear();
                    self.structured_plots.clear();
                    self.open_dataset = None;
                    self.cell_records.clear();
                    self.console = "Runtime state cleared.".to_owned();
                    if std::mem::take(&mut self.run_all_after_reset) {
                        self.enqueue_cells(0..self.cells().len());
                    }
                }
                CellResult::RuntimeError(message) => {
                    let _ = privacy_diagnostics::record("rust_runtime_error");
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
                LspEvent::Status(status) => {
                    // Only the authoritative "rust-analyzer ready ✓" marker
                    // dismisses the splash — not looser phrases like the
                    // post-install "Language services are ready", which is sent
                    // before indexing begins.
                    if status.contains("ready ✓") {
                        self.lsp_ready = true;
                    }
                    self.lsp_status = status;
                }
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
                LspEvent::References(references) => {
                    let count = references.len();
                    self.lsp_references = references;
                    self.inspector_tab = InspectorTab::Search;
                    self.lsp_status = format!("{count} reference(s) found.");
                }
                LspEvent::Signature(signature) => {
                    self.lsp_signature = signature;
                }
                LspEvent::WorkspaceEdit(files) => {
                    if files.is_empty() {
                        self.lsp_status = "Nothing to rename here.".to_owned();
                    } else {
                        self.apply_file_edits(files);
                    }
                }
                LspEvent::CodeActions(actions) => {
                    if actions.is_empty() {
                        self.lsp_status = "No code actions at the cursor.".to_owned();
                    } else {
                        self.lsp_status = format!("{} code action(s) available.", actions.len());
                    }
                    self.code_actions = actions;
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

    /// Render one inspector pane's body, hosted by a [`PaneKind::Inspector`] tile.
    fn inspector_body(&mut self, tab: InspectorTab, ui: &mut egui::Ui) {
        match tab {
            InspectorTab::Variables => {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("LIVE EVCXR STATE")
                            .size(10.0)
                            .strong()
                            .color(MUTED),
                    );
                    ui.label(
                        RichText::new("· click 🔍 to explore a value")
                            .size(10.0)
                            .color(MUTED),
                    );
                });
                let mut inspect: Option<(String, String)> = None;
                egui::Grid::new("variables_table")
                    .striped(true)
                    .min_col_width(72.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Name").strong());
                        ui.label(RichText::new("Type").strong());
                        ui.label(RichText::new("Size").strong());
                        ui.label("");
                        ui.end_row();
                        for variable in &self.variables {
                            ui.label(RichText::new(&variable.name).color(accent()));
                            ui.label(RichText::new(&variable.type_name).monospace().size(10.0));
                            ui.label(
                                RichText::new(infer_size(&variable.type_name))
                                    .monospace()
                                    .size(10.0)
                                    .color(MUTED),
                            );
                            if ui
                                .small_button("🔍")
                                .on_hover_text(
                                    "Inspect: show this value in the Data viewer (or the console)",
                                )
                                .clicked()
                            {
                                inspect = Some((variable.name.clone(), variable.type_name.clone()));
                            }
                            ui.end_row();
                        }
                    });
                if let Some((name, type_name)) = inspect {
                    self.inspect_variable(&name, &type_name);
                }
                if self.variables.is_empty() {
                    ui::theme::empty_state(
                        ui,
                        egui_phosphor_icons::icons::CUBE,
                        "No variables yet",
                        "Run a cell to inspect the live Evcxr state.",
                    );
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
                    ui.label(RichText::new("Hover").strong().color(accent()));
                    egui::ScrollArea::vertical()
                        .id_salt("help_hover_documentation")
                        .max_height(150.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(&self.hover_text).monospace().size(10.0));
                        });
                }
                if !self.completions.is_empty() {
                    ui.separator();
                    ui.label(RichText::new("Completions").strong().color(accent()));
                    let mut selected = None;
                    egui::ScrollArea::vertical()
                        .id_salt("help_completion_results")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for (label, insert) in &self.completions {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(label).monospace().size(10.0),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text("Insert completion")
                                    .clicked()
                                {
                                    selected = Some(insert.clone());
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
                ui.label(RichText::new("Telemetry").strong().color(accent()));
                ui.code("println!(\"forge_metric:loss={}\", loss);\nprintln!(\"forge_vector:w=1,2,3\");\nprintln!(r#\"forge_table:samples={{\\\"columns\\\":[\\\"x\\\",\\\"label\\\"],\\\"rows\\\":[[1.0,\\\"cat\\\"]]}}\"#);");
                ui.separator();
                ui.label(RichText::new("Shortcuts").strong().color(accent()));
                ui.label("Shift+Enter  Run cell\nCtrl+Shift+Enter  Run all\nCtrl+Space  Show completions\nCtrl+S  Save file\nCtrl+N  New file\nCtrl+F  Find and replace\nCtrl+Shift+F  Find in project");
            }
            InspectorTab::Problems => {
                let has_problems =
                    !self.lsp_diagnostics.is_empty() || !self.diagnostic_lines.is_empty();
                ui.horizontal(|ui| {
                    use egui_phosphor_icons::icons;
                    if compact_icon_button(ui, icons::CHECK_CIRCLE, "Re-run cargo check").clicked()
                    {
                        self.run_diagnostics();
                    }
                    if compact_icon_button(ui, icons::BUG, "Run clippy").clicked() {
                        self.run_clippy();
                    }
                    if ui::theme::enabled_compact_icon_button(
                        ui,
                        has_problems,
                        icons::BROOM,
                        "Clear problems",
                    )
                    .clicked()
                    {
                        self.lsp_diagnostics.clear();
                        self.diagnostic_lines.clear();
                    }
                });
                if !has_problems {
                    ui::theme::empty_state(
                        ui,
                        egui_phosphor_icons::icons::CHECK_CIRCLE,
                        "No problems",
                        "cargo check and rust-analyzer found no issues.",
                    );
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
            InspectorTab::Database => self.database_inspector(ui),
            InspectorTab::DeepLearning => self.deep_learning_inspector(ui),
            InspectorTab::Deploy => self.deployment_inspector(ui),
            InspectorTab::Storage => self.object_storage_inspector(ui),
        }
    }

    fn project_root(&self) -> Option<PathBuf> {
        self.project.as_ref().map(|project| project.root.clone())
    }
}

impl ForgeApp {
    /// Find the tile hosting a given pane, if it is present in the tree.
    fn dock_tile_of(tree: &Tree<PaneKind>, kind: PaneKind) -> Option<TileId> {
        tree.tiles.iter().find_map(|(id, tile)| match tile {
            Tile::Pane(pane) if *pane == kind => Some(*id),
            _ => None,
        })
    }
}

impl egui_tiles::Behavior<PaneKind> for ForgeApp {
    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: TileId,
        pane: &mut PaneKind,
    ) -> egui_tiles::UiResponse {
        ui.add_space(2.0);
        self.dock_pane_body(*pane, ui);
        egui_tiles::UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &PaneKind) -> egui::WidgetText {
        // Terminal tabs reflect the shell's live OSC title, or a numbered
        // fallback so multiple terminals stay distinguishable.
        if let PaneKind::Terminal(id) = *pane {
            let live = self
                .terminals
                .get(&id)
                .map(|t| t.title())
                .unwrap_or("Terminal");
            let label = if live == "Terminal" {
                format!("Terminal {id}")
            } else {
                live.to_string()
            };
            return format!("{}  {}", pane.icon(), label).into();
        }
        if let PaneKind::RustConsole(id) = *pane {
            return format!("{}  Rust {id}", pane.icon()).into();
        }
        pane.tab_label().into()
    }

    /// Right-click a tab for hide / undock actions. The tree isn't available
    /// here, so the chosen action is recorded and applied after layout.
    fn on_tab_button(
        &mut self,
        tiles: &mut Tiles<PaneKind>,
        tile_id: TileId,
        button_response: egui::Response,
    ) -> egui::Response {
        let kind = tiles.get_pane(&tile_id).copied();
        button_response.context_menu(|ui| {
            ui.label(
                RichText::new(kind.map(|k| k.title()).unwrap_or("Pane"))
                    .strong()
                    .color(MUTED),
            );
            if kind == Some(PaneKind::DataViewer) {
                // The data viewer has its own floating window mechanism.
                let label = if self.dataset_viewer_docked {
                    "Undock to a floating window"
                } else {
                    "Dock data viewer"
                };
                if ui.button(label).clicked() {
                    self.dataset_viewer_docked = !self.dataset_viewer_docked;
                    ui.close();
                }
            } else if ui
                .button("Undock to a floating window")
                .on_hover_text("Pop this pane out into a movable window")
                .clicked()
            {
                self.pending_dock_action = Some((tile_id, DockAction::Undock));
                ui.close();
            }
            if ui
                .button("Hide pane")
                .on_hover_text("Bring it back from View → Panes")
                .clicked()
            {
                self.pending_dock_action = Some((tile_id, DockAction::Hide));
                ui.close();
            }
            if matches!(kind, Some(PaneKind::Terminal(_))) {
                ui.separator();
                if ui
                    .button("New terminal")
                    .on_hover_text("Open another terminal beside this one")
                    .clicked()
                {
                    self.pending_new_terminal = Some(Some(tile_id));
                    ui.close();
                }
            }
            if matches!(kind, Some(PaneKind::RustConsole(_))) {
                ui.separator();
                if ui
                    .button("New Rust kernel")
                    .on_hover_text("Open another independent Rust kernel beside this one")
                    .clicked()
                {
                    self.pending_new_kernel = Some(Some(tile_id));
                    ui.close();
                }
            }
        });
        button_response
    }

    /// Terminal and Rust-kernel tabs get a close button; the others stay put and
    /// are hidden via the View menu instead.
    fn is_tab_closable(&self, tiles: &Tiles<PaneKind>, tile_id: TileId) -> bool {
        matches!(
            tiles.get_pane(&tile_id),
            Some(PaneKind::Terminal(_)) | Some(PaneKind::RustConsole(_))
        )
    }

    fn on_tab_close(&mut self, tiles: &mut Tiles<PaneKind>, tile_id: TileId) -> bool {
        match tiles.get_pane(&tile_id) {
            // Kill the backing session before the tile is removed.
            Some(PaneKind::Terminal(id)) => {
                self.terminals.remove(id);
            }
            Some(PaneKind::RustConsole(id)) => {
                self.kernels.remove(id);
            }
            _ => {}
        }
        true
    }

    fn simplification_options(&self) -> SimplificationOptions {
        SimplificationOptions {
            // Keep emptied tab groups from vanishing so a hidden pane can be
            // brought back; still allow single-child pruning for tidy splits.
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }
}

/// Build the default dockable workspace: a left navigator, a center column with
/// the editor over a console/data-viewer tab group, and a right inspector tab
/// group holding every inspector pane.
fn build_dock_tree() -> Tree<PaneKind> {
    let mut tiles = Tiles::default();

    let inspector_ids: Vec<TileId> = InspectorTab::ALL
        .iter()
        .map(|tab| tiles.insert_pane(PaneKind::Inspector(*tab)))
        .collect();
    let inspector_group = tiles.insert_tab_tile(inspector_ids);

    // Left column: Files/Outline tabs stacked over the notebook cell rail.
    let files = tiles.insert_pane(PaneKind::Files);
    let outline = tiles.insert_pane(PaneKind::Outline);
    let nav_group = tiles.insert_tab_tile(vec![files, outline]);
    let cells = tiles.insert_pane(PaneKind::Cells);
    let mut left = Linear::new(LinearDir::Vertical, vec![nav_group, cells]);
    left.shares.set_share(nav_group, 0.5);
    left.shares.set_share(cells, 0.5);
    let left = tiles.insert_container(Container::Linear(left));

    // Center: editor over a console-family + data-viewer tab group.
    let editor = tiles.insert_pane(PaneKind::Editor);
    let console = tiles.insert_pane(PaneKind::Console);
    let history = tiles.insert_pane(PaneKind::History);
    let python = tiles.insert_pane(PaneKind::Python);
    let terminal = tiles.insert_pane(PaneKind::Terminal(1));
    let data_viewer = tiles.insert_pane(PaneKind::DataViewer);
    let notebook = tiles.insert_pane(PaneKind::Notebook);
    let bottom = tiles.insert_tab_tile(vec![
        console,
        history,
        python,
        terminal,
        data_viewer,
        notebook,
    ]);

    let mut center = Linear::new(LinearDir::Vertical, vec![editor, bottom]);
    center.shares.set_share(editor, 0.76);
    center.shares.set_share(bottom, 0.24);
    let center = tiles.insert_container(Container::Linear(center));

    let mut root = Linear::new(LinearDir::Horizontal, vec![left, center, inspector_group]);
    root.shares.set_share(left, 0.17);
    root.shares.set_share(center, 0.60);
    root.shares.set_share(inspector_group, 0.23);
    let root = tiles.insert_container(Container::Linear(root));

    Tree::new("forge_dock", root, tiles)
}

/// Where `active` lands after an editor tab is moved from `from` to `to`
/// (remove-then-insert), keeping the active tab pointed at the same tab.
fn reordered_active_index(active: usize, from: usize, to: usize) -> usize {
    if active == from {
        return to;
    }
    let mut adjusted = active;
    if from < adjusted {
        adjusted -= 1;
    }
    if to <= adjusted {
        adjusted += 1;
    }
    adjusted
}

/// The fixed panes the workspace always expects. Terminals are dynamic (zero or
/// more) and handled separately, so they are not listed here.
fn expected_panes() -> Vec<PaneKind> {
    let mut kinds = vec![
        PaneKind::Editor,
        PaneKind::Files,
        PaneKind::Outline,
        PaneKind::Cells,
        PaneKind::Console,
        PaneKind::History,
        PaneKind::Python,
        PaneKind::DataViewer,
    ];
    kinds.extend(
        InspectorTab::ALL
            .iter()
            .map(|tab| PaneKind::Inspector(*tab)),
    );
    kinds
}

/// Restore a saved dock layout, falling back to the default when it is missing,
/// unparseable, or does not contain the panes this build expects. Any number of
/// terminal panes is allowed; every other pane must be a known fixed pane.
fn load_dock_tree(serialized: Option<&str>) -> Tree<PaneKind> {
    if let Some(json) = serialized {
        if let Ok(mut tree) = serde_json::from_str::<Tree<PaneKind>>(json) {
            let present: Vec<PaneKind> = tree
                .tiles
                .iter()
                .filter_map(|(_, tile)| match tile {
                    Tile::Pane(pane) => Some(*pane),
                    _ => None,
                })
                .collect();
            let required = expected_panes();
            let all_required = required.iter().all(|kind| present.contains(kind));
            let no_strangers = present.iter().all(|kind| {
                matches!(
                    kind,
                    PaneKind::Terminal(_) | PaneKind::RustConsole(_) | PaneKind::Notebook
                ) || required.contains(kind)
            });
            if all_required && no_strangers {
                // The Notebook pane was added after some layouts were saved; inject
                // it rather than discarding the user's customized arrangement.
                if !present.contains(&PaneKind::Notebook) {
                    inject_notebook_pane(&mut tree);
                }
                return tree;
            }
        }
    }
    build_dock_tree()
}

/// Add the Notebook pane to a restored tree that predates it, as a tab beside
/// the Console group (falling back to the root), so the layout isn't reset.
fn inject_notebook_pane(tree: &mut Tree<PaneKind>) {
    let new_tile = tree.tiles.insert_pane(PaneKind::Notebook);
    let anchor = tree
        .tiles
        .iter()
        .find_map(|(tid, tile)| match tile {
            Tile::Pane(PaneKind::Console) => Some(*tid),
            _ => None,
        })
        .and_then(|tile| tree.tiles.parent_of(tile))
        .or_else(|| tree.root());
    if let Some(parent) = anchor {
        tree.move_tile_to_container(new_tile, parent, usize::MAX, false);
    }
}

impl eframe::App for ForgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.theme_dirty {
            self.apply_theme(ui.ctx());
            self.theme_dirty = false;
        }
        // Startup splash while the Rust runtime boots; keep background work
        // ticking underneath so it progresses to Ready.
        if self.splash_active(ui.ctx()) {
            self.poll_background(ui.ctx());
            // Kick rust-analyzer so it starts indexing behind the splash.
            self.sync_lsp();
            self.draw_splash(ui);
            ui.ctx().request_repaint();
            return;
        }
        self.splash_start = None;
        self.accessibility_shortcuts(ui.ctx());
        self.command_palette(ui.ctx());
        self.poll_background(ui.ctx());
        // Legacy navigation still assigns `inspector_tab`; when it changes, bring
        // the matching dock pane to the front of its tab group.
        if self.inspector_tab != self.last_inspector_tab {
            self.last_inspector_tab = self.inspector_tab;
            self.dock_focus = Some(PaneKind::Inspector(self.inspector_tab));
        }
        // Shortcut handling runs through the customizable keymap, and is paused
        // while the user is capturing a new binding in Settings.
        use keymap::KeyAction;
        let ctx = ui.ctx().clone();
        let paused = self.rebinding.is_some();
        let save = !paused && self.keymap.triggered(KeyAction::Save, &ctx);
        let new_file = !paused && self.keymap.triggered(KeyAction::NewFile, &ctx);
        let find = !paused && self.keymap.triggered(KeyAction::FindInFile, &ctx);
        let find_in_files = !paused && self.keymap.triggered(KeyAction::FindInProject, &ctx);
        let complete = !paused && self.keymap.triggered(KeyAction::RequestCompletion, &ctx);
        let run = !paused && self.keymap.triggered(KeyAction::RunCell, &ctx);
        let run_all = !paused && self.keymap.triggered(KeyAction::RunAll, &ctx);
        let format_doc = !paused && self.keymap.triggered(KeyAction::FormatDocument, &ctx);
        if save {
            self.save_active();
        }
        if new_file {
            self.create_new_file(None);
        }
        if format_doc {
            self.format_document();
        }
        if find_in_files {
            self.inspector_tab = InspectorTab::Search;
        } else if find {
            self.find_visible = true;
        }
        if complete {
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
        Panel::bottom("status_bar")
            .resizable(false)
            .default_size(24.0)
            .frame(compact_panel_frame(
                theme_colors(self.dark_mode).surface,
                self.dark_mode,
            ))
            .show(ui, |ui| self.status_bar(ui));
        let dock_frame = panel_frame(theme_colors(self.dark_mode).background, self.dark_mode);
        egui::CentralPanel::default()
            .frame(dock_frame)
            .show(ui, |ui| {
                // Take the tree out so both it and `self` (the Behavior) can be
                // borrowed mutably during layout; restore it immediately after.
                let mut tree = self.dock_tree.take().unwrap_or_else(build_dock_tree);
                if let Some(kind) = self.dock_focus.take() {
                    if let Some(id) = Self::dock_tile_of(&tree, kind) {
                        tree.tiles.set_visible(id, true);
                        tree.make_active(|_, tile| matches!(tile, Tile::Pane(p) if *p == kind));
                    }
                }
                tree.ui(self, ui);
                // Apply a tab context-menu action now that the full tree is in hand.
                if let Some((tile, action)) = self.pending_dock_action.take() {
                    match action {
                        DockAction::Hide => tree.tiles.set_visible(tile, false),
                        DockAction::Undock => {
                            // Float: hide the tile and render the pane in a window.
                            if let Some(kind) = tree.tiles.get_pane(&tile).copied() {
                                tree.tiles.set_visible(tile, false);
                                if !self.floating_panes.contains(&kind) {
                                    self.floating_panes.push(kind);
                                }
                            }
                        }
                    }
                }
                if let Some(anchor) = self.pending_new_terminal.take() {
                    let kind = Self::create_terminal(&mut tree, anchor);
                    self.dock_focus = Some(kind);
                }
                if let Some(anchor) = self.pending_new_kernel.take() {
                    let kind = Self::create_kernel(&mut tree, anchor);
                    self.dock_focus = Some(kind);
                }
                self.dock_tree = Some(tree);
            });
        self.after_editor(ui);
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
            format_on_save: self.format_on_save,
            show_welcome: self.welcome_open,
            keymap: self.keymap.to_dto(),
            high_contrast: self.high_contrast,
            lsp_enabled: self.lsp_enabled,
            reduced_motion: self.reduced_motion,
            diagnostics_opt_in: self.diagnostics_opt_in,
            saved_runs: self.saved_runs.clone(),
            experiment_name: self.experiment_name.clone(),
            comparison_metric: self.comparison_metric.clone(),
            dataset_viewer_docked: self.dataset_viewer_docked,
            dataset_pane_height: self.dataset_pane_height,
            selected_python: self.selected_python.clone(),
            selected_jupyter_kernel: self.selected_jupyter_kernel.clone(),
            python_environment_fingerprint: self.python_environment_fingerprint.clone(),
            structured_plots: session::bounded_plots(&self.structured_plots),
            native_regression_artifact: self.native_burn_artifact.clone(),
            native_inference_feature: self.native_burn_inference_feature,
            drift_mean_shift_threshold: self.drift_mean_shift_threshold,
            drift_scale_ratio_lower: self.drift_scale_ratio_lower,
            drift_scale_ratio_upper: self.drift_scale_ratio_upper,
            native_training_backend: self.deep_backend,
            native_training_epochs: self.burn_training_epochs,
            native_training_learning_rate: self.burn_training_learning_rate,
            native_training_validation_fraction: self.burn_training_validation_fraction,
            native_training_patience: self.early_stopping_patience,
            native_training_use_dataset: self.burn_training_use_dataset,
            native_training_feature: self.burn_training_feature.clone(),
            native_training_target: self.burn_training_target.clone(),
            dock_layout: self
                .dock_tree
                .as_ref()
                .and_then(|tree| serde_json::to_string(tree).ok()),
            active_theme: self.active_theme.clone(),
            custom_themes: self.custom_themes.clone(),
            ui_scale: self.ui_scale,
        };
        eframe::set_value(storage, STORAGE_KEY, &state);
    }
}

/// A column-header label with an optional sort caret. The caret glyph is drawn in
/// the Phosphor icon font (the plain Unicode arrows `↑`/`↓` are absent from the UI
/// font and render as tofu boxes), while the label keeps the proportional font.
#[cfg(test)]
mod editor_tests {
    use super::*;
    use ui::editing::lsp_pos_to_offset;
    use ui::grid::ColumnWindow;

    #[test]
    fn tab_reorder_keeps_active_tab_stable() {
        // Verify against the ground truth: apply the move to a labelled vector
        // and check reordered_active_index points at the originally-active label.
        for len in 2..=6usize {
            for from in 0..len {
                for to in 0..len {
                    for active in 0..len {
                        let mut v: Vec<usize> = (0..len).collect();
                        let moved = v.remove(from);
                        v.insert(to, moved);
                        let expected_label = active; // labels equal their start index
                        let new_active = reordered_active_index(active, from, to);
                        assert_eq!(
                            v[new_active], expected_label,
                            "len={len} from={from} to={to} active={active}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn dock_layout_round_trips_and_recovers_from_bad_input() {
        // A serialized default tree restores to a complete, identical layout.
        let original = build_dock_tree();
        let json = serde_json::to_string(&original).unwrap();
        let restored = load_dock_tree(Some(&json));
        let panes = |tree: &Tree<PaneKind>| {
            let mut v: Vec<PaneKind> = tree
                .tiles
                .iter()
                .filter_map(|(_, t)| match t {
                    Tile::Pane(p) => Some(*p),
                    _ => None,
                })
                .collect();
            v.sort_by_key(|p| format!("{p:?}"));
            v
        };
        assert_eq!(panes(&restored), panes(&original));
        // The default tree is the fixed panes, one terminal, and the Notebook
        // pane (kept out of `expected_panes` so older saved layouts still load).
        assert_eq!(panes(&restored).len(), expected_panes().len() + 2);
        assert!(panes(&restored).contains(&PaneKind::Notebook));

        // Missing, unparseable, or incomplete layouts fall back to the default.
        assert_eq!(panes(&load_dock_tree(None)), panes(&original));
        assert_eq!(panes(&load_dock_tree(Some("not json"))), panes(&original));
        let incomplete = Tree::new_tabs("forge_dock", vec![PaneKind::Editor]);
        let incomplete_json = serde_json::to_string(&incomplete).unwrap();
        assert_eq!(
            panes(&load_dock_tree(Some(&incomplete_json))),
            panes(&original)
        );
    }

    #[test]
    fn legacy_layout_without_notebook_has_it_injected_not_reset() {
        // Simulate a saved layout from before the Notebook pane existed: every
        // required pane, but no Notebook. It must be accepted (not reset to the
        // default) with the Notebook pane injected.
        let mut tiles = Tiles::default();
        let mut ids: Vec<TileId> = expected_panes()
            .into_iter()
            .map(|kind| tiles.insert_pane(kind))
            .collect();
        ids.push(tiles.insert_pane(PaneKind::Terminal(1)));
        let root = tiles.insert_tab_tile(ids);
        let legacy = Tree::new("forge_dock", root, tiles);
        let json = serde_json::to_string(&legacy).unwrap();
        let restored = load_dock_tree(Some(&json));
        let has = |tree: &Tree<PaneKind>, kind: PaneKind| {
            tree.tiles
                .iter()
                .any(|(_, t)| matches!(t, Tile::Pane(p) if *p == kind))
        };
        // The user's terminal survived (layout not reset) and Notebook was added.
        assert!(has(&restored, PaneKind::Notebook));
        assert!(has(&restored, PaneKind::Terminal(1)));
    }

    #[test]
    fn new_terminals_get_unique_ids_and_persist() {
        let count_terminals = |tree: &Tree<PaneKind>| {
            tree.tiles
                .iter()
                .filter(|(_, t)| matches!(t, Tile::Pane(PaneKind::Terminal(_))))
                .count()
        };
        let mut tree = build_dock_tree();
        assert_eq!(count_terminals(&tree), 1); // the default terminal, id 1
        assert_eq!(
            ForgeApp::create_terminal(&mut tree, None),
            PaneKind::Terminal(2)
        );
        assert_eq!(
            ForgeApp::create_terminal(&mut tree, None),
            PaneKind::Terminal(3)
        );
        assert_eq!(count_terminals(&tree), 3);

        // A layout with several terminals round-trips and is accepted on load.
        let json = serde_json::to_string(&tree).unwrap();
        let mut restored = load_dock_tree(Some(&json));
        assert_eq!(count_terminals(&restored), 3);
        // Next id keeps climbing past the restored maximum.
        assert_eq!(
            ForgeApp::create_terminal(&mut restored, None),
            PaneKind::Terminal(4)
        );
    }

    #[test]
    fn applies_lsp_text_edits_in_reverse() {
        // Rename `x` -> `total` across two occurrences on lines 0 and 1.
        let mut content = "let x = 1;\nlet y = x + 2;\n".to_owned();
        let edits = vec![
            lsp::TextEdit {
                start_line: 0,
                start_col: 4,
                end_line: 0,
                end_col: 5,
                new_text: "total".to_owned(),
            },
            lsp::TextEdit {
                start_line: 1,
                start_col: 8,
                end_line: 1,
                end_col: 9,
                new_text: "total".to_owned(),
            },
        ];
        apply_edits_to(&mut content, &edits);
        assert_eq!(content, "let total = 1;\nlet y = total + 2;\n");
    }

    #[test]
    fn lsp_position_maps_to_char_offset() {
        let text = "ab\ncde";
        assert_eq!(lsp_pos_to_offset(text, 0, 0), 0);
        assert_eq!(lsp_pos_to_offset(text, 1, 2), 5); // 'e'
        assert_eq!(lsp_pos_to_offset(text, 1, 99), 6); // clamped to line end
    }

    #[test]
    fn rustfmt_formats_rust_source_when_available() {
        let messy = "fn  main( ) {let x=1;println!(\"{}\",x);}\n";
        // Ok → verify formatting; Err → rustfmt not installed here, nothing to check.
        if let Ok(formatted) = run_rustfmt(messy) {
            assert!(formatted.contains("fn main() {"), "got:\n{formatted}");
            assert!(formatted.contains("let x = 1;"), "got:\n{formatted}");
        }
    }

    #[test]
    fn new_rust_kernels_get_unique_ids_and_persist() {
        let count_kernels = |tree: &Tree<PaneKind>| {
            tree.tiles
                .iter()
                .filter(|(_, t)| matches!(t, Tile::Pane(PaneKind::RustConsole(_))))
                .count()
        };
        let mut tree = build_dock_tree();
        assert_eq!(count_kernels(&tree), 0); // none by default
        assert_eq!(
            ForgeApp::create_kernel(&mut tree, None),
            PaneKind::RustConsole(1)
        );
        assert_eq!(
            ForgeApp::create_kernel(&mut tree, None),
            PaneKind::RustConsole(2)
        );
        assert_eq!(count_kernels(&tree), 2);

        // A layout with several kernels round-trips and is accepted on load.
        let json = serde_json::to_string(&tree).unwrap();
        let mut restored = load_dock_tree(Some(&json));
        assert_eq!(count_kernels(&restored), 2);
        assert_eq!(
            ForgeApp::create_kernel(&mut restored, None),
            PaneKind::RustConsole(3)
        );
    }

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

    #[test]
    fn selected_table_projects_rows_and_columns() {
        let table = TableData {
            columns: vec!["a".into(), "b".into(), "c".into()],
            rows: vec![
                vec!["1".into(), "2".into(), "3".into()],
                vec!["4".into(), "5".into(), "6".into()],
            ],
        };
        let selected = [1usize].into_iter().collect();
        let projected = selected_table(&table, &selected, &[2, 0]);
        assert_eq!(projected.columns, ["c", "a"]);
        assert_eq!(projected.rows, [vec!["6", "4"]]);
    }

    #[test]
    fn column_window_bounds_rendering_for_wide_datasets() {
        let columns = (0..10_000).collect::<Vec<_>>();
        let widths = vec![100.0; columns.len()];
        let window = visible_column_window(&columns, &widths, 5_000.0, 500.0);
        assert!(window.start > 0);
        assert!(window.end < columns.len());
        assert!(window.end - window.start <= 8);
        let rendered = (window.end - window.start) as f32 * 108.0;
        assert!((window.leading + rendered + window.trailing - 1_080_000.0).abs() < 0.1);
    }

    #[test]
    fn column_window_handles_empty_and_small_tables() {
        assert_eq!(
            visible_column_window(&[], &[], 0.0, 500.0),
            ColumnWindow {
                start: 0,
                end: 0,
                leading: 0.0,
                trailing: 0.0,
            }
        );
        let window = visible_column_window(&[0, 1], &[80.0, 80.0], 0.0, 500.0);
        assert_eq!(window.start, 0);
        assert_eq!(window.end, 2);
        assert_eq!(window.trailing, 0.0);
    }

    #[test]
    fn row_index_filters_and_sorts_numeric_and_text_values() {
        let table = TableData {
            columns: vec!["name".into(), "score".into()],
            rows: vec![
                vec!["beta".into(), "10".into()],
                vec!["alpha".into(), "2".into()],
                vec!["alphabet".into(), "30".into()],
            ],
        };
        assert_eq!(
            build_row_index(&table, &"alpha".into(), None, false),
            [1, 2]
        );
        assert_eq!(
            build_row_index(&table, &RowFilter::default(), Some(1), false),
            [1, 0, 2]
        );
        assert_eq!(
            build_row_index(&table, &RowFilter::default(), Some(0), true),
            [0, 2, 1]
        );
        // The `#` sentinel sorts by original position, independent of any column.
        assert_eq!(
            build_row_index(
                &table,
                &RowFilter::default(),
                Some(INDEX_SORT_COLUMN),
                false
            ),
            [0, 1, 2]
        );
        assert_eq!(
            build_row_index(&table, &RowFilter::default(), Some(INDEX_SORT_COLUMN), true),
            [2, 1, 0]
        );
        // Column-scoped and comparison filters.
        let greater = RowFilter {
            text: "5".into(),
            column: Some(1),
            mode: FilterMode::GreaterThan,
        };
        assert_eq!(build_row_index(&table, &greater, Some(1), false), [0, 2]);
        let exact = RowFilter {
            text: "alpha".into(),
            column: Some(0),
            mode: FilterMode::Equals,
        };
        assert_eq!(build_row_index(&table, &exact, None, false), [1]);
    }

    #[test]
    fn row_index_worker_returns_revision_keyed_results() {
        let worker = RowIndexWorker::default();
        worker
            .submit(
                RowIndexKey {
                    revision: 7,
                    filter: "alpha".into(),
                    sort_column: Some(1),
                    sort_descending: true,
                },
                Arc::new(TableData {
                    columns: vec!["name".into(), "score".into()],
                    rows: vec![
                        vec!["alpha".into(), "2".into()],
                        vec!["alphabet".into(), "30".into()],
                        vec!["beta".into(), "10".into()],
                    ],
                }),
            )
            .unwrap();
        let result = worker
            .receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_eq!(result.revision, 7);
        assert_eq!(result.filter.text, "alpha");
        assert_eq!(result.rows, [1, 0]);
    }

    #[test]
    fn row_index_scan_can_cancel_and_mixed_columns_sort_as_text() {
        let large = TableData {
            columns: vec!["value".into()],
            rows: (0..2_048).map(|value| vec![value.to_string()]).collect(),
        };
        let probes = std::cell::Cell::new(0);
        assert!(
            build_row_index_cancellable(&large, &RowFilter::default(), None, false, || {
                probes.set(probes.get() + 1);
                probes.get() >= 2
            })
            .is_none()
        );

        let mixed = TableData {
            columns: vec!["value".into()],
            rows: vec![vec!["x".into()], vec!["2".into()], vec!["10".into()]],
        };
        assert_eq!(
            build_row_index(&mixed, &RowFilter::default(), Some(0), false),
            [2, 1, 0]
        );
    }

    #[test]
    fn run_state_console_sentinel_does_not_overflow() {
        // Regression: RunState::Running(CONSOLE_CELL_ID) formatted `cell + 1`,
        // which panicked in debug (usize::MAX + 1).
        let (_, text, _) = run_state_chip(RunState::Running(CONSOLE_CELL_ID));
        assert_eq!(text, "Running console");
    }

    #[test]
    fn run_state_real_cells_are_one_indexed() {
        let (_, text, _) = run_state_chip(RunState::Running(0));
        assert_eq!(text, "Running cell 1");
        let (_, text, _) = run_state_chip(RunState::Running(41));
        assert_eq!(text, "Running cell 42");
    }
}
