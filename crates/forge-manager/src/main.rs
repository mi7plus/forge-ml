//! Forge Manager — a standalone, Navigator-style desktop app to inspect and
//! manage a Forge ML project's environment: a left sidebar (Home / Environment /
//! Packages / Diagnostics), app tiles to launch or get Forge ML, and a verified
//! native-tool install list.
//!
//! It is a thin front-end: `forge_ide --env-status-json` emits a snapshot, the
//! Manager renders it, and the buttons run the same commands the CLI exposes
//! (`--native-provide`, launching the IDE, opening the releases page). The JSON
//! is the whole contract — the Manager never links the environment internals,
//! exactly how Anaconda Navigator sits over conda.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use eframe::egui;
use egui::{
    pos2, vec2, Align, Color32, CornerRadius, Frame, Layout, Margin, Rect, RichText, Stroke,
    UiBuilder,
};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;

const RELEASES_URL: &str = "https://github.com/mi7plus/forge-ml/releases/latest";

// ── Palette (dark, high contrast; neutrals biased toward the blue accent) ─────
const BG: Color32 = Color32::from_rgb(0x15, 0x17, 0x1C);
const SIDEBAR: Color32 = Color32::from_rgb(0x19, 0x1C, 0x22);
const CARD: Color32 = Color32::from_rgb(0x21, 0x25, 0x2E);
const CARD_HI: Color32 = Color32::from_rgb(0x2A, 0x2F, 0x3A);
const BORDER: Color32 = Color32::from_rgb(0x31, 0x37, 0x42);
const TEXT: Color32 = Color32::from_rgb(0xE7, 0xEA, 0xEF);
const MUTED: Color32 = Color32::from_rgb(0x98, 0xA0, 0xAD);
const FAINT: Color32 = Color32::from_rgb(0x6B, 0x72, 0x80);
const ACCENT: Color32 = Color32::from_rgb(0x4F, 0x9D, 0xF0);
const OK: Color32 = Color32::from_rgb(0x5F, 0xB3, 0x7A);
const WARN: Color32 = Color32::from_rgb(0xE0, 0xA4, 0x4B);
const BAD: Color32 = Color32::from_rgb(0xE0, 0x6C, 0x6C);

// ── The JSON contract (mirror of environment::status::EnvStatus) ──────────────

#[derive(Default, Deserialize)]
struct EnvStatus {
    #[serde(default)]
    forge_version: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    manifest_present: bool,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    providers: Vec<IdStatus>,
    #[serde(default)]
    diagnostics: Vec<Check>,
    #[serde(default)]
    gpu: Vec<Named>,
    #[serde(default)]
    native: NativeStatus,
    #[serde(default)]
    python: PythonStatus,
    #[serde(default)]
    gaps: Vec<String>,
}
#[derive(Deserialize)]
struct IdStatus {
    id: String,
    status: String,
}
#[derive(Deserialize)]
struct Check {
    name: String,
    status: String,
    detail: String,
}
#[derive(Deserialize, Clone)]
struct Named {
    name: String,
    detail: String,
}
#[derive(Default, Deserialize)]
struct NativeStatus {
    #[serde(default)]
    prereqs: Vec<Prereq>,
    #[serde(default)]
    catalog: Vec<Named>,
}
#[derive(Deserialize)]
struct Prereq {
    name: String,
    satisfied: bool,
    detail: String,
    providable: bool,
}
#[derive(Default, Deserialize)]
struct PythonStatus {
    present: bool,
    interpreter: Option<String>,
    source: Option<String>,
    version_requested: Option<String>,
    manager: Option<String>,
    #[serde(default)]
    bridge: Vec<String>,
}

// ── Running forge_ide / external commands ─────────────────────────────────────

fn forge_ide_path() -> PathBuf {
    let exe = if cfg!(windows) {
        "forge_ide.exe"
    } else {
        "forge_ide"
    };
    if let Ok(here) = std::env::current_exe() {
        if let Some(sibling) = here.parent().map(|dir| dir.join(exe)) {
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from(exe)
}

fn run_ide(args: &[&str]) -> String {
    match Command::new(forge_ide_path()).args(args).output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            text
        }
        Err(error) => format!("launching forge_ide: {error}"),
    }
}

fn fetch_status(project: &str) -> Result<EnvStatus, String> {
    let output = Command::new(forge_ide_path())
        .args(["--env-status-json", project])
        .output()
        .map_err(|error| format!("launching forge_ide: {error}"))?;
    serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "could not read status: {error}\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn launch_ide(project: &str) {
    let _ = Command::new(forge_ide_path()).arg(project).spawn();
}

fn open_url(url: &str) {
    #[cfg(windows)]
    let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open").arg(url).spawn();
}

// ── App ───────────────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Home,
    Environment,
    Packages,
    Diagnostics,
}
impl Page {
    fn label(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Environment => "Environment",
            Page::Packages => "Packages",
            Page::Diagnostics => "Diagnostics",
        }
    }
}

enum Action {
    None,
    Refresh,
    InstallAll,
    Install(String),
    LaunchIde,
    OpenReleases,
}

struct ManagerApp {
    project: String,
    status: Result<EnvStatus, String>,
    page: Page,
    log: String,
    busy: bool,
}

impl ManagerApp {
    fn new(ctx: &egui::Context) -> Self {
        ctx.set_visuals(theme());
        let project = std::env::args()
            .nth(1)
            .filter(|arg| !arg.starts_with('-'))
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|p| p.display().to_string())
            })
            .unwrap_or_else(|| ".".to_owned());
        let status = fetch_status(&project);
        ManagerApp {
            project,
            status,
            page: Page::Home,
            log: String::new(),
            busy: false,
        }
    }

    fn refresh(&mut self) {
        self.status = fetch_status(&self.project);
    }

    fn install(&mut self, tool: Option<&str>) {
        self.busy = true;
        let mut args = vec!["--native-provide", &self.project];
        if let Some(tool) = tool {
            args.push("--tools");
            args.push(tool);
        }
        self.log = run_ide(&args);
        self.busy = false;
        self.refresh();
    }
}

fn theme() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.faint_bg_color = SIDEBAR;
    v.extreme_bg_color = Color32::from_rgb(0x0F, 0x11, 0x15);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(0x27, 0x3B, 0x55);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.noninteractive,
    ] {
        w.fg_stroke = Stroke::new(1.0, TEXT);
        w.corner_radius = CornerRadius::same(7);
    }
    v.widgets.noninteractive.bg_fill = BG;
    v.widgets.inactive.weak_bg_fill = CARD_HI;
    v.widgets.inactive.bg_fill = CARD_HI;
    v.widgets.hovered.weak_bg_fill = BORDER;
    v.widgets.hovered.bg_fill = BORDER;
    v.widgets.active.weak_bg_fill = ACCENT;
    v.widgets.active.bg_fill = ACCENT;
    v
}

impl eframe::App for ManagerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let full = ui.max_rect();
        let sidebar_w = 206.0;
        let sidebar_rect = Rect::from_min_max(full.min, pos2(full.min.x + sidebar_w, full.max.y));
        let main_rect = Rect::from_min_max(pos2(full.min.x + sidebar_w, full.min.y), full.max);

        // Grounds + divider.
        ui.painter().rect_filled(full, CornerRadius::same(0), BG);
        ui.painter()
            .rect_filled(sidebar_rect, CornerRadius::same(0), SIDEBAR);
        ui.painter().vline(
            full.min.x + sidebar_w,
            full.y_range(),
            Stroke::new(1.0, BORDER),
        );

        let mut action = Action::None;

        // ── Sidebar ──────────────────────────────────────────────────────────
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(sidebar_rect.shrink2(vec2(12.0, 16.0)))
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("FORGE").size(15.0).strong().color(TEXT));
                    ui.label(RichText::new("ML").size(15.0).strong().color(ACCENT));
                });
                ui.add_space(14.0);
                for page in [
                    Page::Home,
                    Page::Environment,
                    Page::Packages,
                    Page::Diagnostics,
                ] {
                    let selected = self.page == page;
                    let text = RichText::new(page.label()).size(14.0).color(if selected {
                        TEXT
                    } else {
                        MUTED
                    });
                    let fill = if selected {
                        Color32::from_rgb(0x22, 0x2E, 0x40)
                    } else {
                        SIDEBAR
                    };
                    let button = egui::Button::new(text)
                        .fill(fill)
                        .stroke(Stroke::new(0.0, SIDEBAR))
                        .min_size(vec2(ui.available_width(), 34.0));
                    if ui.add(button).clicked() {
                        self.page = page;
                    }
                    ui.add_space(2.0);
                }
                // Footer pinned to the bottom.
                let foot = if let Ok(s) = &self.status {
                    format!("Forge ML {} · {}", s.forge_version, s.target)
                } else {
                    "Forge ML".to_owned()
                };
                ui.add_space((ui.available_height() - 20.0).max(8.0));
                ui.label(RichText::new(foot).size(11.0).monospace().color(FAINT));
            },
        );

        // ── Main: topbar + content ───────────────────────────────────────────
        let topbar_h = 54.0;
        let topbar_rect = Rect::from_min_max(
            main_rect.min,
            pos2(main_rect.max.x, main_rect.min.y + topbar_h),
        );
        let content_rect = Rect::from_min_max(
            pos2(main_rect.min.x, main_rect.min.y + topbar_h),
            main_rect.max,
        );
        ui.painter().hline(
            main_rect.x_range(),
            main_rect.min.y + topbar_h,
            Stroke::new(1.0, BORDER),
        );

        // Topbar: project field + Refresh (owns the &mut project borrow).
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(topbar_rect.shrink2(vec2(16.0, 11.0)))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Refresh").clicked() {
                        action = Action::Refresh;
                    }
                    ui.add_space(8.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.project)
                            .desired_width(ui.available_width())
                            .hint_text("project folder"),
                    );
                });
            },
        );

        // Content (borrows status/log/project as shared; topbar borrow has ended).
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(content_rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                Frame::NONE.inner_margin(Margin::same(16)).show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let requested = match &self.status {
                                Err(error) => {
                                    card(ui, |ui| {
                                        ui.colored_label(BAD, "Could not read the environment.");
                                        ui.label(
                                            RichText::new(error)
                                                .color(MUTED)
                                                .monospace()
                                                .size(11.0),
                                        );
                                        ui.label(
                                            RichText::new(
                                                "Is forge_ide beside this app, or on PATH?",
                                            )
                                            .color(MUTED),
                                        );
                                    });
                                    Action::None
                                }
                                Ok(status) => match self.page {
                                    Page::Home => home(ui, status, &self.project),
                                    Page::Environment => environment(ui, status),
                                    Page::Packages => packages(ui, status, self.busy),
                                    Page::Diagnostics => diagnostics(ui, status),
                                },
                            };
                            if matches!(action, Action::None) {
                                action = requested;
                            }
                            if !self.log.is_empty() {
                                card(ui, |ui| {
                                    ui.label(RichText::new("Last action").strong().color(TEXT));
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(&self.log)
                                            .monospace()
                                            .size(11.0)
                                            .color(MUTED),
                                    );
                                });
                            }
                        });
                });
            },
        );

        match action {
            Action::Refresh => self.refresh(),
            Action::InstallAll => self.install(None),
            Action::Install(tool) => self.install(Some(&tool)),
            Action::LaunchIde => launch_ide(&self.project),
            Action::OpenReleases => open_url(RELEASES_URL),
            Action::None => {}
        }
    }
}

// ── Pages ─────────────────────────────────────────────────────────────────────

fn home(ui: &mut egui::Ui, status: &EnvStatus, project: &str) -> Action {
    let mut action = Action::None;

    section_title(ui, "Applications");
    tile(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Forge ML").size(18.0).strong().color(TEXT));
            ui.label(
                RichText::new(format!("v{} · installed", status.forge_version))
                    .monospace()
                    .size(12.0)
                    .color(MUTED),
            );
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "The batteries-included Rust ML studio — notebooks, in-process training, ONNX, \
                 and GPU acceleration by default.",
            )
            .color(MUTED),
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if primary(ui, "Launch IDE").clicked() {
                action = Action::LaunchIde;
            }
            if ui.button("Get the latest release  ↗").clicked() {
                action = Action::OpenReleases;
            }
        });
    });

    card(ui, |ui| {
        ui.label(RichText::new("This project").strong().color(TEXT));
        ui.add_space(8.0);
        kv(ui, "Folder", if project.is_empty() { "." } else { project });
        kv(
            ui,
            "Manifest",
            if status.manifest_present {
                "forge.toml"
            } else {
                "none (defaults)"
            },
        );
        if let Some(profile) = &status.profile {
            kv(ui, "Profile", profile);
        }
        for gap in &status.gaps {
            ui.colored_label(BAD, format!("• {gap}"));
        }
    });

    action
}

fn environment(ui: &mut egui::Ui, status: &EnvStatus) -> Action {
    card(ui, |ui| {
        card_title(ui, "Providers");
        for provider in &status.providers {
            status_row(
                ui,
                color_of(&provider.status),
                &provider.id,
                &provider.status,
            );
        }
    });
    card(ui, |ui| {
        card_title(ui, "GPU");
        if status.gpu.is_empty() {
            ui.colored_label(
                MUTED,
                "No GPU backend detected — training/inference use the CPU.",
            );
        }
        for backend in &status.gpu {
            status_row(ui, OK, &backend.name, &backend.detail);
        }
    });
    card(ui, |ui| {
        card_title(ui, "Python bridge");
        let py = &status.python;
        if !py.present {
            ui.colored_label(MUTED, "No [python] section — Forge's core is Rust.");
        } else {
            match &py.interpreter {
                Some(v) => status_row(
                    ui,
                    OK,
                    "interpreter",
                    &format!("{v} ({})", py.source.as_deref().unwrap_or("?")),
                ),
                None => status_row(ui, BAD, "interpreter", "none found"),
            }
            if let Some(want) = &py.version_requested {
                status_row(ui, WARN, "version", &format!("requested {want}"));
            }
            if let Some(manager) = &py.manager {
                status_row(ui, OK, "manager", manager);
            }
            if !py.bridge.is_empty() {
                status_row(ui, MUTED, "bridge", &py.bridge.join(", "));
            }
        }
    });
    Action::None
}

fn packages(ui: &mut egui::Ui, status: &EnvStatus, busy: bool) -> Action {
    let mut action = Action::None;

    card(ui, |ui| {
        ui.horizontal(|ui| {
            card_title(ui, "Native tools — pinned & SHA-256 verified");
            if busy {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("installing…").color(WARN));
                });
            }
        });
        ui.label(
            RichText::new(
                "Install one and it's placed on PATH for forge run / build / test. Nothing is \
                 trusted unverified.",
            )
            .color(MUTED)
            .size(12.0),
        );
        ui.add_space(10.0);

        if status.native.catalog.is_empty() {
            ui.colored_label(MUTED, "The catalog is empty.");
        }
        for tool in &status.native.catalog {
            let installed = status
                .native
                .prereqs
                .iter()
                .any(|p| p.satisfied && p.name.starts_with(&tool.name));
            Frame::NONE
                .fill(CARD_HI)
                .inner_margin(Margin::symmetric(12, 9))
                .corner_radius(CornerRadius::same(7))
                .stroke(Stroke::new(1.0, BORDER))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&tool.name).strong().size(14.0).color(TEXT));
                        ui.label(
                            RichText::new(format!("v{}", tool.detail))
                                .monospace()
                                .size(11.5)
                                .color(MUTED),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if installed {
                                ui.colored_label(OK, "on system ✓");
                            } else if ui.add_enabled(!busy, primary_widget("Install")).clicked() {
                                action = Action::Install(tool.name.clone());
                            }
                        });
                    });
                });
            ui.add_space(8.0);
        }
    });

    if !status.native.prereqs.is_empty() {
        card(ui, |ui| {
            card_title(ui, "Declared prerequisites — [native]");
            for prereq in &status.native.prereqs {
                let color = if prereq.satisfied {
                    OK
                } else if prereq.providable {
                    WARN
                } else {
                    BAD
                };
                status_row(ui, color, &prereq.name, &prereq.detail);
            }
            ui.add_space(10.0);
            if ui
                .add_enabled(!busy, primary_widget("Provide all needed"))
                .clicked()
            {
                action = Action::InstallAll;
            }
        });
    }

    action
}

fn diagnostics(ui: &mut egui::Ui, status: &EnvStatus) -> Action {
    card(ui, |ui| {
        card_title(ui, "Host tooling");
        for check in &status.diagnostics {
            status_row(ui, color_of(&check.status), &check.name, &check.detail);
        }
    });
    Action::None
}

// ── Widgets ───────────────────────────────────────────────────────────────────

fn card(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    Frame::NONE
        .fill(CARD)
        .inner_margin(Margin::same(15))
        .corner_radius(CornerRadius::same(10))
        .stroke(Stroke::new(1.0, BORDER))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            contents(ui);
        });
    ui.add_space(14.0);
}

fn tile(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    card(ui, contents);
}

fn card_title(ui: &mut egui::Ui, title: &str) {
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.5)
            .strong()
            .color(MUTED),
    );
    ui.add_space(8.0);
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.5)
            .strong()
            .color(MUTED),
    );
    ui.add_space(8.0);
}

fn status_row(ui: &mut egui::Ui, color: Color32, label: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("●").size(11.0).color(color));
        ui.label(RichText::new(label).strong().color(TEXT));
        ui.label(RichText::new(detail).color(MUTED));
    });
    ui.add_space(2.0);
}

fn kv(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).color(MUTED));
        ui.label(RichText::new(value).monospace().size(12.5).color(TEXT));
    });
    ui.add_space(2.0);
}

fn color_of(status: &str) -> Color32 {
    match status {
        "ok" | "available" => OK,
        "note" => WARN,
        s if s.starts_with("missing") || s.starts_with("incompatible") => BAD,
        _ => WARN,
    }
}

fn primary_widget(label: &'static str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label).color(Color32::WHITE).strong()).fill(ACCENT)
}

fn primary(ui: &mut egui::Ui, label: &'static str) -> egui::Response {
    ui.add(primary_widget(label))
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Forge Manager")
            .with_inner_size([900.0, 760.0])
            .with_min_inner_size([680.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Forge Manager",
        options,
        Box::new(|cc| Ok(Box::new(ManagerApp::new(&cc.egui_ctx)))),
    )
}
