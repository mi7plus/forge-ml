use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;

macro_rules! stable_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4().to_string())
            }

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

stable_id!(DatasetId);
stable_id!(RunId);
stable_id!(PlotId);
stable_id!(KernelId);
stable_id!(ArtifactId);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableData {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ForgeEvent {
    Metric { name: String, value: f64 },
    Vector { name: String, values: Vec<f64> },
    Table { name: String, data: TableData },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventEnvelope {
    pub version: u16,
    pub event: ForgeEvent,
}

impl EventEnvelope {
    pub fn new(event: ForgeEvent) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            event,
        }
    }
}

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
