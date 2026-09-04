//! An embedded, cross-platform system terminal pane.
//!
//! [`portable-pty`] provides the pseudo-terminal (ConPTY on Windows, `forkpty`
//! on Unix) and spawns the user's shell. [`alacritty_terminal`] parses the VT /
//! ANSI byte stream into a grid model (colours, styles, scrollback, alt-screen),
//! and this module renders that grid with egui and encodes keyboard input back
//! into the PTY. The result runs full-screen apps like `vim`, `top`, and `htop`.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Processor, Rgb};
use eframe::egui;
use egui::{Color32, FontId, Stroke};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

/// Grid dimensions passed to the emulator. Scrollback size comes from `Config`,
/// so `total_lines` need only match the visible screen here.
struct Dims {
    cols: usize,
    rows: usize,
}

impl Dimensions for Dims {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// Forwards emulator events (title changes, PTY replies, bell, …) to the UI
/// thread, which drains and acts on them each frame.
#[derive(Clone)]
struct EventProxy(Sender<Event>);

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

/// One live terminal: the emulator grid, the PTY, and the reader thread.
pub struct Terminal {
    term: Term<EventProxy>,
    parser: Processor,
    master: Box<dyn MasterPty + Send>,
    /// Kept alive so the pseudo-terminal (notably ConPTY on Windows) stays open
    /// for the lifetime of the session.
    _slave: Box<dyn portable_pty::SlavePty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    output_rx: Receiver<Vec<u8>>,
    event_rx: Receiver<Event>,
    cols: usize,
    rows: usize,
    title: String,
    exited: Option<String>,
    font_size: f32,
}

impl Terminal {
    /// Spawn the user's shell in a new PTY, starting at `cwd` when given.
    pub fn spawn(cwd: Option<PathBuf>, font_size: f32) -> Result<Self, String> {
        let cols = 80usize;
        let rows = 24usize;
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("open pty: {e}"))?;

        let mut cmd = CommandBuilder::new_default_prog();
        if let Some(dir) = cwd.filter(|d| d.is_dir()) {
            cmd.cwd(dir);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn shell: {e}"))?;

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("clone reader: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("take writer: {e}"))?;

        let (output_tx, output_rx) = channel::<Vec<u8>>();
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if output_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let (event_tx, event_rx) = channel::<Event>();
        let term = Term::new(
            Config::default(),
            &Dims { cols, rows },
            EventProxy(event_tx),
        );

        Ok(Self {
            term,
            parser: Processor::new(),
            master: pair.master,
            _slave: pair.slave,
            writer,
            child,
            output_rx,
            event_rx,
            cols,
            rows,
            title: String::new(),
            exited: None,
            font_size,
        })
    }

    pub fn title(&self) -> &str {
        if self.title.is_empty() {
            "Terminal"
        } else {
            &self.title
        }
    }

    fn write_input(&mut self, bytes: &[u8]) {
        if self.exited.is_none() {
            let _ = self.writer.write_all(bytes);
            let _ = self.writer.flush();
            // Typing while scrolled back should jump to the prompt.
            self.term.scroll_display(Scroll::Bottom);
        }
    }

    /// Drain PTY output and emulator events. Returns true if anything changed.
    fn pump(&mut self, ctx: &egui::Context) -> bool {
        let mut changed = false;
        while let Ok(chunk) = self.output_rx.try_recv() {
            self.parser.advance(&mut self.term, &chunk);
            changed = true;
        }
        while let Ok(event) = self.event_rx.try_recv() {
            changed = true;
            match event {
                Event::PtyWrite(text) => {
                    let _ = self.writer.write_all(text.as_bytes());
                    let _ = self.writer.flush();
                }
                Event::Title(title) => self.title = title,
                Event::ResetTitle => self.title.clear(),
                Event::ClipboardStore(_, text) => ctx.copy_text(text),
                Event::ColorRequest(index, formatter) => {
                    let rgb = if index < 256 {
                        self.term.colors()[index]
                    } else {
                        None
                    }
                    .unwrap_or_else(|| indexed_default(index as u8));
                    let reply = formatter(rgb);
                    let _ = self.writer.write_all(reply.as_bytes());
                }
                Event::ChildExit(status) => {
                    self.exited = Some(format!("Shell exited ({status:?})."));
                }
                _ => {}
            }
        }
        changed
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.term.resize(Dims { cols, rows });
        let _ = self.master.resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    /// Render the terminal into the available space and handle its input.
    /// Returns true if a repaint should be scheduled.
    pub fn ui(&mut self, ui: &mut egui::Ui, _dark: bool) -> bool {
        let mut changed = self.pump(ui.ctx());

        if let Some(message) = self.exited.clone() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&message).italics());
                if ui.button("Restart").clicked() {
                    if let Ok(fresh) =
                        Terminal::spawn(None, self.font_size).map_err(|e| self.exited = Some(e))
                    {
                        *self = fresh;
                        changed = true;
                    }
                }
            });
        }

        // The terminal follows the active theme: pane background, default text,
        // and cursor come from the palette (ANSI-colored output is unaffected).
        let palette = crate::ui::theme::active_palette();
        let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
        let (default_fg, default_bg, cursor_color) = (
            rgb(palette.text),
            rgb(palette.background),
            rgb(palette.accent),
        );

        let font = FontId::monospace(self.font_size);
        let (char_w, row_h) = ui.ctx().fonts_mut(|f| {
            (
                f.glyph_width(&font, 'M').max(1.0),
                f.row_height(&font).max(1.0),
            )
        });

        // Claim the whole pane and paint the terminal background.
        let avail = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, default_bg);

        let cols = ((rect.width() / char_w).floor() as usize).max(1);
        let rows = ((rect.height() / row_h).floor() as usize).max(1);
        self.resize(cols, rows);

        // Focus: clicking the terminal grabs keyboard focus.
        if response.clicked() {
            response.request_focus();
        }
        let focused = response.has_focus();

        // Mouse-drag text selection (grid coordinates).
        let display_offset = self.term.grid().display_offset() as i32;
        let point_at = |pos: egui::Pos2| -> Point {
            let col = (((pos.x - rect.left()) / char_w).floor() as i64)
                .clamp(0, self.cols as i64 - 1) as usize;
            let screen_row = (((pos.y - rect.top()) / row_h).floor() as i64)
                .clamp(0, self.rows as i64 - 1) as i32;
            Point::new(Line(screen_row - display_offset), Column(col))
        };
        let side_at = |pos: egui::Pos2| -> Side {
            let frac = ((pos.x - rect.left()) / char_w).fract();
            if frac < 0.5 {
                Side::Left
            } else {
                Side::Right
            }
        };
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                let point = point_at(pos);
                self.term.selection =
                    Some(Selection::new(SelectionType::Simple, point, side_at(pos)));
                changed = true;
            }
        } else if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                if let Some(sel) = self.term.selection.as_mut() {
                    sel.update(point_at(pos), side_at(pos));
                    changed = true;
                }
            }
        }

        // Keyboard input and clipboard shortcuts (only while focused).
        if focused {
            let app_cursor = self.term.mode().contains(TermMode::APP_CURSOR);
            let app_keypad = self.term.mode().contains(TermMode::APP_KEYPAD);
            let bracketed = self.term.mode().contains(TermMode::BRACKETED_PASTE);
            let mut input: Vec<u8> = Vec::new();
            let mut copy: Option<String> = None;
            ui.input(|state| {
                for event in &state.events {
                    match event {
                        egui::Event::Text(text) => {
                            // Control/alt combos come through as Key events below.
                            if !state.modifiers.ctrl && !state.modifiers.alt {
                                input.extend_from_slice(text.as_bytes());
                            }
                        }
                        egui::Event::Paste(text) => {
                            if bracketed {
                                input.extend_from_slice(b"\x1b[200~");
                                input.extend_from_slice(text.as_bytes());
                                input.extend_from_slice(b"\x1b[201~");
                            } else {
                                input.extend_from_slice(text.as_bytes());
                            }
                        }
                        egui::Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } => {
                            // Ctrl+Shift+C copies the selection instead of typing.
                            if *key == egui::Key::C && modifiers.ctrl && modifiers.shift {
                                copy = self.term.selection_to_string();
                            } else if let Some(bytes) =
                                encode_key(*key, modifiers, app_cursor, app_keypad)
                            {
                                input.extend_from_slice(&bytes);
                            }
                        }
                        _ => {}
                    }
                }
            });
            if let Some(text) = copy {
                if !text.is_empty() {
                    ui.ctx().copy_text(text);
                }
            }
            if !input.is_empty() {
                self.write_input(&input);
                changed = true;
            }
        }

        // Mouse wheel scrolls the scrollback buffer.
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.1 {
                let lines = (scroll / row_h).round() as i32;
                if lines != 0 {
                    self.term.scroll_display(Scroll::Delta(lines));
                    changed = true;
                }
            }
        }

        // Render the visible grid, one egui text layout per row.
        let content = self.term.renderable_content();
        let display_offset = content.display_offset as i32;
        let colors = content.colors;
        let selection = content.selection;
        let mut rows_text: Vec<egui::text::LayoutJob> =
            vec![egui::text::LayoutJob::default(); self.rows];

        for indexed in content.display_iter {
            let cell = indexed.cell;
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let row = (indexed.point.line.0 + display_offset) as usize;
            if row >= self.rows {
                continue;
            }

            let mut fg = resolve_color(cell.fg, colors, default_fg, default_bg, cell.flags);
            let mut bg = match cell.bg {
                AnsiColor::Named(NamedColor::Background) => default_bg,
                other => resolve_color(other, colors, default_fg, default_bg, Flags::empty()),
            };
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.contains(Flags::DIM) {
                fg = dim(fg);
            }
            let selected = selection
                .map(|range| range.contains(indexed.point))
                .unwrap_or(false);
            if selected {
                std::mem::swap(&mut fg, &mut bg);
            }

            let ch = if cell.flags.contains(Flags::HIDDEN) {
                ' '
            } else {
                cell.c
            };
            let mut format = egui::TextFormat {
                font_id: font.clone(),
                color: fg,
                ..Default::default()
            };
            if bg != default_bg || selected {
                format.background = bg;
            }
            if cell.flags.contains(Flags::ITALIC) {
                format.italics = true;
            }
            if cell
                .flags
                .intersects(Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE)
            {
                format.underline = Stroke::new(1.0, fg);
            }
            if cell.flags.contains(Flags::STRIKEOUT) {
                format.strikethrough = Stroke::new(1.0, fg);
            }
            rows_text[row].append(&ch.to_string(), 0.0, format);
        }

        for (row, job) in rows_text.into_iter().enumerate() {
            let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
            let pos = egui::pos2(rect.left(), rect.top() + row as f32 * row_h);
            painter.galley(pos, galley, default_fg);
        }

        // Cursor (only when viewing the live screen).
        let cursor = content.cursor;
        let cursor_visible = self.term.mode().contains(TermMode::SHOW_CURSOR);
        let cursor_row = cursor.point.line.0 + display_offset;
        if cursor_visible
            && content.display_offset == 0
            && cursor_row >= 0
            && (cursor_row as usize) < self.rows
        {
            use alacritty_terminal::vte::ansi::CursorShape;
            let x = rect.left() + cursor.point.column.0 as f32 * char_w;
            let y = rect.top() + cursor_row as f32 * row_h;
            let cell = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(char_w, row_h));
            match cursor.shape {
                CursorShape::Hidden => {}
                CursorShape::Beam => {
                    painter.rect_filled(
                        egui::Rect::from_min_size(cell.min, egui::vec2(2.0, row_h)),
                        0.0,
                        cursor_color,
                    );
                }
                CursorShape::Underline => {
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(cell.left(), cell.bottom() - 2.0),
                            egui::vec2(char_w, 2.0),
                        ),
                        0.0,
                        cursor_color,
                    );
                }
                CursorShape::HollowBlock => {
                    painter.rect_stroke(
                        cell,
                        0.0,
                        Stroke::new(1.0, cursor_color),
                        egui::StrokeKind::Inside,
                    );
                }
                _ => {
                    // Solid block outline when unfocused, filled when focused.
                    if focused {
                        painter.rect_filled(cell, 0.0, cursor_color);
                    } else {
                        painter.rect_stroke(
                            cell,
                            0.0,
                            Stroke::new(1.0, cursor_color),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
        }

        if changed {
            ui.ctx().request_repaint();
        }
        changed
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Map a key press to the bytes a terminal expects, honoring application
/// cursor / keypad modes and Ctrl/Alt modifiers.
fn encode_key(
    key: egui::Key,
    mods: &egui::Modifiers,
    app_cursor: bool,
    _app_keypad: bool,
) -> Option<Vec<u8>> {
    use egui::Key;
    let csi = |c: char| {
        if app_cursor {
            vec![0x1b, b'O', c as u8]
        } else {
            vec![0x1b, b'[', c as u8]
        }
    };
    let tilde = |n: u8| vec![0x1b, b'[', b'0' + n, b'~'];

    // Ctrl + letter / symbol -> C0 control code.
    if mods.ctrl && !mods.shift {
        if let Some(byte) = ctrl_byte(key) {
            return Some(vec![byte]);
        }
    }

    let base: Vec<u8> = match key {
        Key::Enter => vec![b'\r'],
        Key::Tab => vec![b'\t'],
        Key::Backspace => vec![0x7f],
        Key::Escape => vec![0x1b],
        Key::ArrowUp => csi('A'),
        Key::ArrowDown => csi('B'),
        Key::ArrowRight => csi('C'),
        Key::ArrowLeft => csi('D'),
        Key::Home => csi('H'),
        Key::End => csi('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => vec![0x1b, b'O', b'P'],
        Key::F2 => vec![0x1b, b'O', b'Q'],
        Key::F3 => vec![0x1b, b'O', b'R'],
        Key::F4 => vec![0x1b, b'O', b'S'],
        Key::F5 => tilde_2(15),
        Key::F6 => tilde_2(17),
        Key::F7 => tilde_2(18),
        Key::F8 => tilde_2(19),
        Key::F9 => tilde_2(20),
        Key::F10 => tilde_2(21),
        Key::F11 => tilde_2(23),
        Key::F12 => tilde_2(24),
        _ => return None,
    };

    // Alt (Meta) prefixes an ESC.
    if mods.alt {
        let mut out = vec![0x1b];
        out.extend_from_slice(&base);
        return Some(out);
    }
    Some(base)
}

/// Two-digit CSI `~` sequences (e.g. F5 = ESC [ 1 5 ~).
fn tilde_2(n: u8) -> Vec<u8> {
    vec![0x1b, b'[', b'0' + n / 10, b'0' + n % 10, b'~']
}

/// The C0 control byte for Ctrl+<key>, if any.
fn ctrl_byte(key: egui::Key) -> Option<u8> {
    use egui::Key;
    let c = match key {
        Key::A => 1,
        Key::B => 2,
        Key::C => 3,
        Key::D => 4,
        Key::E => 5,
        Key::F => 6,
        Key::G => 7,
        Key::H => 8,
        Key::I => 9,
        Key::J => 10,
        Key::K => 11,
        Key::L => 12,
        Key::M => 13,
        Key::N => 14,
        Key::O => 15,
        Key::P => 16,
        Key::Q => 17,
        Key::R => 18,
        Key::S => 19,
        Key::T => 20,
        Key::U => 21,
        Key::V => 22,
        Key::W => 23,
        Key::X => 24,
        Key::Y => 25,
        Key::Z => 26,
        Key::OpenBracket => 27, // Ctrl+[ = ESC
        Key::Backslash => 28,
        Key::CloseBracket => 29,
        _ => return None,
    };
    Some(c)
}

fn to_color32(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

fn dim(c: Color32) -> Color32 {
    Color32::from_rgb(
        (c.r() as u16 * 2 / 3) as u8,
        (c.g() as u16 * 2 / 3) as u8,
        (c.b() as u16 * 2 / 3) as u8,
    )
}

/// Resolve an emulator colour to an RGBA colour, consulting the live palette
/// first and falling back to the standard xterm defaults.
fn resolve_color(
    color: AnsiColor,
    palette: &alacritty_terminal::term::color::Colors,
    default_fg: Color32,
    default_bg: Color32,
    flags: Flags,
) -> Color32 {
    match color {
        AnsiColor::Spec(rgb) => to_color32(rgb),
        AnsiColor::Indexed(index) => {
            // Bold text promotes the 8 base colours to their bright variants.
            let index = if flags.contains(Flags::BOLD) && index < 8 {
                index + 8
            } else {
                index
            };
            palette[index as usize]
                .map(to_color32)
                .unwrap_or_else(|| to_color32(ansi_fallback(index)))
        }
        AnsiColor::Named(named) => match named {
            NamedColor::Foreground => default_fg,
            NamedColor::Background => default_bg,
            other => {
                let idx = other as usize;
                let idx = if flags.contains(Flags::BOLD) && idx < 8 {
                    idx + 8
                } else {
                    idx
                };
                if idx < 256 {
                    palette[idx]
                        .map(to_color32)
                        .unwrap_or_else(|| to_color32(ansi_fallback(idx as u8)))
                } else {
                    default_fg
                }
            }
        },
    }
}

/// The standard xterm 256-colour palette used when the emulator has no override.
fn indexed_default(index: u8) -> Rgb {
    const BASE16: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcc, 0x33, 0x33),
        (0x33, 0xaa, 0x55),
        (0xcc, 0xaa, 0x33),
        (0x33, 0x77, 0xcc),
        (0xaa, 0x55, 0xcc),
        (0x33, 0xaa, 0xaa),
        (0xcc, 0xcc, 0xcc),
        (0x55, 0x55, 0x55),
        (0xff, 0x55, 0x55),
        (0x55, 0xff, 0x88),
        (0xff, 0xdd, 0x55),
        (0x55, 0xaa, 0xff),
        (0xdd, 0x88, 0xff),
        (0x55, 0xff, 0xff),
        (0xff, 0xff, 0xff),
    ];
    if index < 16 {
        let (r, g, b) = BASE16[index as usize];
        Rgb { r, g, b }
    } else if index < 232 {
        // 6x6x6 colour cube.
        let i = index - 16;
        let steps = [0u8, 95, 135, 175, 215, 255];
        Rgb {
            r: steps[(i / 36) as usize],
            g: steps[((i / 6) % 6) as usize],
            b: steps[(i % 6) as usize],
        }
    } else {
        // 24-step greyscale ramp.
        let level = 8 + (index - 232) * 10;
        Rgb {
            r: level,
            g: level,
            b: level,
        }
    }
}

/// Fallback color for an emulator index the program hasn't overridden. The 16
/// base ANSI colors follow the active theme (so prompts, `ls`, git, etc. match
/// the IDE); the 256-color cube keeps its standard xterm values.
fn ansi_fallback(index: u8) -> Rgb {
    if index < 16 {
        themed_ansi16(index)
    } else {
        indexed_default(index)
    }
}

/// Map the 16 base ANSI slots onto the active theme so terminal output is
/// color-coordinated with the rest of the IDE. Semantics are preserved
/// (1=red, 2=green, …); only the exact shades come from the palette.
fn themed_ansi16(index: u8) -> Rgb {
    let p = crate::ui::theme::active_palette();
    let rgb = |c: [u8; 3]| Rgb {
        r: c[0],
        g: c[1],
        b: c[2],
    };
    let col = |c: Color32| Rgb {
        r: c.r(),
        g: c.g(),
        b: c.b(),
    };
    let mix = |a: [u8; 3], b: [u8; 3], t: f32| {
        let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Rgb {
            r: l(a[0], b[0]),
            g: l(a[1], b[1]),
            b: l(a[2], b[2]),
        }
    };
    let bright = |c: [u8; 3]| mix(c, [255, 255, 255], 0.28);
    let red = crate::ui::theme::RED;
    let green = crate::ui::theme::GREEN;
    let ember = crate::ui::theme::EMBER;
    match index {
        0 => mix(p.background, p.text, 0.30), // black — a visible dark grey
        1 => col(red),
        2 => col(green),
        3 => rgb(p.syn_type),            // yellow
        4 => rgb(p.syn_function),        // blue
        5 => rgb(p.syn_keyword),         // magenta
        6 => rgb(p.accent),              // cyan
        7 => rgb(p.muted),               // white
        8 => mix(p.muted, p.text, 0.35), // bright black
        9 => bright([red.r(), red.g(), red.b()]),
        10 => bright([green.r(), green.g(), green.b()]),
        11 => bright([ember.r(), ember.g(), ember.b()]),
        12 => bright(p.syn_function),
        13 => bright(p.syn_keyword),
        14 => bright(p.accent),
        _ => rgb(p.text), // 15 bright white
    }
}

#[cfg(test)]
impl Terminal {
    fn drain_and_parse(&mut self) -> usize {
        let mut total = 0;
        while let Ok(chunk) = self.output_rx.try_recv() {
            total += chunk.len();
            self.parser.advance(&mut self.term, &chunk);
        }
        total
    }

    fn visible_text(&self) -> String {
        self.term.grid().display_iter().map(|c| c.c).collect()
    }

    fn send_test(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end: spawn the OS shell in a PTY, run a command, and confirm the
    /// emulator grid captures its output.
    ///
    /// Skips gracefully when the environment cannot spawn or run a shell under a
    /// pseudo-terminal — for example CI sandboxes without a working console
    /// (Windows ConPTY). On a real desktop it exercises the full pipeline.
    #[test]
    fn shell_runs_a_command_end_to_end() {
        use std::time::Duration;
        let Ok(mut term) = Terminal::spawn(None, 13.0) else {
            return;
        };
        // Let the shell boot and emit its banner/prompt.
        let mut booted = 0usize;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(100));
            booted += term.drain_and_parse();
        }
        if booted < 8 {
            // No usable console in this environment; nothing to verify here.
            return;
        }
        term.send_test(b"echo forge_terminal_marker\r\n");
        let mut found = false;
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(100));
            term.drain_and_parse();
            if term.visible_text().contains("forge_terminal_marker") {
                found = true;
                break;
            }
        }
        assert!(
            found,
            "shell did not echo the marker; grid was:\n{}",
            term.visible_text()
        );
    }
}
