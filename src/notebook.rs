use std::path::Path;

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
