//! Pure data-grid computations for the dataset viewer: row filtering/sorting
//! (including a cancellable variant used by the background indexer), the visible
//! column window for horizontal virtualization, and row/column projection.

use forge_protocol::TableData;
use std::collections::BTreeSet;

/// Sentinel `sort_column` value meaning "sort by the original row index" (the `#`
/// column). Real column indices are always `< usize::MAX`, so this never collides.
pub const INDEX_SORT_COLUMN: usize = usize::MAX;

/// How a [`RowFilter`]'s text is compared against cell values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterMode {
    #[default]
    Contains,
    NotContains,
    Equals,
    StartsWith,
    GreaterThan,
    LessThan,
}

impl FilterMode {
    /// All modes, in the order they appear in the picker.
    pub const ALL: [FilterMode; 6] = [
        FilterMode::Contains,
        FilterMode::NotContains,
        FilterMode::Equals,
        FilterMode::StartsWith,
        FilterMode::GreaterThan,
        FilterMode::LessThan,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FilterMode::Contains => "contains",
            FilterMode::NotContains => "does not contain",
            FilterMode::Equals => "equals",
            FilterMode::StartsWith => "starts with",
            FilterMode::GreaterThan => "greater than",
            FilterMode::LessThan => "less than",
        }
    }

    /// Numeric modes parse both the query and the cell as `f64`.
    pub fn is_numeric(self) -> bool {
        matches!(self, FilterMode::GreaterThan | FilterMode::LessThan)
    }
}

/// A dataset row filter: match `text` against either every column or one chosen
/// column, using `mode`. An empty query matches every row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowFilter {
    pub text: String,
    /// `None` scans every column; `Some(index)` restricts to that column.
    pub column: Option<usize>,
    pub mode: FilterMode,
}

impl From<&str> for RowFilter {
    fn from(text: &str) -> Self {
        RowFilter {
            text: text.to_owned(),
            ..Default::default()
        }
    }
}

impl RowFilter {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// The cell indices this filter inspects for `row`.
    fn columns<'a>(&self, row: &'a [String]) -> Box<dyn Iterator<Item = &'a String> + 'a> {
        match self.column {
            Some(index) => Box::new(row.get(index).into_iter()),
            None => Box::new(row.iter()),
        }
    }

    /// Whether `row` passes this filter.
    pub fn matches(&self, row: &[String]) -> bool {
        if self.is_empty() {
            return true;
        }
        if self.mode.is_numeric() {
            let Ok(threshold) = self.text.trim().parse::<f64>() else {
                // A non-numeric query in a numeric mode filters nothing out.
                return true;
            };
            return self.columns(row).any(|value| {
                value
                    .trim()
                    .parse::<f64>()
                    .is_ok_and(|cell| match self.mode {
                        FilterMode::GreaterThan => cell > threshold,
                        FilterMode::LessThan => cell < threshold,
                        _ => false,
                    })
            });
        }
        let needle = self.text.to_lowercase();
        match self.mode {
            FilterMode::NotContains => !self
                .columns(row)
                .any(|value| value.to_lowercase().contains(&needle)),
            FilterMode::Contains => self
                .columns(row)
                .any(|value| value.to_lowercase().contains(&needle)),
            FilterMode::Equals => self
                .columns(row)
                .any(|value| value.to_lowercase() == needle),
            FilterMode::StartsWith => self
                .columns(row)
                .any(|value| value.to_lowercase().starts_with(&needle)),
            FilterMode::GreaterThan | FilterMode::LessThan => unreachable!("handled above"),
        }
    }
}

pub fn build_row_index(
    data: &TableData,
    filter: &RowFilter,
    sort_column: Option<usize>,
    sort_descending: bool,
) -> Vec<usize> {
    build_row_index_cancellable(data, filter, sort_column, sort_descending, || false)
        .expect("non-cancellable row indexing")
}

pub fn build_row_index_cancellable(
    data: &TableData,
    filter: &RowFilter,
    sort_column: Option<usize>,
    sort_descending: bool,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<usize>> {
    let mut rows = Vec::with_capacity(data.rows.len());
    for (index, row) in data.rows.iter().enumerate() {
        if index.is_multiple_of(1_024) && cancelled() {
            return None;
        }
        if filter.matches(row) {
            rows.push(index);
        }
    }
    if let Some(column) = sort_column {
        if cancelled() {
            return None;
        }
        // The `#` column sorts by original position. `rows` is already in
        // ascending-index order, so descending is just a reverse.
        if column == INDEX_SORT_COLUMN {
            if sort_descending {
                rows.reverse();
            }
            return (!cancelled()).then_some(rows);
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
