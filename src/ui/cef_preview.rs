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
use forge_cef_ipc::{self as ipc, Command, KeyKind, MouseButton};
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
    /// Current offscreen surface size in logical points.
    size: (u32, u32),
    /// Last device scale (pixels-per-point) sent to the helper.
    scale: f32,
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
            scale: 1.0,
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

        // Resize the surface to the tile (logical points), and track the
        // display's device scale so the helper renders at physical resolution
        // (crisp on hi-DPI). Resend when either the size or the scale changes.
        let avail = ui.available_size();
        let want = clamp_size((avail.x.max(1.0) as u32, avail.y.max(1.0) as u32));
        let ppp = ui.ctx().pixels_per_point();
        if want != self.size || (ppp - self.scale).abs() > 0.01 {
            self.size = want;
            self.scale = ppp;
            self.send(Command::Resize {
                width: want.0,
                height: want.1,
                device_scale: ppp,
            });
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
            // Clicking the preview gives it keyboard focus so typing is routed
            // to the page rather than the editor.
            if response.clicked() {
                response.request_focus();
            }
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

        // Keyboard: only while the preview tile holds focus, so typing doesn't
        // leak from the editor. Key up/down carry a Windows VK code (for
        // navigation/editing/shortcuts); Text events become CHAR events (the
        // actual character insertion).
        if response.has_focus() {
            ui.input(|i| {
                for ev in &i.events {
                    match ev {
                        egui::Event::Key { key, pressed, modifiers, .. } => {
                            self.send(Command::Key {
                                kind: if *pressed { KeyKind::KeyDown } else { KeyKind::KeyUp },
                                modifiers: cef_modifiers(modifiers),
                                windows_key_code: vk_code(*key),
                                character: 0,
                            });
                        }
                        egui::Event::Text(text) => {
                            for unit in text.encode_utf16() {
                                self.send(Command::Key {
                                    kind: KeyKind::Char,
                                    modifiers: 0,
                                    windows_key_code: unit as i32,
                                    character: unit,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            });
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

/// CEF event-flag bitmask (`cef_event_flags_t`) for egui modifiers.
fn cef_modifiers(m: &egui::Modifiers) -> u32 {
    const SHIFT_DOWN: u32 = 1 << 1;
    const CONTROL_DOWN: u32 = 1 << 2;
    const ALT_DOWN: u32 = 1 << 3;
    let mut flags = 0;
    if m.shift {
        flags |= SHIFT_DOWN;
    }
    if m.ctrl || m.command {
        flags |= CONTROL_DOWN;
    }
    if m.alt {
        flags |= ALT_DOWN;
    }
    flags
}

/// Windows virtual-key code for an egui key (0 when we don't map it — such keys
/// still type via the accompanying Text/CHAR event). Covers letters, digits,
/// function keys, and the navigation/editing keys the page needs by VK.
fn vk_code(key: egui::Key) -> i32 {
    use egui::Key::*;
    let code: u32 = match key {
        Backspace => 0x08,
        Tab => 0x09,
        Enter => 0x0D,
        Escape => 0x1B,
        Space => 0x20,
        PageUp => 0x21,
        PageDown => 0x22,
        End => 0x23,
        Home => 0x24,
        ArrowLeft => 0x25,
        ArrowUp => 0x26,
        ArrowRight => 0x27,
        ArrowDown => 0x28,
        Insert => 0x2D,
        Delete => 0x2E,
        Num0 => 0x30,
        Num1 => 0x31,
        Num2 => 0x32,
        Num3 => 0x33,
        Num4 => 0x34,
        Num5 => 0x35,
        Num6 => 0x36,
        Num7 => 0x37,
        Num8 => 0x38,
        Num9 => 0x39,
        A => 0x41,
        B => 0x42,
        C => 0x43,
        D => 0x44,
        E => 0x45,
        F => 0x46,
        G => 0x47,
        H => 0x48,
        I => 0x49,
        J => 0x4A,
        K => 0x4B,
        L => 0x4C,
        M => 0x4D,
        N => 0x4E,
        O => 0x4F,
        P => 0x50,
        Q => 0x51,
        R => 0x52,
        S => 0x53,
        T => 0x54,
        U => 0x55,
        V => 0x56,
        W => 0x57,
        X => 0x58,
        Y => 0x59,
        Z => 0x5A,
        F1 => 0x70,
        F2 => 0x71,
        F3 => 0x72,
        F4 => 0x73,
        F5 => 0x74,
        F6 => 0x75,
        F7 => 0x76,
        F8 => 0x77,
        F9 => 0x78,
        F10 => 0x79,
        F11 => 0x7A,
        F12 => 0x7B,
        _ => 0,
    };
    code as i32
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
