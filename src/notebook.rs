use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CellKind {
    Code,
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotebookCell {
    pub kind: CellKind,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RichOutput {
    pub mime: String,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotebookDocument {
    pub cells: Vec<NotebookCell>,
    pub kernel: String,
}

impl NotebookDocument {
    pub fn parse_rust(source: &str) -> Self {
        let cells = cell_byte_ranges(source)
            .into_iter()
            .map(|range| {
                let raw = &source[range];
                let markdown = raw
                    .lines()
                    .next()
                    .is_some_and(|line| line.contains("[markdown]"));
                let source = if markdown {
                    raw.lines()
                        .skip(1)
                        .map(|line| {
                            line.trim_start()
                                .strip_prefix("//#")
                                .unwrap_or(line)
                                .trim_start()
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    raw.to_owned()
                };
                NotebookCell {
                    kind: if markdown {
                        CellKind::Markdown
                    } else {
                        CellKind::Code
                    },
                    source,
                }
            })
            .collect();
        Self {
            cells,
            kernel: "rust".into(),
        }
    }

    pub fn to_rust(&self) -> String {
        self.cells
            .iter()
            .enumerate()
            .map(|(index, cell)| match cell.kind {
                CellKind::Code => {
                    if index == 0 && cell.source.trim_start().starts_with("//# %%") {
                        cell.source.clone()
                    } else {
                        format!("//# %%\n{}", cell.source)
                    }
                }
                CellKind::Markdown => format!(
                    "//# %% [markdown]\n{}",
                    cell.source
                        .lines()
                        .map(|line| format!("//# {line}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn from_ipynb(text: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let kernel = value["metadata"]["kernelspec"]["name"]
            .as_str()
            .unwrap_or("rust")
            .to_owned();
        let cells = value["cells"]
            .as_array()
            .ok_or("Notebook has no cells")?
            .iter()
            .map(|cell| {
                let kind = if cell["cell_type"] == "markdown" {
                    CellKind::Markdown
                } else {
                    CellKind::Code
                };
                let source = match &cell["source"] {
                    serde_json::Value::Array(lines) => {
                        lines.iter().filter_map(|v| v.as_str()).collect::<String>()
                    }
                    serde_json::Value::String(text) => text.clone(),
                    _ => String::new(),
                };
                NotebookCell { kind, source }
            })
            .collect();
        Ok(Self { cells, kernel })
    }

    pub fn to_ipynb(&self) -> Result<String, String> {
        let cells = self
            .cells
            .iter()
            .map(|cell| {
                serde_json::json!({
                    "cell_type": if cell.kind == CellKind::Markdown { "markdown" } else { "code" },
            "metadata": {}, "source": cell.source.split_inclusive('\n').collect::<Vec<_>>(),
                    "execution_count": serde_json::Value::Null, "outputs": []
                })
            })
            .collect::<Vec<_>>();
        serde_json::to_string_pretty(&serde_json::json!({"cells": cells, "metadata": {"kernelspec": {"display_name": self.kernel, "language": "rust", "name": self.kernel}}, "nbformat": 4, "nbformat_minor": 5})).map_err(|e| e.to_string())
    }
}

pub fn is_notebook_document(text: &str) -> bool {
    text.contains("//# %%")
}

pub fn notebook_lsp_prefix_chars() -> usize {
    "fn __forge_notebook__() {\n".chars().count()
}

pub fn lsp_document(text: &str) -> (String, usize) {
    if is_notebook_document(text) {
        let prefix = "fn __forge_notebook__() {\n";
        (format!("{prefix}{text}\n}}\n"), prefix.chars().count())
    } else {
        (text.to_owned(), 0)
    }
}

pub fn prepare_runtime_code(code: &str, source_path: Option<&Path>) -> String {
    let source_directory = source_path.and_then(Path::parent);
    let mut output = Vec::new();
    let mut explicit_path_attribute = false;
    for line in code.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[path") {
            let rewritten = source_directory
                .and_then(|directory| rewrite_path_attribute(line, directory))
                .unwrap_or_else(|| line.to_owned());
            output.push(rewritten);
            explicit_path_attribute = true;
            continue;
        }
        if !explicit_path_attribute {
            if let Some(module_name) = trimmed
                .strip_prefix("mod ")
                .and_then(|value| value.strip_suffix(';'))
                .map(str::trim)
                .filter(|name| {
                    name.chars()
                        .all(|character| character == '_' || character.is_alphanumeric())
                })
            {
                if let Some(directory) = source_directory {
                    let flat = directory.join(format!("{module_name}.rs"));
                    let nested = directory.join(module_name).join("mod.rs");
                    let module_path = [flat, nested].into_iter().find(|path| path.is_file());
                    if let Some(module_path) = module_path {
                        output.push(format!("#[path = \"{}\"]", rust_path(&module_path)));
                    }
                }
            }
        }
        output.push(line.to_owned());
        explicit_path_attribute = false;
    }
    let mut prepared = output.join("\n");
    if code.contains("// forge: expose-main") {
        if let Some(exposed) = expose_main_body(&prepared) {
            prepared = exposed;
        }
    } else if !is_notebook_document(code)
        && code.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("fn main(") || line.starts_with("pub fn main(")
        })
    {
        prepared.push_str("\nmain();");
    }
    prepared
}

pub fn cell_byte_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut starts = vec![0];
    for (offset, _) in text.match_indices("//# %%") {
        if offset > 0 {
            starts.push(offset);
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| *start..starts.get(index + 1).copied().unwrap_or(text.len()))
        .collect()
}

/// Swap cell `first` with the cell after it (`first + 1`) by exchanging their
/// raw byte ranges, preserving every other byte. Returns `None` if either index
/// is out of range. Used by the Notebook pane's move up/down.
pub fn swap_adjacent_cells(content: &str, first: usize) -> Option<String> {
    let ranges = cell_byte_ranges(content);
    let a = ranges.get(first)?.clone();
    let b = ranges.get(first + 1)?.clone();
    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..a.start]);
    out.push_str(&content[b.clone()]);
    out.push_str(&content[a]);
    out.push_str(&content[b.end..]);
    Some(out)
}

fn expose_main_body(code: &str) -> Option<String> {
    let main_start = code
        .lines()
        .scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len() + 1;
            Some((start, line))
        })
        .find_map(|(start, line)| {
            let trimmed = line.trim_start();
            (trimmed.starts_with("fn main(") || trimmed.starts_with("pub fn main("))
                .then_some(start + line.len() - trimmed.len())
        })?;
    let body_start = code[main_start..].find('{')? + main_start;
    let mut depth = 0_usize;
    let mut body_end = None;
    for (offset, character) in code[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    body_end = Some(body_start + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let body_end = body_end?;
    let mut exposed = String::new();
    exposed.push_str(code[..main_start].trim_end());
    exposed.push('\n');
    exposed.push_str(code[body_end + 1..].trim());
    exposed.push('\n');
    exposed.push_str(code[body_start + 1..body_end].trim());
    Some(exposed)
}

fn rewrite_path_attribute(line: &str, source_directory: &Path) -> Option<String> {
    let first_quote = line.find('"')?;
    let second_quote = line[first_quote + 1..].find('"')? + first_quote + 1;
    let declared = Path::new(&line[first_quote + 1..second_quote]);
    if declared.is_absolute() {
        return Some(line.to_owned());
    }
    let resolved = source_directory.join(declared);
    let mut rewritten = String::new();
    rewritten.push_str(&line[..first_quote + 1]);
    rewritten.push_str(&rust_path(&resolved));
    rewritten.push_str(&line[second_quote..]);
    Some(rewritten)
}

fn rust_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_owned())
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn wraps_notebooks_for_rust_analyzer_and_preserves_offsets() {
        let source = "//# %% setup\nlet value = 42;";
        let (wrapped, prefix_chars) = lsp_document(source);
        assert!(wrapped.starts_with("fn __forge_notebook__() {\n"));
        assert!(wrapped.contains(source));
        assert_eq!(prefix_chars, "fn __forge_notebook__() {\n".chars().count());
        let raw_offset = source.find("value").unwrap();
        assert_eq!(raw_offset + prefix_chars - prefix_chars, raw_offset);
    }

    #[test]
    fn leaves_regular_documents_unchanged() {
        assert_eq!(lsp_document("fn main() {}"), ("fn main() {}".into(), 0));
    }

    #[test]
    fn editing_a_cell_range_replaces_only_that_cell() {
        // The Notebook pane's in-place edit replaces cell_byte_ranges[index]
        // (marker + body) with the draft, leaving the other cells untouched.
        let content = "//# %% a\nx = 1;\n//# %% b\ny = 2;\n";
        let ranges = cell_byte_ranges(content);
        assert_eq!(ranges.len(), 2);
        // Edit the first cell.
        let mut edited = content.to_owned();
        edited.replace_range(ranges[0].clone(), "//# %% a\nx = 99;\n");
        assert_eq!(edited, "//# %% a\nx = 99;\n//# %% b\ny = 2;\n");
        // The second cell's range still isolates just that cell.
        let ranges2 = cell_byte_ranges(&edited);
        assert_eq!(&edited[ranges2[1].clone()], "//# %% b\ny = 2;\n");
    }

    #[test]
    fn swapping_adjacent_cells_reorders_only_that_pair() {
        let content = "//# %% a\nx = 1;\n//# %% b\ny = 2;\n//# %% c\nz = 3;\n";
        let swapped = swap_adjacent_cells(content, 0).unwrap();
        assert_eq!(
            swapped,
            "//# %% b\ny = 2;\n//# %% a\nx = 1;\n//# %% c\nz = 3;\n"
        );
        // The third cell is untouched.
        let ranges = cell_byte_ranges(&swapped);
        assert_eq!(&swapped[ranges[2].clone()], "//# %% c\nz = 3;\n");
        // Out-of-range is a no-op (None).
        assert!(swap_adjacent_cells(content, 2).is_none());
    }

    #[test]
    fn markdown_and_ipynb_round_trip() {
        let source = "//# %% [markdown]\n//# # Results\n//# hello\n//# %%\nlet score = 0.9;";
        let notebook = NotebookDocument::parse_rust(source);
        assert_eq!(notebook.cells[0].kind, CellKind::Markdown);
        assert!(notebook.cells[0].source.contains("# Results"));
        let ipynb = notebook.to_ipynb().unwrap();
        let restored = NotebookDocument::from_ipynb(&ipynb).unwrap();
        assert_eq!(restored.cells, notebook.cells);
        assert!(restored.to_rust().contains("[markdown]"));
    }

    #[test]
    fn resolves_relative_modules_for_runtime() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let source_path = root.join("examples/navigation_demo.rs");
        let source = std::fs::read_to_string(&source_path).unwrap();
        let prepared = prepare_runtime_code(&source, Some(&source_path));
        let model_path = rust_path(&root.join("examples/support/model.rs"));
        assert!(prepared.starts_with(&format!("#[path = \"{model_path}\"]")));
        assert!(prepared.contains("mod model;"));
        assert!(!prepared.contains("fn main()"));
        assert!(prepared.contains("let model: LinearModel = LinearModel::new"));
    }
}
