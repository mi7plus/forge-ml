//! Tiny CSV helpers shared by the bundled ML examples. Minimal, but enough for
//! the well-formed datasets in `examples/data/`.

use std::path::Path;

/// Read a comma-separated file (optional double-quoted fields, one header row)
/// into its header names and string rows.
pub fn read_csv(path: impl AsRef<Path>) -> (Vec<String>, Vec<Vec<String>>) {
    let text = std::fs::read_to_string(path).expect("read dataset CSV");
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let headers = split_row(lines.next().unwrap_or_default());
    let rows = lines.map(split_row).collect();
    (headers, rows)
}

fn split_row(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut field)),
            _ => field.push(ch),
        }
    }
    fields.push(field);
    fields.iter().map(|f| f.trim().to_owned()).collect()
}

/// Index of a column by header name (panics if absent — datasets are fixed).
pub fn column(headers: &[String], name: &str) -> usize {
    headers
        .iter()
        .position(|header| header == name)
        .unwrap_or_else(|| panic!("column `{name}` not found"))
}

/// Absolute path to a dataset shipped under `examples/data/`.
pub fn dataset_path(name: &str) -> String {
    format!("{}/examples/data/{name}", env!("CARGO_MANIFEST_DIR"))
}
