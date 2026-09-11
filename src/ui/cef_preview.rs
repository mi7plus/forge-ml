//! In-IDE dockable web preview backed by the out-of-process `forge_cef` helper.
//!
//! The helper renders a page with Chromium **offscreen** and streams BGRA
//! frames into a memory-mapped file; this widget maps that file, uploads each
//! new frame to an egui texture, and paints it in a dockable tile. Pointer and
//! scroll input over the tile are forwarded to the helper over its stdin as
//! [`forge_cef_ipc::Command`]s. Because the helper is a separate process, a
//! browser crash cannot take the IDE down, and the main binary never links CEF
//! or ships its ~150MB runtime — it depends only on the tiny `forge-cef-ipc`
//! crate for the shared layout and command protocol.
//!
//! The helper binary currently exists only for Windows; on other platforms
//! [`CefPreview::spawn`] fails and the pane shows an explanatory message.
//!
//! Coordinates are handled in egui points at device scale 1.0 — the offscreen
//! surface is sized to the tile's point size, so pointer positions map to
//! surface pixels one-to-one. (A hi-DPI-crisp path would size the surface in
//! physical pixels and scale input by pixels-per-point; deferred.)

use eframe::egui;
use forge_cef_ipc::{self as ipc, Command, MouseButton};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Proc, Stdio};

/// A live web-preview session: the helper child, the shared frame buffer, and
/// the texture it is uploaded into.
pub struct CefPreview {
    child: Child,
    stdin: Option<ChildStdin>,
    shm_path: PathBuf,
    map: memmap2::MmapMut,
    texture: Option<egui::TextureHandle>,
    last_seq: u32,
    /// Current offscreen surface size in points (== pixels at scale 1.0).
    size: (u32, u32),
    /// Last error, if the session has failed; shown in place of the frame.
    error: Option<String>,
}

impl CefPreview {
    /// Spawn a helper rendering `url` (an `http(s)://`, `file://`, or `data:`
    /// URL) at an initial `size` in points. Creates and initializes the shared
    /// frame-buffer file, then launches `forge_cef` beside the running IDE exe.
    pub fn spawn(url: &str, size: (u32, u32)) -> Result<Self, String> {
        let (w, h) = clamp_size(size);

        // Create + size + header-initialize the shared frame-buffer file.
        let shm_path = unique_shm_path();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&shm_path)
            .map_err(|e| format!("create frame buffer {}: {e}", shm_path.display()))?;
        file.set_len(ipc::TOTAL_BYTES as u64)
            .map_err(|e| format!("size frame buffer: {e}"))?;
        // SAFETY: private temp file we just created and own for this session.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file) }
            .map_err(|e| format!("map frame buffer: {e}"))?;
        ipc::init_header(&mut map);

        let binary = helper_binary();
        let mut cmd = Proc::new(&binary);
        cmd.arg("--shm")
            .arg(&shm_path)
            .arg("--url")
            .arg(url)
            .arg("--width")
            .arg(w.to_string())
            .arg("--height")
            .arg(h.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        no_console_window(&mut cmd);

        let mut child = cmd.spawn().map_err(|e| {
            let _ = std::fs::remove_file(&shm_path);
            format!("launch {}: {e}", binary.display())
        })?;
        let stdin = child.stdin.take();

        Ok(Self {
            child,
            stdin,
            shm_path,
            map,
            texture: None,
            last_seq: 0,
            size: (w, h),
            error: None,
        })
    }

    /// Point the preview at a different URL (reuses the running helper).
    pub fn navigate(&mut self, url: &str) {
        self.send(Command::Navigate(url.to_string()));
    }

    /// Render the preview into `ui`, pulling the latest frame, resizing the
    /// surface to the available area, and forwarding pointer/scroll input.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(err) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, err);
            return;
        }
        // If the helper has exited, surface it and stop.
        if let Ok(Some(status)) = self.child.try_wait() {
            self.error = Some(format!("Web preview helper exited ({status})."));
            return;
        }

        // Resize the surface to the tile (integer points), if it changed.
        let avail = ui.available_size();
        let want = clamp_size((avail.x.max(1.0) as u32, avail.y.max(1.0) as u32));
        if want != self.size {
            self.size = want;
            self.send(Command::Resize { width: want.0, height: want.1 });
        }

        // Upload the newest frame, if any.
        if let Some(frame) = ipc::latest_frame(&self.map, self.last_seq) {
            self.last_seq = frame.seq;
            let image = bgra_to_color_image(frame.width, frame.height, frame.bgra);
            match &mut self.texture {
                Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.texture =
                        Some(ui.ctx().load_texture("forge_cef_frame", image, egui::TextureOptions::LINEAR));
                }
            }
        }

        // Paint the current texture and capture input over it. Take the id
        // (Copy) so no borrow of `self.texture` is held across `forward_input`.
        let tex_id = self.texture.as_ref().map(|t| t.id());
        if let Some(id) = tex_id {
            let size = egui::vec2(self.size.0 as f32, self.size.1 as f32);
            let response =
                ui.add(egui::Image::new((id, size)).sense(egui::Sense::click_and_drag()));
            self.forward_input(ui, &response);
        } else {
            ui.centered_and_justified(|ui| {
                ui.weak("Loading web preview…");
            });
        }

        // Keep polling for async paints while the pane is visible.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }

    /// Translate egui pointer/scroll interaction into helper commands.
    fn forward_input(&mut self, ui: &egui::Ui, response: &egui::Response) {
        let origin = response.rect.min;
        let to_surface = |p: egui::Pos2| ((p.x - origin.x) as i32, (p.y - origin.y) as i32);

        // Pointer move (only meaningful while hovering).
        if let Some(pos) = response.hover_pos() {
            let (x, y) = to_surface(pos);
            self.send(Command::MouseMove { x, y, modifiers: 0, leaving: false });
        }

        // Button presses/releases. egui gives us press/release via input events;
        // map the primary/secondary/middle buttons.
        let pointer_pos = response
            .interact_pointer_pos()
            .or_else(|| response.hover_pos());
        if let Some(pos) = pointer_pos {
            let (x, y) = to_surface(pos);
            ui.input(|i| {
                for ev in &i.events {
                    if let egui::Event::PointerButton { button, pressed, .. } = ev {
                        if let Some(b) = map_button(*button) {
                            self.send(Command::MouseClick {
                                x,
                                y,
                                modifiers: 0,
                                button: b,
                                up: !*pressed,
                                click_count: 1,
                            });
                        }
                    }
                }
            });

            // Scroll wheel.
            let scroll = ui.input(|i| i.smooth_scroll_delta);
            if scroll != egui::Vec2::ZERO {
                self.send(Command::MouseWheel {
                    x,
                    y,
                    modifiers: 0,
                    delta_x: scroll.x as i32,
                    delta_y: scroll.y as i32,
                });
            }
        }
    }

    /// Send one command to the helper; a write failure marks the session dead.
    fn send(&mut self, cmd: Command) {
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        let mut line = cmd.to_line();
        line.push('\n');
        if stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()).is_err() {
            self.error = Some("Lost the connection to the web preview helper.".to_string());
            self.stdin = None;
        }
    }
}

impl Drop for CefPreview {
    fn drop(&mut self) {
        // Ask the helper to close cleanly, then ensure it's gone, then drop the
        // mapping and delete the temp file (Windows won't remove a mapped file
        // while it's still open, so order matters).
        self.send(Command::Shutdown);
        // Give it a brief moment, then hard-stop.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Drop the map before deleting: replace with an empty mapping is awkward,
        // so rely on field drop order — `map` drops after this method returns.
        // Deleting here can fail on Windows if the view is still mapped; try, and
        // if it fails the OS cleans the temp file up on reboot.
        let _ = std::fs::remove_file(&self.shm_path);
    }
}

fn map_button(b: egui::PointerButton) -> Option<MouseButton> {
    match b {
        egui::PointerButton::Primary => Some(MouseButton::Left),
        egui::PointerButton::Secondary => Some(MouseButton::Right),
        egui::PointerButton::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

fn clamp_size(size: (u32, u32)) -> (u32, u32) {
    (size.0.clamp(1, ipc::MAX_WIDTH), size.1.clamp(1, ipc::MAX_HEIGHT))
}

/// Convert a BGRA frame (top-to-bottom) to an egui `ColorImage`.
fn bgra_to_color_image(width: u32, height: u32, bgra: &[u8]) -> egui::ColorImage {
    let (w, h) = (width as usize, height as usize);
    let mut rgba = vec![0u8; w * h * 4];
    let (dst_px, _) = rgba.as_chunks_mut::<4>();
    let (src_px, _) = bgra.as_chunks::<4>();
    for (dst, src) in dst_px.iter_mut().zip(src_px) {
        dst[0] = src[2]; // R <- B position
        dst[1] = src[1]; // G
        dst[2] = src[0]; // B <- R position
        dst[3] = src[3]; // A
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}

/// Path to the `forge_cef` helper installed beside the running IDE executable,
/// falling back to the bare name (found via PATH) for `cargo run` layouts.
fn helper_binary() -> PathBuf {
    let name = if cfg!(windows) { "forge_cef.exe" } else { "forge_cef" };
    std::env::current_exe()
        .ok()
        .and_then(|here| here.parent().map(|dir| dir.join(name)))
        .filter(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// A unique temp path for this session's frame buffer.
fn unique_shm_path() -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("forge-cef-{}-{n}.frame", std::process::id());
    std::env::temp_dir().join(name)
}

/// On Windows, prevent a console window from flashing when spawning the helper.
#[cfg(windows)]
fn no_console_window(cmd: &mut Proc) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_console_window(_cmd: &mut Proc) {}

/// Convert a local file path to a `file://` URL the preview can load.
pub fn file_url(path: &Path) -> String {
    let absolute = std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned());
    let cleaned = absolute.trim_start_matches(r"\\?\").replace('\\', "/");
    format!("file:///{cleaned}")
}
