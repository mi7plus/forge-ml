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
    path::Path,
};

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
}
