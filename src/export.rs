use crate::{
    data::Dataset,
    experiment::ExperimentRun,
    notebook::{CellKind, NotebookDocument},
};
use arrow::{
    csv::WriterBuilder, ipc::writer::FileWriter, record_batch::RecordBatch,
    util::display::array_value_to_string,
};
use parquet::arrow::ArrowWriter;
use std::{
    fs::{self, File},
    io::Write,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_BUNDLE_FILE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 500 * 1024 * 1024;
const MAX_BUNDLE_FILES: usize = 20_000;
static NEXT_EXPORT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Csv,
    Tsv,
    JsonLines,
    Parquet,
    Arrow,
}

pub fn dataset(dataset: &Dataset, path: &Path, format: DataFormat) -> Result<(), String> {
    dataset_batches(&dataset.batches, path, format)
}

pub fn dataset_batches(
    batches: &[RecordBatch],
    path: &Path,
    format: DataFormat,
) -> Result<(), String> {
    let first = batches
        .first()
        .ok_or("Dataset has no Arrow record batches")?;
    if batches.iter().any(|batch| batch.schema() != first.schema()) {
        return Err("Dataset Arrow batches contain incompatible schemas.".into());
    }
    let parent = path.parent().ok_or("Export path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("Export path requires a valid file name")?;
    let temporary = parent.join(format!(
        ".{file_name}.forge-{}-{}.tmp",
        std::process::id(),
        NEXT_EXPORT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let result = write_dataset(batches, &temporary, format).and_then(|()| {
        fs::OpenOptions::new()
            .write(true)
            .open(&temporary)
            .and_then(|file| file.sync_all())
            .map_err(|error| error.to_string())?;
        publish_export(&temporary, path)
    });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_dataset(batches: &[RecordBatch], path: &Path, format: DataFormat) -> Result<(), String> {
    let schema = batches[0].schema();
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
            for batch in batches {
                writer.write(batch).map_err(|e| e.to_string())?;
            }
        }
        DataFormat::JsonLines => {
            let mut file = File::create(path).map_err(|e| e.to_string())?;
            for batch in batches {
                for row in 0..batch.num_rows() {
                    let object = batch
                        .schema()
                        .fields()
                        .iter()
                        .enumerate()
                        .map(|(index, field)| {
                            array_value_to_string(batch.column(index).as_ref(), row)
                                .map(|value| {
                                    (field.name().clone(), serde_json::Value::String(value))
                                })
                                .map_err(|error| error.to_string())
                        })
                        .collect::<Result<serde_json::Map<_, _>, _>>()?;
                    writeln!(file, "{}", serde_json::Value::Object(object))
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        DataFormat::Parquet => {
            let mut writer = ArrowWriter::try_new(
                File::create(path).map_err(|e| e.to_string())?,
                schema.clone(),
                None,
            )
            .map_err(|e| e.to_string())?;
            for batch in batches {
                writer.write(batch).map_err(|e| e.to_string())?;
            }
            writer.close().map_err(|e| e.to_string())?;
        }
        DataFormat::Arrow => {
            let mut writer =
                FileWriter::try_new(File::create(path).map_err(|e| e.to_string())?, &schema)
                    .map_err(|e| e.to_string())?;
            for batch in batches {
                writer.write(batch).map_err(|e| e.to_string())?;
            }
            writer.finish().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn publish_export(temporary: &Path, destination: &Path) -> Result<(), String> {
    if !destination.exists() {
        return fs::rename(temporary, destination).map_err(|error| error.to_string());
    }
    let backup = destination.with_extension(format!(
        "forge-export-{}-{}.bak",
        std::process::id(),
        NEXT_EXPORT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    if backup.exists() {
        fs::remove_file(&backup).map_err(|error| error.to_string())?;
    }
    fs::rename(destination, &backup).map_err(|error| error.to_string())?;
    match fs::rename(temporary, destination) {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(backup, destination);
            Err(error.to_string())
        }
    }
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
        .iter()
        .map(|p| {
            format!(
                "<tr><td>{}</td><td>{} ({:.1}%)</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(&p.name),
                p.missing,
                p.missing_percent,
                p.numeric_count,
                p.unique,
                number(p.min),
                number(p.max),
                number(p.mean),
                number(p.std_dev)
            )
        })
        .collect::<String>();
    let quality = dataset.quality();
    let alerts = if quality.alerts.is_empty() {
        "<li>No missingness, constant-column, or mixed-type alerts.</li>".to_owned()
    } else {
        quality
            .alerts
            .iter()
            .map(|alert| format!("<li>{}</li>", escape(alert)))
            .collect::<String>()
    };
    let correlations = quality
        .correlations
        .iter()
        .take(20)
        .map(|correlation| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{:.4}</td></tr>",
                escape(&correlation.left),
                escape(&correlation.right),
                correlation.coefficient
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
    html(&format!("Dataset report — {}",escape(name)), &format!("<h1>{}</h1><p>{} rows × {} columns. Preview limited to 100 rows.</p><h2>Quality alerts</h2><ul>{alerts}</ul><h2>Column profile</h2><table><tr><th>Column</th><th>Missing</th><th>Numeric</th><th>Unique</th><th>Min</th><th>Max</th><th>Mean</th><th>Std dev</th></tr>{profile_rows}</table><h2>Numeric correlations</h2><p>Bounded to {} rows and {} numeric columns; strongest 20 pairs shown.</p><table><tr><th>Left</th><th>Right</th><th>Pearson r</th></tr>{correlations}</table><h2>Preview</h2><div class=\"scroll\"><table><tr>{headers}</tr>{rows}</table></div>",escape(name),dataset.rows.len(),dataset.columns.len(),quality.correlation_rows,quality.correlation_columns))
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

pub fn dataset_pdf(name: &str, dataset: &Dataset, path: &Path) -> Result<(), String> {
    let quality = dataset.quality();
    let mut lines = vec![
        format!("Dataset report - {name}"),
        format!(
            "{} rows x {} columns",
            dataset.rows.len(),
            dataset.columns.len()
        ),
        String::new(),
        "Quality alerts".into(),
    ];
    if quality.alerts.is_empty() {
        lines.push("No missingness, constant-column, or mixed-type alerts.".into());
    } else {
        lines.extend(
            quality
                .alerts
                .iter()
                .take(100)
                .map(|alert| format!("- {alert}")),
        );
    }
    lines.extend([String::new(), "Column profile (first 500 columns)".into()]);
    lines.extend(dataset.profile().iter().take(500).map(|profile| {
        format!(
            "{} | missing {} ({:.1}%) | numeric {} | unique {} | min {} | max {} | mean {} | sd {}",
            profile.name,
            profile.missing,
            profile.missing_percent,
            profile.numeric_count,
            profile.unique,
            number(profile.min),
            number(profile.max),
            number(profile.mean),
            number(profile.std_dev)
        )
    }));
    lines.extend([String::new(), "Strongest numeric correlations".into()]);
    lines.extend(quality.correlations.iter().take(20).map(|value| {
        format!(
            "{} / {} | r={:.4}",
            value.left, value.right, value.coefficient
        )
    }));
    lines.extend([String::new(), "Preview (50 rows x 20 columns)".into()]);
    let shown_columns = dataset.columns.len().min(20);
    lines.push(dataset.columns[..shown_columns].join(" | "));
    lines.extend(dataset.rows.iter().take(50).map(|row| {
        row.iter()
            .take(shown_columns)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ")
    }));
    write_text_pdf(path, &lines)
}

pub fn experiment_pdf(runs: &[ExperimentRun], metric: &str, path: &Path) -> Result<(), String> {
    let mut lines = vec![
        "Experiment comparison".into(),
        format!("Metric: {metric}"),
        String::new(),
        "Run | Final | Steps | Executions | Tags | Git commit".into(),
    ];
    lines.extend(runs.iter().take(1_000).map(|run| {
        let values = run.metrics.get(metric);
        let final_value = values
            .and_then(|values| values.last())
            .map(|point| format!("{:.6}", point[1]))
            .unwrap_or_else(|| "-".into());
        format!(
            "{} | {} | {} | {} | {} | {}",
            run.name,
            final_value,
            values.map_or(0, Vec::len),
            run.execution_count,
            run.tags.join(", "),
            run.provenance.git_commit
        )
    }));
    write_text_pdf(path, &lines)
}

pub(crate) fn write_text_pdf(path: &Path, lines: &[String]) -> Result<(), String> {
    let wrapped = lines
        .iter()
        .flat_map(|line| wrap_pdf_line(line, 92))
        .collect::<Vec<_>>();
    let pages = wrapped.chunks(52).collect::<Vec<_>>();
    let page_count = pages.len().max(1);
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            page_count,
            (0..page_count)
                .map(|index| format!("{} 0 R", 4 + index * 2))
                .collect::<Vec<_>>()
                .join(" ")
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    for index in 0..page_count {
        let page_id = 4 + index * 2;
        let content_id = page_id + 1;
        objects.push(
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>")
                .into_bytes(),
        );
        let page_lines = pages.get(index).copied().unwrap_or_default();
        let mut stream = "BT /F1 10 Tf 50 750 Td 14 TL".to_owned();
        for line in page_lines {
            stream.push_str(&format!("\n({}) Tj T*", pdf_escape(line)));
        }
        stream.push_str("\nET");
        objects.push(
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                stream.len(),
                stream
            )
            .into_bytes(),
        );
    }
    let mut pdf = b"%PDF-1.4\n%ForgeML\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    atomic_bytes(path, &pdf)
}

fn wrap_pdf_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    let chars = line.chars().collect::<Vec<_>>();
    chars
        .chunks(width)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

fn pdf_escape(line: &str) -> String {
    line.chars()
        .map(|character| {
            let character = if character.is_ascii_graphic() || character == ' ' {
                character
            } else {
                '?'
            };
            match character {
                '(' => "\\(".into(),
                ')' => "\\)".into(),
                '\\' => "\\\\".into(),
                value => value.to_string(),
            }
        })
        .collect()
}

fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Export path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("Export path requires a valid file name")?;
    let temporary = parent.join(format!(
        ".{file_name}.forge-{}-{}.tmp",
        std::process::id(),
        NEXT_EXPORT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let result = fs::write(&temporary, bytes)
        .map_err(|error| error.to_string())
        .and_then(|()| {
            fs::OpenOptions::new()
                .write(true)
                .open(&temporary)
                .and_then(|file| file.sync_all())
                .map_err(|error| error.to_string())?;
            publish_export(&temporary, path)
        });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
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
    fn streams_multiple_batches_with_one_csv_header() {
        let root = std::env::temp_dir().join(format!("forge-batch-export-{}", std::process::id()));
        let first = Dataset::from_table(
            TableData {
                columns: vec!["value".into()],
                rows: vec![vec!["1".into()]],
            },
            None,
        )
        .unwrap();
        let second = Dataset::from_table(
            TableData {
                columns: vec!["value".into()],
                rows: vec![vec!["2".into()]],
            },
            None,
        )
        .unwrap();
        let batches = vec![first.batches[0].clone(), second.batches[0].clone()];
        let path = root.join("stream.csv");
        dataset_batches(&batches, &path, DataFormat::Csv).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "value\n1\n2\n");
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
    #[test]
    fn native_pdf_writer_paginates_escapes_and_replaces_atomically() {
        let root = std::env::temp_dir().join(format!("forge-pdf-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("report.pdf");
        fs::write(&path, "old").unwrap();
        let lines = (0..60)
            .map(|index| format!("line ({index}) \\ value"))
            .collect::<Vec<_>>();
        write_text_pdf(&path, &lines).unwrap();
        let pdf = fs::read(&path).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let text = String::from_utf8(pdf).unwrap();
        assert!(text.contains("/Count 2"));
        assert!(text.contains(r"line \(0\) \\ value"));
        assert!(text.contains("xref\n0 8"));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
