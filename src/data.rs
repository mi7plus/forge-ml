use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::ipc::reader::FileReader;
use arrow::util::display::array_value_to_string;
use forge_protocol::{ForgeEvent, TableData};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    ops::Deref,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

const MAX_IMPORT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_IMPORT_ROWS: usize = 1_000_000;
const MAX_IMPORT_COLUMNS: usize = 10_000;
const MAX_DECODED_BYTES: usize = 512 * 1024 * 1024;
const MAX_CELL_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
struct ImportLimits {
    rows: usize,
    columns: usize,
    decoded_bytes: usize,
    cell_bytes: usize,
}

const IMPORT_LIMITS: ImportLimits = ImportLimits {
    rows: MAX_IMPORT_ROWS,
    columns: MAX_IMPORT_COLUMNS,
    decoded_bytes: MAX_DECODED_BYTES,
    cell_bytes: MAX_CELL_BYTES,
};

static NEXT_DATASET_REVISION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct Dataset {
    pub table: TableData,
    pub batch: RecordBatch,
    pub source: Option<String>,
    pub revision: u64,
}

impl Deref for Dataset {
    type Target = TableData;
    fn deref(&self) -> &Self::Target {
        &self.table
    }
}

impl Dataset {
    pub fn from_table(table: TableData, source: Option<String>) -> Result<Self, String> {
        let fields = table
            .columns
            .iter()
            .map(|name| Field::new(name, DataType::Utf8, true))
            .collect::<Vec<_>>();
        let arrays = (0..table.columns.len())
            .map(|column| {
                Arc::new(StringArray::from(
                    table
                        .rows
                        .iter()
                        .map(|row| row.get(column).map(String::as_str))
                        .collect::<Vec<_>>(),
                )) as ArrayRef
            })
            .collect::<Vec<_>>();
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            table,
            batch,
            source,
            revision: NEXT_DATASET_REVISION.fetch_add(1, Ordering::Relaxed),
        })
    }

    pub fn profile(&self) -> Vec<ColumnProfile> {
        self.table
            .columns
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let mut missing = 0;
                let mut unique = std::collections::HashSet::new();
                let mut numeric = Vec::new();
                for value in self.table.rows.iter().filter_map(|row| row.get(index)) {
                    if value.trim().is_empty() {
                        missing += 1;
                    }
                    unique.insert(value);
                    if let Ok(value) = value.parse::<f64>() {
                        numeric.push(value);
                    }
                }
                ColumnProfile {
                    name: name.clone(),
                    missing,
                    unique: unique.len(),
                    min: numeric.iter().copied().reduce(f64::min),
                    max: numeric.iter().copied().reduce(f64::max),
                    mean: (!numeric.is_empty())
                        .then(|| numeric.iter().sum::<f64>() / numeric.len() as f64),
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ColumnProfile {
    pub name: String,
    pub missing: usize,
    pub unique: usize,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
}

#[derive(Default)]
pub struct DataWorkspace {
    pub metrics: HashMap<String, Vec<[f64; 2]>>,
    pub vectors: HashMap<String, Vec<f64>>,
    pub tables: HashMap<String, Dataset>,
}

impl DataWorkspace {
    pub fn insert_table(
        &mut self,
        name: String,
        table: TableData,
        source: String,
    ) -> Result<(), String> {
        self.tables
            .insert(name, Dataset::from_table(table, Some(source))?);
        Ok(())
    }
    pub fn fingerprints(&self) -> HashMap<String, String> {
        self.tables
            .iter()
            .map(|(name, dataset)| {
                let bytes = serde_json::to_vec(&dataset.table).unwrap_or_default();
                (name.clone(), crate::experiment::stable_digest(&bytes))
            })
            .collect()
    }
    pub fn source_fingerprints(&self) -> HashMap<String, String> {
        self.tables
            .iter()
            .filter_map(|(name, dataset)| {
                dataset.source.as_ref().map(|source| {
                    (
                        name.clone(),
                        crate::experiment::stable_digest(source.as_bytes()),
                    )
                })
            })
            .collect()
    }
    pub fn apply(&mut self, event: ForgeEvent) {
        match event {
            ForgeEvent::Metric { name, value } => {
                let series = self.metrics.entry(name).or_default();
                series.push([series.len() as f64, value]);
            }
            ForgeEvent::Vector { name, values } => {
                self.vectors.insert(name, values);
            }
            ForgeEvent::Table { name, data } => {
                if let Ok(dataset) = Dataset::from_table(data, Some("runtime".into())) {
                    self.tables.insert(name, dataset);
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.metrics.clear();
        self.vectors.clear();
        self.tables.clear();
    }
    pub fn has_telemetry(&self) -> bool {
        !self.metrics.is_empty() || !self.vectors.is_empty()
    }
}

pub fn load_table(path: &Path) -> Result<(String, TableData, String), String> {
    load_table_with_limits(path, IMPORT_LIMITS)
}

fn load_table_with_limits(
    path: &Path,
    limits: ImportLimits,
) -> Result<(String, TableData, String), String> {
    validate_import_file(path)?;
    let ext = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let table = match ext.as_str() {
        "csv" => delimited(path, b',', limits),
        "tsv" => delimited(path, b'\t', limits),
        "jsonl" | "ndjson" => json_lines(path, limits),
        "parquet" => record_batches(
            ParquetRecordBatchReaderBuilder::try_new(File::open(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
                .with_batch_size(8_192)
                .build()
                .map_err(|e| e.to_string())?,
            limits,
        ),
        "arrow" | "ipc" => record_batches(
            FileReader::try_new(File::open(path).map_err(|e| e.to_string())?, None)
                .map_err(|e| e.to_string())?,
            limits,
        ),
        _ => return Err("Supported formats: CSV, TSV, JSON Lines, Parquet, Arrow IPC.".into()),
    }?;
    let name = path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("dataset")
        .to_owned();
    Ok((name, table, path.display().to_string()))
}

fn validate_import_file(path: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("Dataset import requires a regular file.".into());
    }
    if metadata.len() > MAX_IMPORT_BYTES {
        return Err(
            "Dataset files larger than 512 MiB must be queried or streamed instead of imported."
                .into(),
        );
    }
    Ok(())
}

pub fn load_millwright_table(path: &Path) -> Result<(String, TableData, String), String> {
    validate_import_file(path)?;
    let table = match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "csv" => millwright::table::Table::from_csv(path),
        "parquet" => millwright::table::Table::from_parquet(path),
        _ => return Err("Millwright imports CSV and Parquet tables.".into()),
    }
    .map_err(|error| error.to_string())?;
    let name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("millwright_dataset")
        .to_owned();
    Ok((
        name,
        millwright_table_data(&table, IMPORT_LIMITS)?,
        format!("published Millwright 2.2.1: {}", path.display()),
    ))
}

fn millwright_table_data(
    table: &millwright::table::Table,
    limits: ImportLimits,
) -> Result<TableData, String> {
    let mut output = TableBuilder::new(table.column_names(), limits)?;
    for index in 0..table.nrows() {
        let row = table
            .as_polars()
            .get_row(index)
            .map_err(|error| error.to_string())?
            .0
            .into_iter()
            .map(|value| value.to_string())
            .collect();
        output.push(row)?;
    }
    Ok(output.finish())
}

struct TableBuilder {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    decoded_bytes: usize,
    limits: ImportLimits,
}

impl TableBuilder {
    fn new(columns: Vec<String>, limits: ImportLimits) -> Result<Self, String> {
        if columns.len() > limits.columns {
            return Err(format!(
                "Dataset exceeds the {}-column import limit.",
                limits.columns
            ));
        }
        let decoded_bytes = columns.iter().map(String::len).sum();
        if decoded_bytes > limits.decoded_bytes {
            return Err("Dataset headers exceed the decoded-data import limit.".into());
        }
        Ok(Self {
            columns,
            rows: Vec::new(),
            decoded_bytes,
            limits,
        })
    }

    fn push(&mut self, row: Vec<String>) -> Result<(), String> {
        if self.rows.len() >= self.limits.rows {
            return Err(format!(
                "Dataset exceeds the {}-row import limit.",
                self.limits.rows
            ));
        }
        if row.len() != self.columns.len() {
            return Err(format!(
                "Dataset row has {} values but the schema has {} columns.",
                row.len(),
                self.columns.len()
            ));
        }
        for value in &row {
            if value.len() > self.limits.cell_bytes {
                return Err(format!(
                    "Dataset cell exceeds the {} MiB import limit.",
                    self.limits.cell_bytes / (1024 * 1024)
                ));
            }
            self.decoded_bytes = self
                .decoded_bytes
                .checked_add(value.len())
                .ok_or("Decoded dataset size overflow")?;
            if self.decoded_bytes > self.limits.decoded_bytes {
                return Err(format!(
                    "Dataset exceeds the {} MiB decoded-data import limit.",
                    self.limits.decoded_bytes / (1024 * 1024)
                ));
            }
        }
        self.rows.push(row);
        Ok(())
    }

    fn finish(self) -> TableData {
        TableData {
            columns: self.columns,
            rows: self.rows,
        }
    }
}

fn delimited(path: &Path, delimiter: u8, limits: ImportLimits) -> Result<TableData, String> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let columns = reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(str::to_owned)
        .collect();
    let mut table = TableBuilder::new(columns, limits)?;
    for record in reader.records() {
        table.push(
            record
                .map_err(|error| error.to_string())?
                .iter()
                .map(str::to_owned)
                .collect(),
        )?;
    }
    Ok(table.finish())
}

fn json_lines(path: &Path, limits: ImportLimits) -> Result<TableData, String> {
    let mut columns = Vec::new();
    let mut known = std::collections::HashSet::new();
    let mut row_count = 0usize;
    for line in BufReader::new(File::open(path).map_err(|e| e.to_string())?).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > limits.cell_bytes {
            return Err(format!(
                "JSON line exceeds the {} MiB import limit.",
                limits.cell_bytes / (1024 * 1024)
            ));
        }
        row_count += 1;
        if row_count > limits.rows {
            return Err(format!(
                "Dataset exceeds the {}-row import limit.",
                limits.rows
            ));
        }
        let object = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&line)
            .map_err(|e| e.to_string())?;
        for key in object.keys() {
            if known.insert(key.clone()) {
                columns.push(key.clone());
                if columns.len() > limits.columns {
                    return Err(format!(
                        "Dataset exceeds the {}-column import limit.",
                        limits.columns
                    ));
                }
            }
        }
    }
    let mut table = TableBuilder::new(columns, limits)?;
    for line in BufReader::new(File::open(path).map_err(|e| e.to_string())?).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let object = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&line)
            .map_err(|e| e.to_string())?;
        let row = table
            .columns
            .iter()
            .map(|key| match object.get(key) {
                Some(serde_json::Value::String(value)) => value.clone(),
                Some(serde_json::Value::Null) | None => String::new(),
                Some(value) => value.to_string(),
            })
            .collect();
        table.push(row)?;
    }
    Ok(table.finish())
}

fn record_batches<I>(batches: I, limits: ImportLimits) -> Result<TableData, String>
where
    I: IntoIterator<Item = Result<RecordBatch, arrow::error::ArrowError>>,
{
    let mut table: Option<TableBuilder> = None;
    for batch in batches {
        let batch = batch.map_err(|e| e.to_string())?;
        let batch_columns = batch
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect::<Vec<_>>();
        if let Some(table) = &table {
            if table.columns != batch_columns {
                return Err("Arrow record batches contain incompatible schemas.".into());
            }
        } else {
            table = Some(TableBuilder::new(batch_columns, limits)?);
        }
        for row in 0..batch.num_rows() {
            table.as_mut().expect("table initialized above").push(
                batch
                    .columns()
                    .iter()
                    .map(|array| {
                        array_value_to_string(array.as_ref(), row).map_err(|e| e.to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )?;
        }
    }
    Ok(table.map(TableBuilder::finish).unwrap_or(TableData {
        columns: Vec::new(),
        rows: Vec::new(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creates_arrow_batch_and_profiles_table() {
        let dataset = Dataset::from_table(
            TableData {
                columns: vec!["x".into()],
                rows: vec![vec!["1".into()], vec!["".into()], vec!["3".into()]],
            },
            None,
        )
        .unwrap();
        assert_eq!(dataset.batch.num_rows(), 3);
        assert_eq!(dataset.profile()[0].mean, Some(2.0));
    }

    #[test]
    fn replacement_datasets_receive_new_revisions() {
        let table = TableData {
            columns: vec!["x".into()],
            rows: vec![vec!["1".into()]],
        };
        let first = Dataset::from_table(table.clone(), None).unwrap();
        let second = Dataset::from_table(table, None).unwrap();
        assert_ne!(first.revision, second.revision);
    }

    #[test]
    fn import_budget_rejects_excess_rows_and_decoded_bytes() {
        let limits = ImportLimits {
            rows: 1,
            columns: 2,
            decoded_bytes: 8,
            cell_bytes: 8,
        };
        let mut rows = TableBuilder::new(vec!["x".into()], limits).unwrap();
        rows.push(vec!["1".into()]).unwrap();
        assert!(rows.push(vec!["2".into()]).unwrap_err().contains("row"));

        let mut bytes = TableBuilder::new(vec!["x".into()], limits).unwrap();
        assert!(bytes
            .push(vec!["12345678".into()])
            .unwrap_err()
            .contains("decoded-data"));
    }

    #[test]
    fn json_lines_streaming_preserves_late_columns() {
        let path = std::env::temp_dir().join(format!("forge-jsonl-{}.jsonl", std::process::id()));
        std::fs::write(&path, "{\"a\":1}\n{\"b\":2}\n").unwrap();
        let table = json_lines(&path, IMPORT_LIMITS).unwrap();
        assert_eq!(table.columns, ["a", "b"]);
        assert_eq!(table.rows, [vec!["1", ""], vec!["", "2"]]);
        let _ = std::fs::remove_file(path);
    }
}
