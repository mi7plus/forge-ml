//! Editor and text helpers: word/offset math, LSP position mapping and edit
//! application, caret and inline-diagnostic painting, rustfmt, and the file-tree
//! explorer rendering.

use crate::lsp::{self, Diagnostic as LspDiagnostic};
use crate::project::{self, FileNode};
use crate::ui::theme::{accent, EMBER, MUTED, RED, TEXT};
use crate::{EditorTab, ExplorerAction};
use eframe::egui;
use egui::{Color32, RichText, Stroke};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

pub fn safe_file_stem(value: &str) -> String {
    let stem = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    if stem.is_empty() {
        "plot".into()
    } else {
        stem
    }
}

pub fn word_start_at(text: &str, offset: usize) -> Option<usize> {
    let chars = text.chars().collect::<Vec<_>>();
    let is_word = |character: char| character.is_alphanumeric() || character == '_';
    let mut index = offset.min(chars.len());
    if index == chars.len()
        || !chars
            .get(index)
            .is_some_and(|character| is_word(*character))
    {
        if index == 0 || !is_word(chars[index - 1]) {
            return None;
        }
        index -= 1;
    }
    while index > 0 && is_word(chars[index - 1]) {
        index -= 1;
    }
    Some(index)
}

/// Toggle a `// ` line comment on the line containing char offset `cursor`.
/// Comment inserts after the existing indentation; uncomment strips a leading
/// `// ` (or bare `//`). Returns the new content and the adjusted caret offset.
pub fn toggle_line_comment(content: &str, cursor: usize) -> (String, usize) {
    let chars: Vec<char> = content.chars().collect();
    let cursor = cursor.min(chars.len());
    let mut line_start = cursor;
    while line_start > 0 && chars[line_start - 1] != '\n' {
        line_start -= 1;
    }
    let mut line_end = cursor;
    while line_end < chars.len() && chars[line_end] != '\n' {
        line_end += 1;
    }
    let line: String = chars[line_start..line_end].iter().collect();
    let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    let indent_len = indent.chars().count();
    let body: String = line.chars().skip(indent_len).collect();
    let (new_body, delta): (String, isize) = if let Some(rest) = body.strip_prefix("// ") {
        (rest.to_owned(), -3)
    } else if let Some(rest) = body.strip_prefix("//") {
        (rest.to_owned(), -2)
    } else {
        (format!("// {body}"), 3)
    };
    let mut new_content: String = chars[..line_start].iter().collect();
    new_content.push_str(&indent);
    new_content.push_str(&new_body);
    new_content.extend(chars[line_end..].iter());
    // The edit happens at the indent boundary, so shift the caret by `delta`
    // only if it sat at or after that boundary.
    let boundary = line_start + indent_len;
    let new_cursor = if cursor >= boundary {
        (cursor as isize + delta).max(boundary as isize) as usize
    } else {
        cursor
    };
    (new_content, new_cursor)
}

pub fn char_to_byte(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

pub fn line_column(text: &str, char_offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for character in text.chars().take(char_offset) {
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

pub fn csv_field(value: &str) -> String {
    if value
        .chars()
        .any(|character| matches!(character, ',' | '"' | '\n' | '\r'))
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

pub fn paint_editor_caret(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    cursor: egui::text::CCursor,
    dark: bool,
    blink: bool,
) {
    if blink {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
    let bright_phase = !blink || ui.input(|input| input.time) % 1.0 < 0.65;
    let local = output.galley.pos_from_cursor(cursor);
    let x = output.galley_pos.x + local.min.x;
    let top = output.galley_pos.y + local.min.y - 1.0;
    let bottom = output.galley_pos.y + local.max.y + 1.0;
    let segment = [egui::pos2(x, top), egui::pos2(x, bottom)];
    let outline = if dark {
        Color32::from_rgb(3, 7, 12)
    } else {
        Color32::WHITE
    };
    let caret = if dark && bright_phase {
        Color32::from_rgb(118, 224, 255)
    } else if dark {
        Color32::from_rgb(59, 129, 153)
    } else if bright_phase {
        Color32::from_rgb(0, 45, 84)
    } else {
        Color32::from_rgb(60, 105, 135)
    };
    let outline_width = if bright_phase { 5.0 } else { 3.0 };
    let caret_width = if bright_phase { 3.0 } else { 1.5 };
    ui.painter()
        .line_segment(segment, Stroke::new(outline_width, outline));
    ui.painter()
        .line_segment(segment, Stroke::new(caret_width, caret));
}

pub fn paint_navigable_word(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    text: &str,
    offset: usize,
) {
    let chars = text.chars().collect::<Vec<_>>();
    let is_word = |character: char| character.is_alphanumeric() || character == '_';
    let mut start = offset.min(chars.len());
    let mut end = start;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    if start == end {
        return;
    }
    let start_rect = output
        .galley
        .pos_from_cursor(egui::text::CCursor::new(start));
    let end_rect = output.galley.pos_from_cursor(egui::text::CCursor::new(end));
    if start_rect.min.y == end_rect.min.y {
        let y = start_rect.max.y - 1.0;
        ui.painter().line_segment(
            [
                output.galley_pos + egui::vec2(start_rect.min.x, y),
                output.galley_pos + egui::vec2(end_rect.min.x, y),
            ],
            Stroke::new(1.5, accent()),
        );
    }
}

pub fn paint_inline_diagnostics(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    text: &str,
    diagnostics: &[LspDiagnostic],
) {
    let chars = text.chars().collect::<Vec<_>>();
    let painter = ui.painter().with_clip_rect(output.text_clip_rect);
    for diagnostic in diagnostics {
        let line_start = text
            .split_inclusive('\n')
            .take(diagnostic.line as usize)
            .map(str::chars)
            .map(Iterator::count)
            .sum::<usize>();
        let mut start = (line_start + diagnostic.column as usize).min(chars.len());
        while start < chars.len() && chars[start].is_whitespace() && chars[start] != '\n' {
            start += 1;
        }
        let mut end = start;
        while end < chars.len()
            && (chars[end].is_alphanumeric() || chars[end] == '_' || chars[end] == ':')
        {
            end += 1;
        }
        if end == start {
            end = (start + 1).min(chars.len());
        }
        let start_rect = output
            .galley
            .pos_from_cursor(egui::text::CCursor::new(start));
        let end_rect = output.galley.pos_from_cursor(egui::text::CCursor::new(end));
        if start_rect.min.y != end_rect.min.y {
            continue;
        }
        let left = output.galley_pos.x + start_rect.min.x;
        let right = (output.galley_pos.x + end_rect.min.x).max(left + 5.0);
        let baseline = output.galley_pos.y + start_rect.max.y - 1.0;
        let color = match diagnostic.severity {
            1 => RED,
            2 => EMBER,
            _ => accent(),
        };
        let mut points = Vec::new();
        let mut x = left;
        let mut high = true;
        while x <= right {
            points.push(egui::pos2(x, baseline + if high { -1.5 } else { 1.0 }));
            high = !high;
            x += 3.0;
        }
        points.push(egui::pos2(right, baseline));
        painter.add(egui::Shape::line(points, Stroke::new(1.4, color)));
        let hover_rect = egui::Rect::from_min_max(
            egui::pos2(left, output.galley_pos.y + start_rect.min.y),
            egui::pos2(right, output.galley_pos.y + start_rect.max.y + 3.0),
        );
        if ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|pointer| hover_rect.contains(pointer))
        {
            output
                .response
                .response
                .clone()
                .on_hover_text_at_pointer(&diagnostic.message);
        }
    }
}

pub fn welcome_tab() -> EditorTab {
    EditorTab {
        path: None,
        title: "experiment.rs".to_owned(),
        dirty: false,
        disk_hash: None,
        external_change_pending: false,
        content: r#"//# %% setup
let learning_rate = 0.03_f32;
let epochs = 12;

//# %% dataset
let samples = vec![0.2_f32, 0.7, 1.1, 1.8, 2.4];
println!("forge_vector:samples=0.2,0.7,1.1,1.8,2.4");

//# %% training
for epoch in 0..epochs {
    let loss = (-0.35 * epoch as f64).exp();
    println!("forge_metric:loss={}", loss);
}
"training complete""#
            .to_owned(),
    }
}

pub fn blank_tab() -> EditorTab {
    EditorTab {
        path: None,
        title: "Untitled.rs".to_owned(),
        content: String::new(),
        dirty: false,
        disk_hash: None,
        external_change_pending: false,
    }
}

pub fn content_hash(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

pub fn file_title(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("untitled")
        .to_owned()
}

/// Convert an LSP (line, utf16-character) position to a char offset in `text`.
pub fn lsp_pos_to_offset(text: &str, line: usize, character: usize) -> usize {
    let mut char_offset = 0usize;
    let mut current_line = 0usize;
    let mut chars = text.chars().peekable();
    while current_line < line {
        match chars.next() {
            Some('\n') => {
                current_line += 1;
                char_offset += 1;
            }
            Some(_) => char_offset += 1,
            None => return char_offset,
        }
    }
    let mut utf16 = 0usize;
    while utf16 < character {
        match chars.peek() {
            Some('\n') | None => break,
            Some(c) => {
                utf16 += c.len_utf16();
                char_offset += 1;
                chars.next();
            }
        }
    }
    char_offset
}

/// Apply LSP text edits to `content` in-place (last edit first, so offsets hold).
pub fn apply_edits_to(content: &mut String, edits: &[lsp::TextEdit]) {
    let mut resolved: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|edit| {
            let start = lsp_pos_to_offset(content, edit.start_line, edit.start_col);
            let end = lsp_pos_to_offset(content, edit.end_line, edit.end_col);
            (start, end, edit.new_text.as_str())
        })
        .collect();
    resolved.sort_by_key(|item| std::cmp::Reverse(item.0));
    for (start, end, new_text) in resolved {
        let start_b = content
            .char_indices()
            .nth(start)
            .map(|(b, _)| b)
            .unwrap_or(content.len());
        let end_b = content
            .char_indices()
            .nth(end)
            .map(|(b, _)| b)
            .unwrap_or(content.len());
        if start_b <= end_b && end_b <= content.len() {
            content.replace_range(start_b..end_b, new_text);
        }
    }
}

/// Format Rust source with `rustfmt`, piping through stdin/stdout so the editor
/// buffer (which may be unsaved) is formatted without touching disk.
pub fn run_rustfmt(source: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("rustfmt")
        .args(["--edition", "2021", "--emit", "stdout", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not launch rustfmt: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("rustfmt stdin unavailable")?
        .write_all(source.as_bytes())
        .map_err(|e| e.to_string())?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|e| e.to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("rustfmt error");
        Err(first.to_owned())
    }
}

pub fn infer_size(type_name: &str) -> &str {
    if type_name.contains("Vec<") {
        "dynamic"
    } else if type_name.starts_with('[') {
        "array"
    } else {
        "scalar"
    }
}

pub fn draw_file_nodes(
    ui: &mut egui::Ui,
    nodes: &[FileNode],
    selected: Option<&Path>,
) -> Option<ExplorerAction> {
    let mut action = None;
    for node in nodes {
        if let Some(children) = &node.children {
            let shown = egui::CollapsingHeader::new(RichText::new(&node.name).color(TEXT))
                .show(ui, |ui| draw_file_nodes(ui, children, selected));
            shown.header_response.context_menu(|ui| {
                if ui.button("New file here...").clicked() {
                    action = Some(ExplorerAction::NewFile(node.path.clone()));
                    ui.close();
                }
            });
            if let Some(child_action) = shown.body_returned.flatten() {
                action = Some(child_action);
            }
        } else {
            let editable = project::is_editable(&node.path);
            let active = selected == Some(node.path.as_path());
            let marker = node
                .git_status
                .as_deref()
                .map(|status| format!(" [{status}]"))
                .unwrap_or_default();
            let response = ui.selectable_label(
                active,
                RichText::new(format!("  {}{marker}", node.name))
                    .monospace()
                    .size(11.0)
                    .color(if active {
                        accent()
                    } else if editable {
                        TEXT
                    } else {
                        MUTED
                    }),
            );
            if response.clicked() && editable {
                action = Some(ExplorerAction::Open(node.path.clone()));
            }
            response.context_menu(|ui| {
                if ui
                    .button(RichText::new("Delete file...").color(RED))
                    .clicked()
                {
                    action = Some(ExplorerAction::Delete(node.path.clone()));
                    ui.close();
                }
            });
        }
    }
    action
}

pub fn collect_editable_files(nodes: &[FileNode], paths: &mut Vec<PathBuf>) {
    for node in nodes {
        if let Some(children) = &node.children {
            collect_editable_files(children, paths);
        } else if project::is_editable(&node.path) {
            paths.push(node.path.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_file_stem_sanitizes_and_defaults() {
        assert_eq!(safe_file_stem("a b/c.d"), "a_b_c_d");
        assert_eq!(safe_file_stem("ok-name_1"), "ok-name_1");
        assert_eq!(safe_file_stem(""), "plot");
        assert_eq!(safe_file_stem("***"), "___");
    }

    #[test]
    fn word_start_at_finds_the_token_start() {
        assert_eq!(word_start_at("let foo", 6), Some(4)); // inside "foo"
        assert_eq!(word_start_at("let foo", 7), Some(4)); // caret at end of "foo"
        assert_eq!(word_start_at("foo", 0), Some(0));
        assert_eq!(word_start_at("a b", 1), Some(0)); // caret just after "a" selects it
        assert_eq!(word_start_at("a  b", 2), None); // caret in whitespace, no adjacent word
        assert_eq!(word_start_at("", 0), None);
    }

    #[test]
    fn char_to_byte_handles_multibyte() {
        // 'é' is two UTF-8 bytes, so char 2 starts at byte 3.
        assert_eq!(char_to_byte("héllo", 2), 3);
        assert_eq!(char_to_byte("abc", 0), 0);
        assert_eq!(char_to_byte("abc", 99), 3); // past the end clamps to len
    }

    #[test]
    fn line_column_is_one_based() {
        assert_eq!(line_column("ab\ncd", 0), (1, 1));
        assert_eq!(line_column("ab\ncd", 4), (2, 2)); // 'd'
        assert_eq!(line_column("ab\ncd", 3), (2, 1)); // start of line 2
    }

    #[test]
    fn csv_field_quotes_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("he\"llo"), "\"he\"\"llo\"");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn content_hash_is_stable_and_distinct() {
        assert_eq!(content_hash("abc"), content_hash("abc"));
        assert_ne!(content_hash("abc"), content_hash("abd"));
    }

    #[test]
    fn file_title_falls_back_to_untitled() {
        assert_eq!(file_title(Path::new("/x/y/foo.rs")), "foo.rs");
        assert_eq!(file_title(Path::new("/")), "untitled");
    }

    #[test]
    fn lsp_pos_to_offset_maps_lines_and_utf16() {
        assert_eq!(lsp_pos_to_offset("ab\ncd", 0, 0), 0);
        assert_eq!(lsp_pos_to_offset("ab\ncd", 1, 1), 4); // 'd'
                                                          // '😀' is one char but two UTF-16 code units, so column 3 lands past it.
        assert_eq!(lsp_pos_to_offset("a😀b", 0, 3), 2);
    }

    #[test]
    fn apply_edits_to_applies_last_first() {
        let mut content = "hello world".to_owned();
        apply_edits_to(
            &mut content,
            &[lsp::TextEdit {
                start_line: 0,
                start_col: 6,
                end_line: 0,
                end_col: 11,
                new_text: "there".into(),
            }],
        );
        assert_eq!(content, "hello there");

        // Two non-overlapping edits on one line must both land.
        let mut content = "aaa bbb".to_owned();
        apply_edits_to(
            &mut content,
            &[
                lsp::TextEdit {
                    start_line: 0,
                    start_col: 0,
                    end_line: 0,
                    end_col: 3,
                    new_text: "X".into(),
                },
                lsp::TextEdit {
                    start_line: 0,
                    start_col: 4,
                    end_line: 0,
                    end_col: 7,
                    new_text: "Y".into(),
                },
            ],
        );
        assert_eq!(content, "X Y");
    }

    #[test]
    fn toggle_line_comment_round_trips() {
        // Comment inserts after indentation.
        let (out, cur) = toggle_line_comment("    let x = 1;", 4);
        assert_eq!(out, "    // let x = 1;");
        assert_eq!(cur, 7); // caret shifted by the inserted "// "
                            // Uncommenting restores the original.
        let (back, cur2) = toggle_line_comment(&out, cur);
        assert_eq!(back, "    let x = 1;");
        assert_eq!(cur2, 4);
        // Only the caret's line changes in a multi-line buffer.
        let (out, _) = toggle_line_comment("a\nb\nc", 2); // caret on line "b"
        assert_eq!(out, "a\n// b\nc");
        // A bare `//` (no trailing space) also uncomments.
        let (out, _) = toggle_line_comment("//x", 3);
        assert_eq!(out, "x");
    }

    #[test]
    fn infer_size_classifies_type_shape() {
        assert_eq!(infer_size("Vec<f64>"), "dynamic");
        assert_eq!(infer_size("[f64; 3]"), "array");
        assert_eq!(infer_size("f64"), "scalar");
    }
}
