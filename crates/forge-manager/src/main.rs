//! Forge Manager — a standalone, Navigator-style GUI over a Forge ML project's
//! environment. It shells out to `forge_ide` (the same environment CLI the
//! `forge` command uses) and renders the JSON status it emits, with one-click
//! actions for the operations that already exist (`native provide`, …). It never
//! links the environment internals — the JSON is the contract — so it stays a
//! thin, independent front-end, exactly as Anaconda Navigator sits over conda.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use eframe::egui;
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;

// ── The JSON contract (mirror of forge_ide's environment::status::EnvStatus) ──

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

// ── Running forge_ide ─────────────────────────────────────────────────────────

/// Locate the `forge_ide` binary: next to this executable (installed layout),
/// else on `PATH` (dev: cargo puts both in target/<profile>/).
fn forge_ide_path() -> PathBuf {
    let exe_name = if cfg!(windows) {
        "forge_ide.exe"
    } else {
        "forge_ide"
    };
    if let Ok(here) = std::env::current_exe() {
        if let Some(sibling) = here.parent().map(|dir| dir.join(exe_name)) {
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from(exe_name)
}

fn run_ide(args: &[&str]) -> Result<String, String> {
    let output = Command::new(forge_ide_path())
        .args(args)
        .output()
        .map_err(|error| format!("launching forge_ide: {error}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr);
    if !err.trim().is_empty() {
        text.push_str(&err);
    }
    Ok(text)
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

// ── The app ───────────────────────────────────────────────────────────────────

struct ManagerApp {
    project: String,
    status: Result<EnvStatus, String>,
    log: String,
}

impl ManagerApp {
    fn new() -> Self {
        let project = std::env::args()
            .nth(1)
            .filter(|arg| !arg.starts_with('-'))
            .or_else(|| std::env::current_dir().ok().map(|p| p.display().to_string()))
            .unwrap_or_else(|| ".".to_owned());
        let status = fetch_status(&project);
        ManagerApp {
            project,
            status,
            log: String::new(),
        }
    }

    fn refresh(&mut self) {
        self.status = fetch_status(&self.project);
    }

    fn provide_native(&mut self) {
        self.log = run_ide(&["--native-provide", &self.project]).unwrap_or_else(|error| error);
        self.refresh();
    }
}

const OK: egui::Color32 = egui::Color32::from_rgb(90, 180, 110);
const WARN: egui::Color32 = egui::Color32::from_rgb(210, 160, 70);
const BAD: egui::Color32 = egui::Color32::from_rgb(210, 100, 100);

fn status_color(status: &str) -> egui::Color32 {
    match status {
        "ok" | "available" => OK,
        "note" => WARN,
        s if s.starts_with("missing") => BAD,
        s if s.starts_with("incompatible") => BAD,
        _ => WARN,
    }
}

/// A deferred action requested from the (immutably-borrowing) render pass.
enum Action {
    None,
    Refresh,
    ProvideNative,
}

impl eframe::App for ManagerApp {
    // This eframe hands the app a `Ui` directly (no manual panels).
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut action = Action::None;

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading("Forge Manager");
            if let Ok(status) = &self.status {
                ui.label(
                    egui::RichText::new(format!("v{} · {}", status.forge_version, status.target))
                        .weak(),
                );
            }
        });
        ui.horizontal(|ui| {
            ui.label("Project:");
            ui.add(egui::TextEdit::singleline(&mut self.project).desired_width(420.0));
            if ui.button("Refresh").clicked() {
                action = Action::Refresh;
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| match &self.status {
            Err(error) => {
                ui.colored_label(BAD, error);
            }
            Ok(status) => {
                let requested = render(ui, status, &self.log);
                if let Action::None = action {
                    action = requested;
                }
            }
        });

        match action {
            Action::Refresh => self.refresh(),
            Action::ProvideNative => self.provide_native(),
            Action::None => {}
        }
    }
}

fn render(ui: &mut egui::Ui, status: &EnvStatus, log: &str) -> Action {
    let mut action = Action::None;

    section(ui, "Environment", |ui| {
        row(ui, "manifest", if status.manifest_present { "forge.toml" } else { "none (defaults)" }, OK);
        if let Some(profile) = &status.profile {
            row(ui, "profile", profile, OK);
        }
    });

    section(ui, "Providers", |ui| {
        for provider in &status.providers {
            row(ui, &provider.id, &provider.status, status_color(&provider.status));
        }
    });

    section(ui, "Diagnostics", |ui| {
        for check in &status.diagnostics {
            row(ui, &check.name, &check.detail, status_color(&check.status));
        }
    });

    section(ui, "GPU backends", |ui| {
        if status.gpu.is_empty() {
            ui.label("none detected — training/inference use the CPU");
        }
        for backend in &status.gpu {
            row(ui, &backend.name, &backend.detail, OK);
        }
    });

    section(ui, "Native prerequisites", |ui| {
        if status.native.prereqs.is_empty() {
            ui.label("no [native] section — nothing required");
        }
        for prereq in &status.native.prereqs {
            ui.horizontal(|ui| {
                let color = if prereq.satisfied { OK } else if prereq.providable { WARN } else { BAD };
                ui.colored_label(color, if prereq.satisfied { "ok" } else { "missing" });
                ui.strong(&prereq.name);
                ui.label(egui::RichText::new(&prereq.detail).weak());
                if prereq.providable && ui.button("Install").clicked() {
                    action = Action::ProvideNative;
                }
            });
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Catalog (installable prebuilts):").weak());
        ui.horizontal_wrapped(|ui| {
            if status.native.catalog.is_empty() {
                ui.label("— (catalog empty)");
            }
            for tool in &status.native.catalog {
                ui.label(format!("{} {}", tool.name, tool.detail));
            }
        });
        if !status.native.catalog.is_empty() && ui.button("Provide all needed").clicked() {
            action = Action::ProvideNative;
        }
    });

    section(ui, "Python bridge", |ui| {
        if !status.python.present {
            ui.label("no [python] section — Forge's core is Rust");
        } else {
            match &status.python.interpreter {
                Some(version) => row(
                    ui,
                    "interpreter",
                    &format!("{version} ({})", status.python.source.as_deref().unwrap_or("?")),
                    OK,
                ),
                None => row(ui, "interpreter", "none found", BAD),
            }
            if let Some(want) = &status.python.version_requested {
                row(ui, "version", &format!("requested {want}"), WARN);
            }
            if let Some(manager) = &status.python.manager {
                row(ui, "manager", manager, OK);
            }
            if !status.python.bridge.is_empty() {
                row(ui, "bridge", &status.python.bridge.join(", "), WARN);
            }
        }
    });

    if !status.gaps.is_empty() {
        section(ui, "Gaps", |ui| {
            for gap in &status.gaps {
                ui.colored_label(BAD, gap);
            }
        });
    }

    if !log.is_empty() {
        section(ui, "Last action", |ui| {
            ui.label(egui::RichText::new(log).monospace().size(11.0));
        });
    }

    action
}

fn section(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(8.0);
    ui.heading(title);
    ui.separator();
    contents(ui);
}

fn row(ui: &mut egui::Ui, label: &str, detail: &str, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.colored_label(color, "●");
        ui.add(egui::Label::new(egui::RichText::new(label).strong()).truncate());
        ui.label(egui::RichText::new(detail).weak());
    });
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Forge Manager")
            .with_inner_size([760.0, 720.0])
            .with_min_inner_size([520.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Forge Manager",
        options,
        Box::new(|_cc| Ok(Box::new(ManagerApp::new()))),
    )
}
