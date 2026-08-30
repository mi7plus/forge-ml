//! Pure data-grid computations for the dataset viewer: row filtering/sorting
//! (including a cancellable variant used by the background indexer), the visible
//! column window for horizontal virtualization, and row/column projection.

use forge_protocol::TableData;
use std::collections::BTreeSet;

pub fn build_row_index(
    data: &TableData,
    filter: &str,
    sort_column: Option<usize>,
    sort_descending: bool,
) -> Vec<usize> {
    build_row_index_cancellable(data, filter, sort_column, sort_descending, || false)
        .expect("non-cancellable row indexing")
}

pub fn build_row_index_cancellable(
    data: &TableData,
    filter: &str,
    sort_column: Option<usize>,
    sort_descending: bool,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<usize>> {
    let needle = filter.to_lowercase();
    let mut rows = Vec::with_capacity(data.rows.len());
    for (index, row) in data.rows.iter().enumerate() {
        if index.is_multiple_of(1_024) && cancelled() {
            return None;
        }
        if needle.is_empty()
            || row
                .iter()
                .any(|value| value.to_lowercase().contains(&needle))
        {
            rows.push(index);
        }
    }
    if let Some(column) = sort_column {
        if cancelled() {
            return None;
        }
        let mut numeric = Vec::with_capacity(rows.len());
        let mut all_numeric = true;
        for (position, index) in rows.iter().enumerate() {
            if position.is_multiple_of(1_024) && cancelled() {
                return None;
            }
            match data.rows[*index]
                .get(column)
                .map(String::as_str)
                .unwrap_or_default()
                .parse::<f64>()
            {
                Ok(value) => numeric.push((*index, value)),
                Err(_) => {
                    all_numeric = false;
                    break;
                }
            }
        }
        rows = if all_numeric {
            let mut keyed = numeric;
            keyed.sort_by(|left, right| {
                let ordering = left.1.total_cmp(&right.1);
                if sort_descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
            keyed.into_iter().map(|(index, _)| index).collect()
        } else {
            let mut keyed = Vec::with_capacity(rows.len());
            for (position, index) in rows.into_iter().enumerate() {
                if position.is_multiple_of(1_024) && cancelled() {
                    return None;
                }
                let key = data.rows[index]
                    .get(column)
                    .map(String::as_str)
                    .unwrap_or_default()
                    .to_lowercase();
                keyed.push((index, key));
            }
            keyed.sort_by(|left, right| {
                let ordering = left.1.cmp(&right.1);
                if sort_descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
            keyed.into_iter().map(|(index, _)| index).collect()
        };
    }
    (!cancelled()).then_some(rows)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnWindow {
    pub start: usize,
    pub end: usize,
    pub leading: f32,
    pub trailing: f32,
}

pub fn visible_column_window(
    columns: &[usize],
    widths: &[f32],
    offset: f32,
    viewport_width: f32,
) -> ColumnWindow {
    if columns.is_empty() {
        return ColumnWindow {
            start: 0,
            end: 0,
            leading: 0.0,
            trailing: 0.0,
        };
    }
    let cell_width = |column: usize| widths.get(column).copied().unwrap_or(120.0) + 8.0;
    let total = columns
        .iter()
        .map(|column| cell_width(*column))
        .sum::<f32>();
    let offset = offset.max(0.0).min(total);
    let viewport_end = offset + viewport_width.max(1.0);
    let mut cursor = 0.0;
    let mut first = 0;
    while first < columns.len() && cursor + cell_width(columns[first]) < offset {
        cursor += cell_width(columns[first]);
        first += 1;
    }
    first = first.saturating_sub(1);
    let leading = columns[..first]
        .iter()
        .map(|column| cell_width(*column))
        .sum::<f32>();
    cursor = leading;
    let mut end = first;
    while end < columns.len() && cursor < viewport_end {
        cursor += cell_width(columns[end]);
        end += 1;
    }
    end = (end + 1).min(columns.len());
    let rendered = columns[first..end]
        .iter()
        .map(|column| cell_width(*column))
        .sum::<f32>();
    ColumnWindow {
        start: first,
        end,
        leading,
        trailing: (total - leading - rendered).max(0.0),
    }
}

pub fn selected_table(
    data: &TableData,
    selected_rows: &BTreeSet<usize>,
    columns: &[usize],
) -> TableData {
    let rows = data
        .rows
        .iter()
        .enumerate()
        .filter(|(index, _)| selected_rows.is_empty() || selected_rows.contains(index))
        .map(|(_, row)| {
            columns
                .iter()
                .map(|column| row.get(*column).cloned().unwrap_or_default())
                .collect()
        })
        .collect();
    TableData {
        columns: columns
            .iter()
            .map(|index| data.columns[*index].clone())
            .collect(),
        rows,
    }
}
