//! Dataset preparation transforms — categorical encoding, missing-value
//! imputation, and feature scaling — turning selected columns of a raw table
//! into a fully numeric feature table for training or export. Pure and
//! deterministic, so it is unit-tested end-to-end and needs no GUI to verify.

use forge_protocol::TableData;

const MAX_LEVELS: usize = 64;
const MISSING_LEVEL: &str = "(missing)";

/// How to turn a categorical (string) column into numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// One indicator column per distinct level (`col=level`).
    OneHot,
    /// A single column of first-seen level indices (0, 1, 2, …).
    Ordinal,
}

/// What to do with a numeric cell that is empty or non-numeric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// Drop any row that has a missing numeric feature.
    DropRows,
    /// Replace with the column mean of the present values.
    Mean,
    /// Replace with zero.
    Zero,
}

/// How to rescale the numeric feature columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scaling {
    /// Leave numeric columns unchanged.
    None,
    /// Standardize to zero mean and unit variance.
    Standardize,
    /// Rescale to the [0, 1] range.
    MinMax,
}

/// A full preparation recipe over one table.
#[derive(Debug, Clone)]
pub struct PrepConfig {
    /// Columns to turn into numeric features (numeric or categorical).
    pub feature_columns: Vec<String>,
    /// The subset of `feature_columns` to treat as categorical and encode.
    pub categorical_columns: Vec<String>,
    pub encoding: Encoding,
    pub missing: Missing,
    pub scaling: Scaling,
    /// Columns copied through unchanged (typically the target/label column).
    pub passthrough: Vec<String>,
}

/// A human-readable summary of what a transform did.
#[derive(Debug, Clone, PartialEq)]
pub struct PrepReport {
    pub rows_in: usize,
    pub rows_out: usize,
    pub numeric_features: usize,
    pub encoded_features: usize,
    pub dropped_rows: usize,
    pub imputed_cells: usize,
    pub notes: Vec<String>,
}

impl PrepReport {
    /// A compact multi-line summary for the inspector.
    pub fn summary(&self) -> String {
        let mut lines = vec![format!(
            "Prepared {} → {} rows · {} numeric + {} encoded feature columns.",
            self.rows_in, self.rows_out, self.numeric_features, self.encoded_features
        )];
        if self.dropped_rows > 0 {
            lines.push(format!("Dropped {} row(s) with missing values.", self.dropped_rows));
        }
        if self.imputed_cells > 0 {
            lines.push(format!("Imputed {} missing numeric cell(s).", self.imputed_cells));
        }
        lines.extend(self.notes.iter().cloned());
        lines.join("\n")
    }
}

fn cell(row: &[String], index: usize) -> &str {
    row.get(index).map(String::as_str).unwrap_or("")
}

fn parse_num(cell: &str) -> Option<f64> {
    match cell.trim().parse::<f64>() {
        Ok(value) if value.is_finite() => Some(value),
        _ => None,
    }
}

/// Format a finite float as the shortest string that round-trips, so imputed
/// and scaled cells stay readable in the grid.
fn fmt_num(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// Transform selected columns of `table` into a fully numeric feature table
/// following `config`, returning the new table and a report. Passthrough
/// columns come first (unchanged), then imputed/scaled numeric features, then
/// encoded categorical features.
pub fn transform(table: &TableData, config: &PrepConfig) -> Result<(TableData, PrepReport), String> {
    if config.feature_columns.is_empty() {
        return Err("Choose at least one feature column".into());
    }
    let index_of = |name: &str| {
        table
            .columns
            .iter()
            .position(|c| c == name)
            .ok_or_else(|| format!("Column `{name}` was not found"))
    };

    // Categorical columns are the requested subset that are actually features.
    let categorical: Vec<String> = config
        .categorical_columns
        .iter()
        .filter(|c| config.feature_columns.contains(c))
        .cloned()
        .collect();
    let numeric_cols: Vec<String> = config
        .feature_columns
        .iter()
        .filter(|c| !categorical.contains(c))
        .cloned()
        .collect();
    let passthrough: Vec<String> = config
        .passthrough
        .iter()
        .map(|c| c.trim().to_owned())
        .filter(|c| !c.is_empty())
        .collect();
    for column in &passthrough {
        if config.feature_columns.contains(column) {
            return Err(format!(
                "`{column}` cannot be both a feature and a passthrough column"
            ));
        }
    }

    let numeric_idx: Vec<usize> = numeric_cols
        .iter()
        .map(|c| index_of(c))
        .collect::<Result<_, _>>()?;
    let categorical_idx: Vec<usize> = categorical
        .iter()
        .map(|c| index_of(c))
        .collect::<Result<_, _>>()?;
    let passthrough_idx: Vec<usize> = passthrough
        .iter()
        .map(|c| index_of(c))
        .collect::<Result<_, _>>()?;

    // Column means for numeric imputation (present, finite values only).
    let mut means = vec![0.0f64; numeric_idx.len()];
    let mut counts = vec![0usize; numeric_idx.len()];
    for row in &table.rows {
        for (j, &index) in numeric_idx.iter().enumerate() {
            if let Some(value) = parse_num(cell(row, index)) {
                means[j] += value;
                counts[j] += 1;
            }
        }
    }
    for (m, &c) in means.iter_mut().zip(&counts) {
        if c > 0 {
            *m /= c as f64;
        }
    }

    // Distinct levels per categorical column, in first-seen order (capped).
    let mut levels: Vec<Vec<String>> = vec![Vec::new(); categorical_idx.len()];
    for row in &table.rows {
        for (j, &index) in categorical_idx.iter().enumerate() {
            let raw = cell(row, index).trim();
            let level = if raw.is_empty() { MISSING_LEVEL } else { raw };
            if !levels[j].iter().any(|l| l == level) {
                if levels[j].len() >= MAX_LEVELS {
                    return Err(format!(
                        "Column `{}` has more than {MAX_LEVELS} distinct values; \
                         it is not suitable for encoding",
                        categorical[j]
                    ));
                }
                levels[j].push(level.to_owned());
            }
        }
    }

    // First pass: decide kept rows and gather imputed numeric values.
    let mut kept_numeric: Vec<Vec<f64>> = Vec::new();
    let mut kept_rows: Vec<usize> = Vec::new();
    let mut dropped_rows = 0usize;
    let mut imputed_cells = 0usize;
    for (r, row) in table.rows.iter().enumerate() {
        let mut values = Vec::with_capacity(numeric_idx.len());
        let mut drop = false;
        for (j, &index) in numeric_idx.iter().enumerate() {
            match parse_num(cell(row, index)) {
                Some(value) => values.push(value),
                None => match config.missing {
                    Missing::DropRows => {
                        drop = true;
                        break;
                    }
                    Missing::Mean => {
                        imputed_cells += 1;
                        values.push(if counts[j] > 0 { means[j] } else { 0.0 });
                    }
                    Missing::Zero => {
                        imputed_cells += 1;
                        values.push(0.0);
                    }
                },
            }
        }
        if drop {
            dropped_rows += 1;
            continue;
        }
        kept_numeric.push(values);
        kept_rows.push(r);
    }
    if kept_rows.is_empty() {
        return Err("No rows remained after preparation".into());
    }

    // Scaling statistics over the kept numeric rows, then apply in place.
    let d = numeric_idx.len();
    if d > 0 && config.scaling != Scaling::None {
        let n = kept_numeric.len() as f64;
        match config.scaling {
            Scaling::Standardize => {
                let mut mean = vec![0.0; d];
                for row in &kept_numeric {
                    for (j, &v) in row.iter().enumerate() {
                        mean[j] += v;
                    }
                }
                for m in &mut mean {
                    *m /= n;
                }
                let mut std = vec![0.0; d];
                for row in &kept_numeric {
                    for (j, &v) in row.iter().enumerate() {
                        std[j] += (v - mean[j]).powi(2);
                    }
                }
                for s in &mut std {
                    *s = (*s / n).sqrt();
                    if *s < 1e-9 {
                        *s = 1.0;
                    }
                }
                for row in &mut kept_numeric {
                    for (j, v) in row.iter_mut().enumerate() {
                        *v = (*v - mean[j]) / std[j];
                    }
                }
            }
            Scaling::MinMax => {
                let mut lo = vec![f64::INFINITY; d];
                let mut hi = vec![f64::NEG_INFINITY; d];
                for row in &kept_numeric {
                    for (j, &v) in row.iter().enumerate() {
                        lo[j] = lo[j].min(v);
                        hi[j] = hi[j].max(v);
                    }
                }
                for row in &mut kept_numeric {
                    for (j, v) in row.iter_mut().enumerate() {
                        let span = hi[j] - lo[j];
                        *v = if span > 1e-12 { (*v - lo[j]) / span } else { 0.0 };
                    }
                }
            }
            Scaling::None => {}
        }
    }

    // Assemble output columns and rows.
    let mut columns: Vec<String> = passthrough.clone();
    columns.extend(numeric_cols.iter().cloned());
    let mut encoded_features = 0usize;
    for (j, column) in categorical.iter().enumerate() {
        match config.encoding {
            Encoding::OneHot => {
                for level in &levels[j] {
                    columns.push(format!("{column}={level}"));
                    encoded_features += 1;
                }
            }
            Encoding::Ordinal => {
                columns.push(column.clone());
                encoded_features += 1;
            }
        }
    }

    let mut rows: Vec<Vec<String>> = Vec::with_capacity(kept_rows.len());
    for (kept_i, &r) in kept_rows.iter().enumerate() {
        let src = &table.rows[r];
        let mut out: Vec<String> = Vec::with_capacity(columns.len());
        for &index in &passthrough_idx {
            out.push(cell(src, index).to_owned());
        }
        for &value in &kept_numeric[kept_i] {
            out.push(fmt_num(value));
        }
        for (j, &index) in categorical_idx.iter().enumerate() {
            let raw = cell(src, index).trim();
            let level = if raw.is_empty() { MISSING_LEVEL } else { raw };
            match config.encoding {
                Encoding::OneHot => {
                    for candidate in &levels[j] {
                        out.push(if candidate == level { "1".into() } else { "0".into() });
                    }
                }
                Encoding::Ordinal => {
                    let id = levels[j].iter().position(|l| l == level).unwrap_or(0);
                    out.push(id.to_string());
                }
            }
        }
        rows.push(out);
    }

    let mut notes = Vec::new();
    if !categorical.is_empty() {
        let how = match config.encoding {
            Encoding::OneHot => "one-hot",
            Encoding::Ordinal => "ordinal",
        };
        notes.push(format!(
            "Encoded {} categorical column(s) with {how} encoding.",
            categorical.len()
        ));
    }
    if d > 0 {
        let how = match config.scaling {
            Scaling::None => "no",
            Scaling::Standardize => "standardize",
            Scaling::MinMax => "min-max",
        };
        notes.push(format!("Applied {how} scaling to {d} numeric column(s)."));
    }

    let report = PrepReport {
        rows_in: table.rows.len(),
        rows_out: rows.len(),
        numeric_features: numeric_cols.len(),
        encoded_features,
        dropped_rows,
        imputed_cells,
        notes,
    };
    Ok((TableData { columns, rows }, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TableData {
        TableData {
            columns: vec!["age".into(), "city".into(), "label".into()],
            rows: vec![
                vec!["20".into(), "NYC".into(), "a".into()],
                vec!["".into(), "LA".into(), "b".into()], // missing age
                vec!["40".into(), "NYC".into(), "a".into()],
                vec!["60".into(), "SF".into(), "b".into()],
            ],
        }
    }

    fn config() -> PrepConfig {
        PrepConfig {
            feature_columns: vec!["age".into(), "city".into()],
            categorical_columns: vec!["city".into()],
            encoding: Encoding::OneHot,
            missing: Missing::Mean,
            scaling: Scaling::None,
            passthrough: vec!["label".into()],
        }
    }

    #[test]
    fn one_hot_encodes_and_imputes_mean() {
        let (out, report) = transform(&table(), &config()).unwrap();
        // label + age + one-hot(NYC, LA, SF)
        assert_eq!(out.columns, vec!["label", "age", "city=NYC", "city=LA", "city=SF"]);
        assert_eq!(report.rows_out, 4);
        assert_eq!(report.encoded_features, 3);
        assert_eq!(report.imputed_cells, 1);
        // Missing age imputed with mean of {20,40,60} = 40.
        assert_eq!(out.rows[1][1], "40");
        // Row 0 is city=NYC → [1,0,0].
        assert_eq!(&out.rows[0][2..5], &["1", "0", "0"]);
        // Passthrough label preserved.
        assert_eq!(out.rows[3][0], "b");
    }

    #[test]
    fn drop_rows_removes_missing() {
        let mut cfg = config();
        cfg.missing = Missing::DropRows;
        let (out, report) = transform(&table(), &cfg).unwrap();
        assert_eq!(report.rows_out, 3);
        assert_eq!(report.dropped_rows, 1);
        assert_eq!(out.rows.len(), 3);
    }

    #[test]
    fn ordinal_encoding_indexes_levels() {
        let mut cfg = config();
        cfg.encoding = Encoding::Ordinal;
        let (out, _) = transform(&table(), &cfg).unwrap();
        assert_eq!(out.columns, vec!["label", "age", "city"]);
        // First-seen order: NYC=0, LA=1, SF=2.
        assert_eq!(out.rows[0][2], "0");
        assert_eq!(out.rows[1][2], "1");
        assert_eq!(out.rows[3][2], "2");
    }

    #[test]
    fn minmax_scaling_maps_to_unit_range() {
        let mut cfg = config();
        cfg.scaling = Scaling::MinMax;
        let (out, _) = transform(&table(), &cfg).unwrap();
        // age after mean-impute: [20,40,40,60] → min 20, max 60.
        assert_eq!(out.rows[0][1], "0"); // (20-20)/40
        assert_eq!(out.rows[3][1], "1"); // (60-20)/40
        assert_eq!(out.rows[1][1], "0.5"); // (40-20)/40
    }

    #[test]
    fn standardize_yields_zero_mean() {
        let mut cfg = config();
        cfg.scaling = Scaling::Standardize;
        let (out, _) = transform(&table(), &cfg).unwrap();
        let sum: f64 = out.rows.iter().map(|r| r[1].parse::<f64>().unwrap()).sum();
        assert!(sum.abs() < 1e-9, "standardized column should have ~zero mean");
    }

    #[test]
    fn rejects_feature_and_passthrough_overlap() {
        let mut cfg = config();
        cfg.passthrough = vec!["age".into()];
        assert!(transform(&table(), &cfg).is_err());
    }

    #[test]
    fn missing_category_becomes_its_own_level() {
        let mut t = table();
        t.rows.push(vec!["10".into(), "".into(), "a".into()]);
        let (out, _) = transform(&t, &config()).unwrap();
        assert!(out.columns.iter().any(|c| c == "city=(missing)"));
    }
}
