//! In-editor preview for Markdown and HTML files.
//!
//! Markdown is rendered in-app with a compact renderer (headings, lists, block
//! quotes, fenced code, rules, and inline bold / `code` / links) — no external
//! egui-coupled dependency, so it can't drift from the pinned egui version. HTML
//! can't be rendered inside egui without a browser engine, so its preview opens
//! the file in the system browser (full fidelity) and shows the source.

use crate::ui::theme::{accent, MUTED, TEXT};
use eframe::egui;
use egui::{Color32, RichText};
use egui_phosphor_icons::icons;
use std::path::Path;
use std::process::Command;

const CODE_BG: Color32 = Color32::from_rgb(0x24, 0x28, 0x31);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Markdown,
    Html,
}

/// How a previewable file is shown in the editor pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewMode {
    #[default]
    Edit,
    Split,
    Preview,
}

/// Whether `path` is previewable, and how.
pub fn kind_for(path: &Path) -> Option<PreviewKind> {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("md" | "markdown" | "mkd" | "mdown") => Some(PreviewKind::Markdown),
        Some("html" | "htm" | "xhtml") => Some(PreviewKind::Html),
        _ => None,
    }
}

/// Render the preview for `source` (the current editor contents) of a file at
/// `path` (used by the HTML preview to open the saved file).
pub fn render(ui: &mut egui::Ui, kind: PreviewKind, source: &str, path: Option<&Path>) {
    match kind {
        PreviewKind::Markdown => markdown(ui, source),
        PreviewKind::Html => html(ui, source, path),
    }
}

// ── Markdown ──────────────────────────────────────────────────────────────────

fn markdown(ui: &mut egui::Ui, source: &str) {
    let mut lines = source.lines().peekable();
    while let Some(raw) = lines.next() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();

        // Fenced code block.
        if let Some(rest) = trimmed.strip_prefix("```") {
            let _lang = rest;
            let mut code = String::new();
            for inner in lines.by_ref() {
                if inner.trim_start().starts_with("```") {
                    break;
                }
                code.push_str(inner);
                code.push('\n');
            }
            code_block(ui, code.trim_end_matches('\n'));
            continue;
        }

        if line.is_empty() {
            ui.add_space(7.0);
            continue;
        }
        if matches!(trimmed, "---" | "***" | "___") {
            ui.add_space(4.0);
            ui.separator();
            continue;
        }
        if let Some((level, rest)) = heading(trimmed) {
            let size = match level {
                1 => 26.0,
                2 => 21.0,
                3 => 17.5,
                _ => 15.0,
            };
            ui.add_space(if level <= 2 { 8.0 } else { 4.0 });
            ui.label(RichText::new(rest).size(size).strong().color(TEXT));
            ui.add_space(2.0);
            continue;
        }
        if let Some(quote) = trimmed.strip_prefix("> ").or_else(|| trimmed.strip_prefix(">")) {
            ui.horizontal_top(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("|").size(16.0).strong().color(accent()));
                inline(ui, quote.trim_start(), MUTED);
            });
            continue;
        }
        if let Some((marker, item)) = list_item(trimmed) {
            let indent = 8.0 + 14.0 * leading_spaces(line) as f32;
            ui.horizontal_top(|ui| {
                ui.add_space(indent);
                ui.label(RichText::new(marker).color(accent()));
                inline(ui, item, TEXT);
            });
            continue;
        }
        inline(ui, line, TEXT);
    }
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) {
        if let Some(rest) = line[hashes..].strip_prefix(' ') {
            return Some((hashes, rest.trim_start()));
        }
    }
    None
}

/// The bullet marker to draw and the item text, for `-`/`*`/`+` and `N.` lists.
fn list_item(line: &str) -> Option<(&'static str, &str)> {
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some((icons::DOT.as_str(), rest));
        }
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        if let Some(rest) = line[digits..].strip_prefix(". ") {
            return Some(("–", rest.trim_start()));
        }
    }
    None
}

fn leading_spaces(line: &str) -> usize {
    (line.chars().take_while(|&c| c == ' ').count() / 2).min(4)
}

fn code_block(ui: &mut egui::Ui, code: &str) {
    egui::Frame::NONE
        .fill(CODE_BG)
        .inner_margin(egui::Margin::same(10))
        .corner_radius(egui::CornerRadius::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(code).monospace().size(12.5).color(TEXT));
        });
    ui.add_space(4.0);
}

// ── Inline spans ──────────────────────────────────────────────────────────────

enum Span {
    Text(String),
    Bold(String),
    Code(String),
    Link(String, String),
}

fn inline(ui: &mut egui::Ui, text: &str, base: Color32) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.spacing_mut().item_spacing.y = 2.0;
        for span in parse_inline(text) {
            match span {
                Span::Text(s) => words(ui, &s, base, false),
                Span::Bold(s) => words(ui, &s, base, true),
                Span::Code(s) => {
                    ui.label(
                        RichText::new(s)
                            .monospace()
                            .size(12.5)
                            .color(accent())
                            .background_color(CODE_BG),
                    );
                }
                Span::Link(label, url) => {
                    if ui.link(RichText::new(label).color(accent())).clicked() {
                        open(&url);
                    }
                }
            }
        }
    });
}

/// Add plain words individually so the wrapped layout breaks between words.
fn words(ui: &mut egui::Ui, text: &str, color: Color32, bold: bool) {
    for word in text.split_whitespace() {
        let mut rich = RichText::new(word).color(color);
        if bold {
            rich = rich.strong();
        }
        ui.label(rich);
    }
}

/// A small inline parser: `` `code` ``, `**bold**`, and `[label](url)`; the rest
/// is plain text. Unclosed markers fall back to plain.
fn parse_inline(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &text[i..];
        if let Some(inner) = rest.strip_prefix('`').and_then(|r| r.split_once('`')) {
            flush(&mut plain, &mut spans);
            spans.push(Span::Code(inner.0.to_owned()));
            i += 1 + inner.0.len() + 1;
        } else if let Some(after) = rest.strip_prefix("**") {
            if let Some((bold, _)) = after.split_once("**") {
                flush(&mut plain, &mut spans);
                spans.push(Span::Bold(bold.to_owned()));
                i += 2 + bold.len() + 2;
            } else {
                plain.push_str("**");
                i += 2;
            }
        } else if let Some(after) = rest.strip_prefix('[') {
            if let Some((label, tail)) = after.split_once("](") {
                if let Some((url, _)) = tail.split_once(')') {
                    flush(&mut plain, &mut spans);
                    spans.push(Span::Link(label.to_owned(), url.to_owned()));
                    i += 1 + label.len() + 2 + url.len() + 1;
                    continue;
                }
            }
            plain.push('[');
            i += 1;
        } else {
            let ch = rest.chars().next().unwrap();
            plain.push(ch);
            i += ch.len_utf8();
        }
    }
    flush(&mut plain, &mut spans);
    spans
}

fn flush(plain: &mut String, spans: &mut Vec<Span>) {
    if !plain.is_empty() {
        spans.push(Span::Text(std::mem::take(plain)));
    }
}

// ── HTML ──────────────────────────────────────────────────────────────────────

fn html(ui: &mut egui::Ui, source: &str, path: Option<&Path>) {
    let on_disk = path.filter(|p| p.exists());
    ui.horizontal(|ui| {
        // Primary: full CSS/JS rendering in Forge's own WebView window.
        let rendered = egui::Button::new(
            RichText::new("Open rendered preview")
                .color(Color32::WHITE)
                .strong(),
        )
        .fill(accent());
        if ui.add_enabled(on_disk.is_some(), rendered).clicked() {
            if let Some(path) = on_disk {
                open_in_forge_webview(path);
            }
        }
        // Secondary: the external system browser.
        let browser = format!("Open in browser  {}", icons::ARROW_SQUARE_OUT.as_str());
        if ui.add_enabled(on_disk.is_some(), egui::Button::new(browser)).clicked() {
            if let Some(path) = on_disk {
                open_path(path);
            }
        }
    });
    ui.label(
        RichText::new(if on_disk.is_some() {
            "Rendered preview opens a real WebView in Forge. The quick structural view is below."
        } else {
            "Save the file to open the rendered preview. The quick structural view is below."
        })
        .color(MUTED)
        .size(12.0),
    );
    ui.add_space(8.0);
    render_blocks(ui, &parse_html(source));
}

/// A minimal structural HTML renderer: it lays out the common document tags with
/// egui widgets. It is not a browser — it ignores CSS and scripts and only
/// approximates layout — but it renders headings, paragraphs, lists, links,
/// inline/preformatted code, block quotes, rules, images (as placeholders), and
/// simple tables inline in the IDE.

#[derive(Clone, Copy, Default)]
struct Style {
    bold: bool,
    italic: bool,
    code: bool,
}

enum Run {
    Text(String, Style),
    Link(String, String),
}

enum Block {
    Heading(u8, String),
    Para(Vec<Run>),
    Item { ordered: Option<usize>, depth: usize, runs: Vec<Run> },
    Pre(String),
    Quote(Vec<Run>),
    Rule,
    Row { header: bool, cells: Vec<Vec<Run>> },
    Image(String),
}

fn render_blocks(ui: &mut egui::Ui, blocks: &[Block]) {
    for block in blocks {
        match block {
            Block::Heading(level, text) => {
                let size = match level {
                    1 => 26.0,
                    2 => 21.0,
                    3 => 17.5,
                    _ => 15.0,
                };
                ui.add_space(if *level <= 2 { 8.0 } else { 4.0 });
                ui.label(RichText::new(text).size(size).strong().color(TEXT));
                ui.add_space(2.0);
            }
            Block::Para(runs) => {
                render_runs(ui, runs, TEXT);
                ui.add_space(4.0);
            }
            Block::Item { ordered, depth, runs } => {
                ui.horizontal_top(|ui| {
                    ui.add_space(8.0 + 16.0 * *depth as f32);
                    match ordered {
                        Some(n) => ui.label(RichText::new(format!("{n}.")).color(accent())),
                        None => ui.label(RichText::new(icons::DOT.as_str()).color(accent())),
                    };
                    render_runs(ui, runs, TEXT);
                });
            }
            Block::Pre(code) => code_block(ui, code),
            Block::Quote(runs) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("|").size(16.0).strong().color(accent()));
                    render_runs(ui, runs, MUTED);
                });
            }
            Block::Rule => {
                ui.add_space(4.0);
                ui.separator();
            }
            Block::Image(alt) => {
                ui.label(RichText::new(format!("[image: {alt}]")).italics().color(MUTED));
            }
            Block::Row { header, cells } => {
                ui.horizontal_top(|ui| {
                    for cell in cells {
                        ui.add_space(2.0);
                        let base = if *header { TEXT } else { MUTED };
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::symmetric(6, 2))
                            .show(ui, |ui| render_runs(ui, cell, base));
                    }
                });
            }
        }
    }
}

fn render_runs(ui: &mut egui::Ui, runs: &[Run], base: Color32) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.spacing_mut().item_spacing.y = 2.0;
        for run in runs {
            match run {
                Run::Text(text, style) => {
                    for word in text.split_whitespace() {
                        let mut rich = RichText::new(word).color(base);
                        if style.bold {
                            rich = rich.strong();
                        }
                        if style.italic {
                            rich = rich.italics();
                        }
                        if style.code {
                            rich = rich.monospace().color(accent()).background_color(CODE_BG);
                        }
                        ui.label(rich);
                    }
                }
                Run::Link(text, href) => {
                    if ui.link(RichText::new(text).color(accent())).clicked() {
                        open(href);
                    }
                }
            }
        }
    });
}

/// Parse a document's worth of HTML into renderable blocks. Best-effort and
/// forgiving: unknown tags are ignored (their text still shows), scripts and
/// styles are dropped, and malformed markup degrades to text.
fn parse_html(source: &str) -> Vec<Block> {
    let mut w = Walker::default();
    let mut rest = source;
    while !rest.is_empty() {
        if let Some(open) = rest.find('<') {
            if open > 0 {
                w.text(&decode_entities(&rest[..open]));
            }
            rest = &rest[open..];
            let Some(close) = rest.find('>') else {
                w.text(&decode_entities(rest));
                break;
            };
            let inner = &rest[1..close];
            rest = &rest[close + 1..];
            if inner.starts_with("!--") || inner.starts_with('!') {
                continue; // comment / doctype
            }
            if let Some(name) = inner.strip_prefix('/') {
                w.close(name.trim().trim_end_matches('/').to_ascii_lowercase().as_str());
            } else {
                let inner = inner.trim();
                let (name, attrs) = match inner.find(|c: char| c.is_whitespace()) {
                    Some(sp) => (&inner[..sp], inner[sp..].trim().trim_end_matches('/')),
                    None => (inner.trim_end_matches('/'), ""),
                };
                w.open(&name.to_ascii_lowercase(), attrs);
            }
        } else {
            w.text(&decode_entities(rest));
            break;
        }
    }
    w.finish()
}

#[derive(Default)]
struct Walker {
    blocks: Vec<Block>,
    runs: Vec<Run>,
    style: Style,
    link: Option<String>,
    heading: Option<u8>,
    in_quote: bool,
    list: Vec<Option<usize>>,
    pre: Option<String>,
    skip: bool, // inside <script>/<style>
    cell: Option<Vec<Run>>,
    row: Option<(bool, Vec<Vec<Run>>)>,
}

impl Walker {
    fn text(&mut self, text: &str) {
        if self.skip {
            return;
        }
        if let Some(buf) = &mut self.pre {
            buf.push_str(text);
            return;
        }
        if text.trim().is_empty() && self.runs.is_empty() && self.cell.is_none() {
            return;
        }
        let run = Run::Text(text.to_owned(), self.style);
        match &mut self.cell {
            Some(cell) => cell.push(run),
            None => {
                if let Some(href) = &self.link {
                    self.runs
                        .push(Run::Link(text.trim().to_owned(), href.clone()));
                } else {
                    self.runs.push(run);
                }
            }
        }
    }

    fn open(&mut self, name: &str, attrs: &str) {
        match name {
            "script" | "style" => self.skip = true,
            "br" => self.flush(),
            "hr" => {
                self.flush();
                self.blocks.push(Block::Rule);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                self.heading = Some(name.as_bytes()[1] - b'0');
            }
            "p" | "div" | "section" | "article" | "header" | "footer" | "main" => self.flush(),
            "ul" | "ol" => self.list.push((name == "ol").then_some(1)),
            "li" => self.flush(),
            "blockquote" => {
                self.flush();
                self.in_quote = true;
            }
            "pre" => {
                self.flush();
                self.pre = Some(String::new());
            }
            "strong" | "b" => self.style.bold = true,
            "em" | "i" => self.style.italic = true,
            "code" if self.pre.is_none() => self.style.code = true,
            "a" => self.link = attr(attrs, "href"),
            "img" => {
                let alt = attr(attrs, "alt").or_else(|| attr(attrs, "src")).unwrap_or_default();
                self.blocks.push(Block::Image(alt));
            }
            "table" => self.flush(),
            "tr" => self.row = Some((false, Vec::new())),
            "td" | "th" => {
                if name == "th" {
                    if let Some(row) = &mut self.row {
                        row.0 = true;
                    }
                }
                self.cell = Some(Vec::new());
            }
            _ => {}
        }
    }

    fn close(&mut self, name: &str) {
        match name {
            "script" | "style" => self.skip = false,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.flush(),
            "p" | "div" | "section" | "article" | "header" | "footer" | "main" => self.flush(),
            "li" => self.flush(),
            "ul" | "ol" => {
                self.list.pop();
            }
            "blockquote" => {
                self.flush();
                self.in_quote = false;
            }
            "pre" => {
                if let Some(code) = self.pre.take() {
                    self.blocks.push(Block::Pre(code.trim_matches('\n').to_owned()));
                }
            }
            "strong" | "b" => self.style.bold = false,
            "em" | "i" => self.style.italic = false,
            "code" => self.style.code = false,
            "a" => self.link = None,
            "td" | "th" => {
                if let (Some(cell), Some(row)) = (self.cell.take(), &mut self.row) {
                    row.1.push(cell);
                }
            }
            "tr" => {
                if let Some((header, cells)) = self.row.take() {
                    if !cells.is_empty() {
                        self.blocks.push(Block::Row { header, cells });
                    }
                }
            }
            _ => {}
        }
    }

    /// Emit the accumulated inline runs as the appropriate block, and clear.
    fn flush(&mut self) {
        if self.runs.is_empty() {
            return;
        }
        let runs = std::mem::take(&mut self.runs);
        let block = if let Some(level) = self.heading.take() {
            Block::Heading(level, runs_to_text(&runs))
        } else if self.in_quote {
            Block::Quote(runs)
        } else if !self.list.is_empty() {
            let depth = self.list.len().saturating_sub(1);
            let ordered = self.list.last_mut().and_then(|counter| {
                counter.as_mut().map(|n| {
                    let current = *n;
                    *n += 1;
                    current
                })
            });
            Block::Item { ordered, depth, runs }
        } else {
            Block::Para(runs)
        };
        self.blocks.push(block);
    }

    fn finish(mut self) -> Vec<Block> {
        self.flush();
        self.blocks
    }
}

fn runs_to_text(runs: &[Run]) -> String {
    let mut out = String::new();
    for run in runs {
        match run {
            Run::Text(text, _) => out.push_str(text.trim()),
            Run::Link(text, _) => out.push_str(text),
        }
        out.push(' ');
    }
    out.trim().to_owned()
}

/// Extract the value of attribute `key` from a raw attribute string.
fn attr(attrs: &str, key: &str) -> Option<String> {
    let start = attrs.find(key)?;
    let after = attrs[start + key.len()..].trim_start();
    let after = after.strip_prefix('=')?.trim_start();
    let (quote, body) = match after.chars().next()? {
        q @ ('"' | '\'') => (Some(q), &after[1..]),
        _ => (None, after),
    };
    let end = match quote {
        Some(q) => body.find(q)?,
        None => body.find(char::is_whitespace).unwrap_or(body.len()),
    };
    Some(decode_entities(&body[..end]))
}

/// Decode the handful of HTML entities that matter for a text preview.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        if let Some(semi) = rest[..rest.len().min(12)].find(';') {
            let entity = &rest[1..semi];
            let decoded = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some(' '),
                other => other
                    .strip_prefix("#x")
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| other.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                    .and_then(char::from_u32),
            };
            if let Some(ch) = decoded {
                out.push(ch);
                rest = &rest[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

// ── Opening things ────────────────────────────────────────────────────────────

fn open(target: &str) {
    open_raw(target);
}

fn open_path(path: &Path) {
    open_raw(&path.to_string_lossy());
}

/// Open the file in Forge's own WebView window (`forge_webview`, installed beside
/// the app), for full CSS/JS rendering without an external browser.
fn open_in_forge_webview(path: &Path) {
    let exe = if cfg!(windows) {
        "forge_webview.exe"
    } else {
        "forge_webview"
    };
    let binary = std::env::current_exe()
        .ok()
        .and_then(|here| here.parent().map(|dir| dir.join(exe)))
        .filter(|candidate| candidate.is_file())
        .unwrap_or_else(|| std::path::PathBuf::from(exe));
    let _ = Command::new(binary).arg(path).spawn();
}

fn open_raw(target: &str) {
    #[cfg(windows)]
    let _ = Command::new("cmd").args(["/C", "start", "", target]).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(target).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open").arg(target).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn recognizes_previewable_extensions() {
        assert_eq!(kind_for(&PathBuf::from("a.md")), Some(PreviewKind::Markdown));
        assert_eq!(kind_for(&PathBuf::from("a.HTML")), Some(PreviewKind::Html));
        assert_eq!(kind_for(&PathBuf::from("a.rs")), None);
    }

    #[test]
    fn parses_headings_and_lists() {
        assert_eq!(heading("## Title"), Some((2, "Title")));
        assert_eq!(heading("###notspace"), None);
        assert_eq!(list_item("- item").map(|(_, t)| t), Some("item"));
        assert_eq!(list_item("3. third").map(|(_, t)| t), Some("third"));
    }

    #[test]
    fn inline_parser_splits_code_bold_and_links() {
        let spans = parse_inline("a `x` **b** [t](u)");
        assert!(matches!(spans.first(), Some(Span::Text(_))));
        assert!(spans.iter().any(|s| matches!(s, Span::Code(c) if c == "x")));
        assert!(spans.iter().any(|s| matches!(s, Span::Bold(b) if b == "b")));
        assert!(spans
            .iter()
            .any(|s| matches!(s, Span::Link(t, u) if t == "t" && u == "u")));
    }

    #[test]
    fn unclosed_markers_stay_plain() {
        let spans = parse_inline("a ** b");
        assert_eq!(spans.len(), 1);
        assert!(matches!(&spans[0], Span::Text(t) if t == "a ** b"));
    }

    #[test]
    fn html_parses_headings_lists_and_links() {
        let blocks = parse_html(
            "<h1>Title</h1><p>Hi <b>there</b> <a href=\"u\">link</a></p>\
             <ul><li>one</li><li>two</li></ul><script>ignore()</script>",
        );
        assert!(matches!(&blocks[0], Block::Heading(1, t) if t == "Title"));
        assert!(blocks.iter().any(|b| matches!(b, Block::Para(_))));
        let items = blocks.iter().filter(|b| matches!(b, Block::Item { .. })).count();
        assert_eq!(items, 2);
        // The script contents must not appear as text.
        assert!(!blocks.iter().any(|b| matches!(b, Block::Para(runs) if runs_to_text(runs).contains("ignore"))));
        // The link survived.
        assert!(blocks.iter().any(|b| matches!(b, Block::Para(runs)
            if runs.iter().any(|r| matches!(r, Run::Link(t, u) if t == "link" && u == "u")))));
    }

    #[test]
    fn html_decodes_entities_and_attrs() {
        assert_eq!(decode_entities("a &amp; b &lt;c&gt; &#65;"), "a & b <c> A");
        assert_eq!(attr("class=\"x\" href='y' rel=z", "href").as_deref(), Some("y"));
    }
}
