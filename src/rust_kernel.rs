//! An independent Rust REPL kernel pane.
//!
//! Each [`RustKernel`] owns its own Evcxr session (a separate [`RuntimeHandle`]
//! process), so several scratch kernels can run side by side — each a dockable,
//! floatable pane — without sharing state with the notebook or one another.

use crate::runtime::{CellResult, RuntimeHandle};
use crate::{accent, EMBER, GREEN, RED, TEXT};
use eframe::egui;
use egui::RichText;

/// One live Rust REPL backed by its own Evcxr process.
pub struct RustKernel {
    runtime: RuntimeHandle,
    output: String,
    input: String,
    history: Vec<String>,
    ready: bool,
    failed: bool,
    exec_count: usize,
    /// True while a submitted expression is still running.
    pending: bool,
}

impl RustKernel {
    pub fn spawn() -> Self {
        Self {
            runtime: RuntimeHandle::spawn(),
            output: "Booting isolated Rust kernel...\n".to_owned(),
            input: String::new(),
            history: Vec::new(),
            ready: false,
            failed: false,
            exec_count: 0,
            pending: true,
        }
    }

    /// Drain runtime results into the output buffer. Returns true if anything changed.
    fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Some(result) = self.runtime.try_recv() {
            changed = true;
            self.pending = false;
            match result {
                CellResult::Ready => {
                    self.ready = true;
                    self.output.push_str("Kernel ready.\n");
                }
                CellResult::Success { output, .. } => {
                    if !output.trim().is_empty() {
                        self.output.push_str(&output);
                        self.output.push('\n');
                    }
                }
                CellResult::Error { message, .. } => {
                    self.output.push_str(&message);
                    self.output.push('\n');
                }
                CellResult::Reset => {
                    self.ready = true;
                    self.output.push_str("Kernel reset.\n");
                }
                CellResult::RuntimeError(message) => {
                    self.failed = true;
                    self.output.push_str(&message);
                    self.output.push('\n');
                }
            }
        }
        changed
    }

    fn run_input(&mut self) {
        let code = self.input.trim().to_owned();
        if code.is_empty() {
            return;
        }
        self.exec_count += 1;
        self.output
            .push_str(&format!("In [{}]: {}\n", self.exec_count, code));
        self.history.push(code.clone());
        let _ = self.runtime.execute(self.exec_count, code);
        self.input.clear();
        self.pending = true;
    }

    /// Render the kernel pane and drive it. Returns true if a repaint is needed.
    pub fn ui(&mut self, id: u32, ui: &mut egui::Ui) -> bool {
        let mut changed = self.poll();
        ui.push_id(("rust_kernel", id), |ui| {
            use egui_phosphor_icons::icons;
            ui.horizontal(|ui| {
                let (glyph, label, color) = if self.failed {
                    (icons::X_CIRCLE, "Failed", RED)
                } else if self.pending {
                    (icons::CIRCLE_NOTCH, "Running", accent())
                } else if self.ready {
                    (icons::CHECK_CIRCLE, "Ready", GREEN)
                } else {
                    (icons::CIRCLE_DASHED, "Booting", EMBER)
                };
                ui.label(
                    RichText::new(format!("{}  {label}", glyph.as_str()))
                        .size(11.0)
                        .color(color),
                );
                if crate::compact_icon_button(ui, icons::BROOM, "Clear kernel output").clicked() {
                    self.output.clear();
                }
                if crate::compact_icon_button(
                    ui,
                    icons::ARROW_COUNTER_CLOCKWISE,
                    "Restart this kernel",
                )
                .clicked()
                {
                    let _ = self.runtime.reset();
                    self.ready = false;
                    self.failed = false;
                    self.pending = true;
                    self.output.push_str("Restarting kernel...\n");
                }
                if self.pending
                    && crate::compact_icon_button(ui, icons::STOP, "Stop execution").clicked()
                {
                    let _ = self.runtime.stop();
                    self.pending = false;
                }
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt(("rust_kernel_out", id))
                .max_height((ui.available_height() - 30.0).max(30.0))
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(&self.output)
                            .monospace()
                            .color(if self.failed { RED } else { TEXT }),
                    );
                });

            ui.horizontal(|ui| {
                ui.label(RichText::new("In [ ]:").monospace().strong().color(accent()));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.input)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .hint_text("Rust expression (own session)"),
                );
                let submit = (response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    || ui.button("Run").clicked();
                if submit {
                    self.run_input();
                    response.request_focus();
                    changed = true;
                }
            });
        });

        if self.pending {
            ui.ctx().request_repaint();
        }
        changed || self.pending
    }
}

impl Drop for RustKernel {
    fn drop(&mut self) {
        let _ = self.runtime.stop();
    }
}
