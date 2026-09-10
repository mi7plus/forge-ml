//! Forge Manager — a standalone, Navigator-style desktop app to inspect and
//! manage a Forge ML project's environment.
//!
//! It is a thin front-end: `forge_ide --env-status-json` emits a snapshot, the
//! Manager renders it, and the buttons run the same commands the CLI exposes
//! (`--native-provide`, launching the IDE, opening the releases page). The JSON
//! is the whole contract, so the Manager never links the environment internals —
//! exactly how Anaconda Navigator sits over conda.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use eframe::egui;
use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;

const RELEASES_URL: &str = "https://github.com/mi7plus/forge-ml/releases/latest";

// ── Palette (dark, high contrast) ─────────────────────────────────────────────
const BG: Color32 = Color32::from_rgb(0x15, 0x17, 0x1C); // window
const HEADER: Color32 = Color32::from_rgb(0x1B, 0x1E, 0x25);
const CARD: Color32 = Color32::from_rgb(0x21, 0x25, 0x2E);
const CARD_HI: Color32 = Color32::from_rgb(0x2A, 0x2F, 0x3A);
const BORDER: Color32 = Color32::from_rgb(0x33, 0x38, 0x44);
const TEXT: Color32 = Color32::from_rgb(0xE7, 0xEA, 0xEF);
const MUTED: Color32 = Color32::from_rgb(0x98, 0xA0, 0xAD);
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
    let exe = if cfg!(windows) { "forge_ide.exe" } else { "forge_ide" };
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
            .or_else(|| std::env::current_dir().ok().map(|p| p.display().to_string()))
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
    v.faint_bg_color = HEADER;
    v.extreme_bg_color = Color32::from_rgb(0x0F, 0x11, 0x15);
    v.hyperlink_color = ACCENT;
    v.widgets.noninteractive.bg_fill = BG;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.weak_bg_fill = CARD_HI;
    v.widgets.inactive.bg_fill = CARD_HI;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.weak_bg_fill = BORDER;
    v.widgets.hovered.bg_fill = BORDER;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.active.weak_bg_fill = ACCENT;
    v.widgets.active.bg_fill = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(0x2A, 0x4A, 0x70);
    v
}

impl eframe::App for ManagerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.painter()
            .rect_filled(ui.max_rect(), CornerRadius::same(0), BG);

        let mut action = Action::None;

        // Header.
        Frame::NONE
            .fill(HEADER)
            .inner_margin(Margin::symmetric(16, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Forge Manager").size(19.0).strong().color(TEXT));
                    if let Ok(status) = &self.status {
                        ui.label(
                            RichText::new(format!(
                                "Forge ML {} · {}",
                                status.forge_version, status.target
                            ))
                            .color(MUTED),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Refresh").clicked() {
                            action = Action::Refresh;
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut self.project)
                                .desired_width(360.0)
                                .hint_text("project folder"),
                        );
                        ui.label(RichText::new("Project").color(MUTED));
                    });
                });
            });

        // Tabs.
        Frame::NONE
            .fill(BG)
            .inner_margin(Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (page, label) in [
                        (Page::Home, "Home"),
                        (Page::Environment, "Environment"),
                        (Page::Packages, "Packages"),
                        (Page::Diagnostics, "Diagnostics"),
                    ] {
                        let selected = self.page == page;
                        let text = RichText::new(label)
                            .size(14.0)
                            .color(if selected { ACCENT } else { MUTED });
                        if ui.selectable_label(selected, text).clicked() {
                            self.page = page;
                        }
                    }
                });
            });
        ui.separator();

        // Content.
        Frame::NONE
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let requested = match &self.status {
                            Err(error) => {
                                card(ui, |ui| {
                                    ui.colored_label(BAD, "Could not read the environment.");
                                    ui.label(RichText::new(error).color(MUTED).monospace().size(11.0));
                                    ui.label(
                                        RichText::new(
                                            "Is forge_ide next to this app, or on PATH?",
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
                        if let Action::None = action {
                            action = requested;
                        }
                        if !self.log.is_empty() {
                            card(ui, |ui| {
                                ui.label(RichText::new("Last action").strong().color(TEXT));
                                ui.add_space(4.0);
                                ui.label(RichText::new(&self.log).monospace().size(11.0).color(MUTED));
                            });
                        }
                    });
            });

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

    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Forge ML").size(18.0).strong().color(TEXT));
            ui.label(RichText::new(format!("v{}", status.forge_version)).color(MUTED));
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new(
                "The batteries-included Rust ML studio. Open a project in the IDE, or get \
                 the latest signed installer.",
            )
            .color(MUTED),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if primary_button(ui, "Launch IDE").clicked() {
                action = Action::LaunchIde;
            }
            if ui.button("Get the latest release  ↗").clicked() {
                action = Action::OpenReleases;
            }
        });
    });

    card(ui, |ui| {
        ui.label(RichText::new("This project").strong().color(TEXT));
        ui.add_space(4.0);
        kv(ui, "Folder", if project.is_empty() { "." } else { project });
        kv(
            ui,
            "Manifest",
            if status.manifest_present { "forge.toml" } else { "none (defaults)" },
        );
        if let Some(profile) = &status.profile {
            kv(ui, "Profile", profile);
        }
        if !status.gaps.is_empty() {
            ui.add_space(6.0);
            for gap in &status.gaps {
                ui.colored_label(BAD, format!("• {gap}"));
            }
        }
    });

    action
}

fn environment(ui: &mut egui::Ui, status: &EnvStatus) -> Action {
    card(ui, |ui| {
        ui.label(RichText::new("Providers").strong().color(TEXT));
        ui.add_space(6.0);
        for provider in &status.providers {
            status_row(ui, color_of(&provider.status), &provider.id, &provider.status);
        }
    });

    card(ui, |ui| {
        ui.label(RichText::new("GPU").strong().color(TEXT));
        ui.add_space(6.0);
        if status.gpu.is_empty() {
            ui.colored_label(MUTED, "No GPU backend detected — training/inference use the CPU.");
        }
        for backend in &status.gpu {
            status_row(ui, OK, &backend.name, &backend.detail);
        }
    });

    card(ui, |ui| {
        ui.label(RichText::new("Python bridge").strong().color(TEXT));
        ui.add_space(6.0);
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
            ui.label(RichText::new("Native tools").strong().color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if busy {
                    ui.label(RichText::new("installing…").color(WARN));
                }
            });
        });
        ui.label(
            RichText::new(
                "Pinned, SHA-256-verified prebuilts. Install one and it's put on PATH for \
                 forge run / build / test.",
            )
            .color(MUTED)
            .size(12.0),
        );
        ui.add_space(8.0);

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
                .inner_margin(Margin::symmetric(12, 8))
                .corner_radius(CornerRadius::same(6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&tool.name).strong().color(TEXT));
                        ui.label(RichText::new(format!("v{}", tool.detail)).color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if installed {
                                ui.colored_label(OK, "on system ✓");
                            } else if ui
                                .add_enabled(!busy, primary_widget("Install"))
                                .clicked()
                            {
                                action = Action::Install(tool.name.clone());
                            }
                        });
                    });
                });
            ui.add_space(6.0);
        }
    });

    if !status.native.prereqs.is_empty() {
        card(ui, |ui| {
            ui.label(RichText::new("Declared prerequisites ([native])").strong().color(TEXT));
            ui.add_space(6.0);
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
            ui.add_space(6.0);
            if ui.add_enabled(!busy, primary_widget("Provide all needed")).clicked() {
                action = Action::InstallAll;
            }
        });
    }

    action
}

fn diagnostics(ui: &mut egui::Ui, status: &EnvStatus) -> Action {
    card(ui, |ui| {
        ui.label(RichText::new("Host tooling").strong().color(TEXT));
        ui.add_space(6.0);
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
        .inner_margin(Margin::same(14))
        .corner_radius(CornerRadius::same(10))
        .stroke(Stroke::new(1.0, BORDER))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            contents(ui);
        });
    ui.add_space(12.0);
}

fn status_row(ui: &mut egui::Ui, color: Color32, label: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("●").color(color));
        ui.label(RichText::new(label).strong().color(TEXT));
        ui.label(RichText::new(detail).color(MUTED));
    });
}

fn kv(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{key}:")).color(MUTED));
        ui.label(RichText::new(value).color(TEXT));
    });
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

fn primary_button(ui: &mut egui::Ui, label: &'static str) -> egui::Response {
    ui.add(primary_widget(label))
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Forge Manager")
            .with_inner_size([880.0, 760.0])
            .with_min_inner_size([620.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Forge Manager",
        options,
        Box::new(|cc| Ok(Box::new(ManagerApp::new(&cc.egui_ctx)))),
    )
}
