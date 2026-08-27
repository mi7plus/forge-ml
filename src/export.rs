use crate::{
    data::Dataset,
    experiment::ExperimentRun,
    notebook::{CellKind, NotebookDocument},
};
use arrow::{csv::WriterBuilder, ipc::writer::FileWriter};
use parquet::arrow::ArrowWriter;
use std::{
    fs::{self, File},
    io::Write,
    path::{Component, Path, PathBuf},
};

const MAX_BUNDLE_FILE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 500 * 1024 * 1024;
const MAX_BUNDLE_FILES: usize = 20_000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Csv,
    Tsv,
    JsonLines,
    Parquet,
    Arrow,
}

pub fn dataset(dataset: &Dataset, path: &Path, format: DataFormat) -> Result<(), String> {
    let parent = path.parent().ok_or("Export path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    match format {
        DataFormat::Csv | DataFormat::Tsv => {
            let file = File::create(path).map_err(|e| e.to_string())?;
            let mut writer = WriterBuilder::new()
                .with_header(true)
                .with_delimiter(if format == DataFormat::Csv {
                    b','
                } else {
                    b'\t'
                })
                .build(file);
            writer.write(&dataset.batch).map_err(|e| e.to_string())?;
        }
        DataFormat::JsonLines => {
            let mut file = File::create(path).map_err(|e| e.to_string())?;
            for row in &dataset.table.rows {
                let object = dataset
                    .table
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        (
                            name.clone(),
                            serde_json::Value::String(row.get(index).cloned().unwrap_or_default()),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>();
                writeln!(file, "{}", serde_json::Value::Object(object))
                    .map_err(|e| e.to_string())?;
            }
        }
        DataFormat::Parquet => {
            let mut writer = ArrowWriter::try_new(
                File::create(path).map_err(|e| e.to_string())?,
                dataset.batch.schema(),
                None,
            )
            .map_err(|e| e.to_string())?;
            writer.write(&dataset.batch).map_err(|e| e.to_string())?;
            writer.close().map_err(|e| e.to_string())?;
        }
        DataFormat::Arrow => {
            let mut writer = FileWriter::try_new(
                File::create(path).map_err(|e| e.to_string())?,
                &dataset.batch.schema(),
            )
            .map_err(|e| e.to_string())?;
            writer.write(&dataset.batch).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn notebook_markdown(document: &NotebookDocument) -> String {
    document
        .cells
        .iter()
        .map(|cell| match cell.kind {
            CellKind::Markdown => cell.source.clone(),
            CellKind::Code => format!("```rust\n{}\n```", cell.source),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn notebook_html(document: &NotebookDocument) -> String {
    let body = document
        .cells
        .iter()
        .map(|cell| match cell.kind {
            CellKind::Markdown => format!(
                "<section class=\"markdown\"><pre>{}</pre></section>",
                escape(&cell.source)
            ),
            CellKind::Code => format!(
                "<pre><code class=\"language-rust\">{}</code></pre>",
                escape(&cell.source)
            ),
        })
        .collect::<String>();
    format!("<!doctype html><meta charset=\"utf-8\"><title>Forge ML notebook</title><style>body{{font:16px system-ui;max-width:960px;margin:40px auto;padding:0 20px}}pre{{white-space:pre-wrap;background:#17191d;color:#eee;padding:16px;border-radius:8px}}.markdown pre{{background:none;color:inherit;padding:0}}</style><main>{body}</main>")
}

pub fn experiment_bundle(
    run: &ExperimentRun,
    source_root: &Path,
    destination: &Path,
) -> Result<(), String> {
    let file = File::create(destination).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("run.json", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(&serde_json::to_vec_pretty(run).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    for artifact in &run.artifacts {
        let relative = Path::new(artifact);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue;
        }
        let path = source_root.join(relative);
        if path.is_file() {
            zip.start_file(
                format!(
                    "artifacts/{}",
                    relative.to_string_lossy().replace('\\', "/")
                ),
                options,
            )
            .map_err(|e| e.to_string())?;
            zip.write_all(&fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(serde::Serialize)]
struct BundleEntry {
    path: String,
    bytes: u64,
    digest: String,
}

pub fn project_bundle(root: &Path, destination: &Path) -> Result<(), String> {
    let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    collect_project_files(&canonical_root, &canonical_root, &mut files)?;
    files.sort();
    if files.len() > MAX_BUNDLE_FILES {
        return Err(format!("Project bundle exceeds {MAX_BUNDLE_FILES} files"));
    }
    let mut total = 0u64;
    let mut entries = Vec::new();
    for relative in &files {
        let path = canonical_root.join(relative);
        let metadata = path.metadata().map_err(|e| e.to_string())?;
        if metadata.len() > MAX_BUNDLE_FILE_BYTES {
            return Err(format!(
                "{} exceeds the 100 MB per-file bundle limit",
                relative.display()
            ));
        }
        total = total
            .checked_add(metadata.len())
            .ok_or("Bundle size overflow")?;
        if total > MAX_BUNDLE_BYTES {
            return Err("Project bundle exceeds the 500 MB limit".into());
        }
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        entries.push(BundleEntry {
            path: relative.to_string_lossy().replace('\\', "/"),
            bytes: metadata.len(),
            digest: crate::experiment::stable_digest(&bytes),
        });
    }
    let file = File::create(destination).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (relative, entry) in files.iter().zip(&entries) {
        zip.start_file(format!("project/{}", entry.path), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&fs::read(canonical_root.join(relative)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    let manifest = serde_json::json!({"schema":2,"forge_version":env!("CARGO_PKG_VERSION"),"digest_algorithm":"sha256","file_count":entries.len(),"total_uncompressed_bytes":total,"entries":entries,"excluded":[".git",".forge","target",".venv","node_modules","credential-like files","symlinks"]});
    zip.start_file("forge-bundle.json", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn collect_project_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() || excluded(relative) {
            continue;
        }
        if kind.is_dir() {
            collect_project_files(root, &path, files)?;
        } else if kind.is_file() {
            files.push(relative.to_owned());
        }
    }
    Ok(())
}
fn excluded(path: &Path) -> bool {
    let parts = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(v) => v.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if parts.iter().any(|v| {
        matches!(
            *v,
            ".git" | ".forge" | "target" | ".venv" | "node_modules" | "__pycache__"
        )
    }) {
        return true;
    }
    let name = parts
        .last()
        .copied()
        .unwrap_or_default()
        .to_ascii_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || name.contains("credentials")
        || name.contains("secret")
        || matches!(
            Path::new(&name).extension().and_then(|v| v.to_str()),
            Some("pem" | "key" | "p12" | "pfx")
        )
}

pub fn dataset_report(name: &str, dataset: &Dataset) -> String {
    let profile_rows = dataset
        .profile()
        .into_iter()
        .map(|p| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(&p.name),
                p.missing,
                p.unique,
                number(p.min),
                number(p.max),
                number(p.mean)
            )
        })
        .collect::<String>();
    let headers = dataset
        .columns
        .iter()
        .map(|v| format!("<th>{}</th>", escape(v)))
        .collect::<String>();
    let rows = dataset
        .rows
        .iter()
        .take(100)
        .map(|row| {
            format!(
                "<tr>{}</tr>",
                row.iter()
                    .map(|v| format!("<td>{}</td>", escape(v)))
                    .collect::<String>()
            )
        })
        .collect::<String>();
    html(&format!("Dataset report — {}",escape(name)), &format!("<h1>{}</h1><p>{} rows × {} columns. Preview limited to 100 rows.</p><h2>Column profile</h2><table><tr><th>Column</th><th>Missing</th><th>Unique</th><th>Min</th><th>Max</th><th>Mean</th></tr>{profile_rows}</table><h2>Preview</h2><div class=\"scroll\"><table><tr>{headers}</tr>{rows}</table></div>",escape(name),dataset.rows.len(),dataset.columns.len()))
}

pub fn experiment_report(runs: &[ExperimentRun], metric: &str) -> String {
    let rows = runs
        .iter()
        .map(|run| {
            let values = run.metrics.get(metric);
            let final_value = values
                .and_then(|v| v.last())
                .map(|v| format!("{:.6}", v[1]))
                .unwrap_or_else(|| "—".into());
            format!(
                "<tr><td>{}</td><td>{final_value}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(&run.name),
                values.map_or(0, Vec::len),
                escape(&run.tags.join(", ")),
                escape(&run.provenance.git_commit)
            )
        })
        .collect::<String>();
    html(&format!("Experiment comparison — {}",escape(metric)), &format!("<h1>Experiment comparison</h1><p>Metric: <strong>{}</strong></p><table><tr><th>Run</th><th>Final</th><th>Steps</th><th>Tags</th><th>Git commit</th></tr>{rows}</table><h2>Run manifests</h2><pre>{}</pre>",escape(metric),escape(&serde_json::to_string_pretty(runs).unwrap_or_default())))
}
fn number(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.6}"))
        .unwrap_or_else(|| "—".into())
}
fn html(title: &str, body: &str) -> String {
    format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>{title}</title><style>body{{font:15px system-ui;max-width:1100px;margin:40px auto;padding:0 20px;color:#202124}}table{{border-collapse:collapse;width:100%}}th,td{{border:1px solid #ccd1d5;padding:7px;text-align:left}}th{{background:#eef2f5}}.scroll{{overflow:auto}}pre{{white-space:pre-wrap;background:#17191d;color:#eee;padding:16px;border-radius:8px}}</style></head><body>{body}<footer><p>Generated by Forge ML {}</p></footer></body></html>",env!("CARGO_PKG_VERSION"))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_protocol::TableData;
    #[test]
    fn exports_dataset_formats() {
        let root = std::env::temp_dir().join(format!("forge-export-{}", std::process::id()));
        let data = Dataset::from_table(
            TableData {
                columns: vec!["x".into()],
                rows: vec![vec!["1".into()]],
            },
            None,
        )
        .unwrap();
        for (name, format) in [
            ("x.csv", DataFormat::Csv),
            ("x.tsv", DataFormat::Tsv),
            ("x.jsonl", DataFormat::JsonLines),
            ("x.parquet", DataFormat::Parquet),
            ("x.arrow", DataFormat::Arrow),
        ] {
            dataset(&data, &root.join(name), format).unwrap();
        }
        assert!(root.join("x.parquet").metadata().unwrap().len() > 0);
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn project_bundle_excludes_secrets_and_build_state() {
        let root =
            std::env::temp_dir().join(format!("forge-project-bundle-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join(".env"), "TOKEN=secret").unwrap();
        fs::write(root.join("target/build.bin"), "build").unwrap();
        let destination = root.join("bundle.zip");
        project_bundle(&root, &destination).unwrap();
        let archive = zip::ZipArchive::new(File::open(destination).unwrap()).unwrap();
        let names = archive.file_names().map(str::to_owned).collect::<Vec<_>>();
        assert!(names.contains(&"project/src/main.rs".into()));
        assert!(!names
            .iter()
            .any(|name| name.contains(".env") || name.contains("target")));
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn html_reports_escape_dataset_content() {
        let data = Dataset::from_table(
            TableData {
                columns: vec!["<x>".into()],
                rows: vec![vec!["<script>".into()]],
            },
            None,
        )
        .unwrap();
        let report = dataset_report("unsafe", &data);
        assert!(!report.contains("<script>"));
        assert!(report.contains("&lt;script&gt;"));
    }
}
