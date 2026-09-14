//! Wire types shared between the Forge IDE and the code it runs.
//!
//! A notebook or project prints telemetry to stdout using the `forge_*` line
//! prefixes; the IDE parses those lines back into typed [`EventEnvelope`]s with
//! [`parse_stdout_events`]. This crate is the single source of truth for that
//! contract: the [`PROTOCOL_VERSION`], the opaque identifier types
//! ([`DatasetId`], [`RunId`], [`PlotId`], [`KernelId`], [`ArtifactId`]), and the
//! [`ForgeEvent`] payloads (metric, vector, table). It is deliberately tiny and
//! dependency-light so every workspace crate can depend on it.

#![deny(missing_docs)]

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Version stamped into every [`EventEnvelope`]; the IDE ignores events whose
/// version it does not recognize, so this is bumped on any breaking change.
pub const PROTOCOL_VERSION: u16 = 1;

macro_rules! stable_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Generate a fresh, globally-unique identifier (a random UUIDv4).
            pub fn new() -> Self {
                Self(Uuid::new_v4().to_string())
            }

            /// Borrow the identifier's string form (as persisted and serialized).
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

stable_id!(
    /// Identifies a dataset registered in the workspace.
    DatasetId
);
stable_id!(
    /// Identifies a single experiment run / execution.
    RunId
);
stable_id!(
    /// Identifies a structured plot produced by a run.
    PlotId
);
stable_id!(
    /// Identifies a language/execution kernel.
    KernelId
);
stable_id!(
    /// Identifies an exported or registered artifact.
    ArtifactId
);

/// A rectangular table of stringified cells emitted by a `forge_table:` line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableData {
    /// Column headers, left to right.
    pub columns: Vec<String>,
    /// Row-major cell values; every row has `columns.len()` entries.
    pub rows: Vec<Vec<String>>,
}

/// One piece of telemetry decoded from a program's stdout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ForgeEvent {
    /// A single named scalar (e.g. a loss value at a step).
    Metric {
        /// Metric name.
        name: String,
        /// Metric value.
        value: f64,
    },
    /// A named numeric vector (e.g. a weight row or an embedding).
    Vector {
        /// Vector name.
        name: String,
        /// Vector values.
        values: Vec<f64>,
    },
    /// A named rectangular table.
    Table {
        /// Table name.
        name: String,
        /// Table contents.
        data: TableData,
    },
}

/// A [`ForgeEvent`] tagged with the [`PROTOCOL_VERSION`] it was produced under.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventEnvelope {
    /// Protocol version the event was serialized with.
    pub version: u16,
    /// The wrapped event payload.
    pub event: ForgeEvent,
}

impl EventEnvelope {
    /// Wrap `event` in an envelope stamped with the current [`PROTOCOL_VERSION`].
    pub fn new(event: ForgeEvent) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            event,
        }
    }
}

/// Parse every recognized `forge_*` telemetry line out of a block of program
/// stdout, discarding anything else and any event of an unknown version.
pub fn parse_stdout_events(output: &str) -> Vec<EventEnvelope> {
    output.lines().filter_map(parse_legacy_line).collect()
}

fn parse_legacy_line(line: &str) -> Option<EventEnvelope> {
    let line = line.trim();
    if let Some(payload) = line.strip_prefix("forge_event:") {
        let envelope: EventEnvelope = serde_json::from_str(payload.trim()).ok()?;
        return (envelope.version == PROTOCOL_VERSION).then_some(envelope);
    }
    if let Some(payload) = line.strip_prefix("forge_metric:") {
        let (name, value) = payload.split_once('=')?;
        return value.trim().parse().ok().map(|value| {
            EventEnvelope::new(ForgeEvent::Metric {
                name: name.trim().to_owned(),
                value,
            })
        });
    }
    if let Some(payload) = line.strip_prefix("forge_vector:") {
        let (name, values) = payload.split_once('=')?;
        let values = values
            .split(',')
            .filter_map(|value| value.trim().parse().ok())
            .collect::<Vec<_>>();
        return (!values.is_empty()).then(|| {
            EventEnvelope::new(ForgeEvent::Vector {
                name: name.trim().to_owned(),
                values,
            })
        });
    }
    if let Some(payload) = line.strip_prefix("forge_table:") {
        let (name, json) = payload.split_once('=')?;
        let value: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
        let columns = value
            .get("columns")?
            .as_array()?
            .iter()
            .map(json_cell)
            .collect::<Vec<_>>();
        let rows = value
            .get("rows")?
            .as_array()?
            .iter()
            .map(|row| {
                row.as_array()
                    .map(|cells| cells.iter().map(json_cell).collect())
            })
            .collect::<Option<Vec<Vec<String>>>>()?;
        if columns.is_empty() || rows.iter().any(|row| row.len() != columns.len()) {
            return None;
        }
        return Some(EventEnvelope::new(ForgeEvent::Table {
            name: name.trim().to_owned(),
            data: TableData { columns, rows },
        }));
    }
    None
}

fn json_cell(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ids_round_trip() {
        let id = DatasetId::new();
        let restored: DatasetId =
            serde_json::from_str(&serde_json::to_string(&id).unwrap()).unwrap();
        assert_eq!(restored, id);
    }

    #[test]
    fn adapts_legacy_metric_vector_and_table_output() {
        let events = parse_stdout_events(
            "forge_metric:loss=0.42\nforge_vector:w=1,2\nforge_table:d={\"columns\":[\"x\"],\"rows\":[[1]]}",
        );
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0].event, ForgeEvent::Metric { .. }));
        assert!(matches!(events[1].event, ForgeEvent::Vector { .. }));
        assert!(matches!(events[2].event, ForgeEvent::Table { .. }));
    }

    #[test]
    fn accepts_only_current_versioned_events() {
        let event = EventEnvelope::new(ForgeEvent::Metric {
            name: "loss".into(),
            value: 1.0,
        });
        let line = format!("forge_event:{}", serde_json::to_string(&event).unwrap());
        assert_eq!(parse_stdout_events(&line), vec![event]);

        let old = line.replace("\"version\":1", "\"version\":0");
        assert!(parse_stdout_events(&old).is_empty());
    }
}
