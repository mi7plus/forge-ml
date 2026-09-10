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
        let open_label = format!("Open in browser  {}", icons::ARROW_SQUARE_OUT.as_str());
        if ui
            .add_enabled(on_disk.is_some(), egui::Button::new(open_label))
            .clicked()
        {
            if let Some(path) = on_disk {
                open_path(path);
            }
        }
        ui.label(
            RichText::new(if on_disk.is_some() {
                "HTML renders in your browser."
            } else {
                "Save the file first to preview it in your browser."
            })
            .color(MUTED),
        );
    });
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);
    ui.label(RichText::new("SOURCE").size(11.0).strong().color(MUTED));
    ui.add_space(4.0);
    code_block(ui, source);
}

// ── Opening things ────────────────────────────────────────────────────────────

fn open(target: &str) {
    open_raw(target);
}

fn open_path(path: &Path) {
    open_raw(&path.to_string_lossy());
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
}
