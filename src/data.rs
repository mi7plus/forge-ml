use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::ipc::reader::FileReader;
use arrow::util::display::array_value_to_string;
use forge_protocol::{ForgeEvent, TableData};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::{
    collections::HashMap,
    fs::File,
    ops::Deref,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
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

    #[cfg(feature = "millwright")]
    #[allow(dead_code)]
    pub fn from_millwright(table: &millwright::table::Table) -> Result<Self, String> {
        let columns = table.column_names();
        let rows = (0..table.nrows())
            .map(|index| {
                table
                    .as_polars()
                    .get_row(index)
                    .map(|row| row.0.into_iter().map(|value| value.to_string()).collect())
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_table(
            TableData { columns, rows },
            Some("millwright::Table".into()),
        )
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

    #[cfg(feature = "millwright")]
    pub fn import_millwright(&mut self, path: &Path) -> Result<String, String> {
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
        self.tables
            .insert(name.clone(), Dataset::from_millwright(&table)?);
        Ok(name)
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
    const MAX_IMPORT_BYTES: u64 = 512 * 1024 * 1024;
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
    let ext = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let table = match ext.as_str() {
        "csv" => delimited(path, b','),
        "tsv" => delimited(path, b'\t'),
        "jsonl" | "ndjson" => json_lines(path),
        "parquet" => record_batches(
            ParquetRecordBatchReaderBuilder::try_new(File::open(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
                .build()
                .map_err(|e| e.to_string())?,
        ),
        "arrow" | "ipc" => record_batches(
            FileReader::try_new(File::open(path).map_err(|e| e.to_string())?, None)
                .map_err(|e| e.to_string())?,
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

fn delimited(path: &Path, delimiter: u8) -> Result<TableData, String> {
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
    let rows = reader
        .records()
        .map(|r| {
            r.map(|r| r.iter().map(str::to_owned).collect())
                .map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    Ok(TableData { columns, rows })
}

fn json_lines(path: &Path) -> Result<TableData, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let objects = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(line)
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut columns = Vec::new();
    for object in &objects {
        for key in object.keys() {
            if !columns.contains(key) {
                columns.push(key.clone());
            }
        }
    }
    let rows = objects
        .into_iter()
        .map(|object| {
            columns
                .iter()
                .map(|key| match object.get(key) {
                    Some(serde_json::Value::String(v)) => v.clone(),
                    Some(serde_json::Value::Null) | None => String::new(),
                    Some(v) => v.to_string(),
                })
                .collect()
        })
        .collect();
    Ok(TableData { columns, rows })
}

fn record_batches<I>(batches: I) -> Result<TableData, String>
where
    I: IntoIterator<Item = Result<RecordBatch, arrow::error::ArrowError>>,
{
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    for batch in batches {
        let batch = batch.map_err(|e| e.to_string())?;
        if columns.is_empty() {
            columns = batch
                .schema()
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect();
        }
        for row in 0..batch.num_rows() {
            rows.push(
                batch
                    .columns()
                    .iter()
                    .map(|a| array_value_to_string(a.as_ref(), row).unwrap_or_default())
                    .collect(),
            );
        }
    }
    Ok(TableData { columns, rows })
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
}
