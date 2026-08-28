use serde::{Deserialize, Serialize};

pub const MAX_MONITOR_EVENTS: usize = 10_000;
const MAX_MONITOR_TEXT_BYTES: usize = 256;
const MAX_MONITOR_JSON_BYTES: usize = 16 * 1024 * 1024;
const MONITOR_SCHEMA: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceEvent {
    pub model: String,
    pub version: String,
    pub requests: u64,
    #[serde(default)]
    pub errors: u64,
    #[serde(default)]
    pub p95_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DriftEvent {
    pub model: String,
    pub version: String,
    pub feature: String,
    pub score: f64,
    #[serde(default)]
    pub threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MonitoringSnapshot {
    pub schema: u16,
    pub service_events: Vec<ServiceEvent>,
    pub drift_events: Vec<DriftEvent>,
}

fn safe_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_MONITOR_TEXT_BYTES && !value.contains('\0')
}

pub fn valid_service(event: &ServiceEvent) -> bool {
    safe_text(&event.model)
        && safe_text(&event.version)
        && event.errors <= event.requests
        && event
            .p95_ms
            .is_none_or(|value| value.is_finite() && value >= 0.0)
}

pub fn valid_drift(event: &DriftEvent) -> bool {
    safe_text(&event.model)
        && safe_text(&event.version)
        && safe_text(&event.feature)
        && event.score.is_finite()
        && event.score >= 0.0
        && event.threshold.is_finite()
        && event.threshold >= 0.0
}

pub fn record_service(events: &mut Vec<ServiceEvent>, event: ServiceEvent) {
    if valid_service(&event) {
        retain_push(events, event);
    }
}

pub fn record_drift(events: &mut Vec<DriftEvent>, event: DriftEvent) {
    if valid_drift(&event) {
        retain_push(events, event);
    }
}

fn retain_push<T>(events: &mut Vec<T>, event: T) {
    if events.len() >= MAX_MONITOR_EVENTS {
        let remove = events.len() - MAX_MONITOR_EVENTS + 1;
        events.drain(..remove);
    }
    events.push(event);
}

pub fn snapshot_json(
    service_events: &[ServiceEvent],
    drift_events: &[DriftEvent],
) -> Result<Vec<u8>, String> {
    if service_events.len() > MAX_MONITOR_EVENTS
        || drift_events.len() > MAX_MONITOR_EVENTS
        || service_events.iter().any(|event| !valid_service(event))
        || drift_events.iter().any(|event| !valid_drift(event))
    {
        return Err("Monitoring snapshot contains invalid or excessive events".into());
    }
    let output = serde_json::to_vec_pretty(&MonitoringSnapshot {
        schema: MONITOR_SCHEMA,
        service_events: service_events.to_vec(),
        drift_events: drift_events.to_vec(),
    })
    .map_err(|error| error.to_string())?;
    if output.len() > MAX_MONITOR_JSON_BYTES {
        return Err("Monitoring snapshot exceeds the 16 MiB limit".into());
    }
    Ok(output)
}

pub fn parse_snapshot(bytes: &[u8]) -> Result<MonitoringSnapshot, String> {
    if bytes.len() > MAX_MONITOR_JSON_BYTES {
        return Err("Monitoring snapshot exceeds the 16 MiB limit".into());
    }
    let snapshot: MonitoringSnapshot = serde_json::from_slice(bytes)
        .map_err(|error| format!("Invalid monitoring snapshot: {error}"))?;
    if snapshot.schema != MONITOR_SCHEMA {
        return Err(format!(
            "Unsupported monitoring snapshot schema {}",
            snapshot.schema
        ));
    }
    snapshot_json(&snapshot.service_events, &snapshot.drift_events)?;
    Ok(snapshot)
}

pub fn parse_runtime_output(output: &str) -> (Vec<ServiceEvent>, Vec<DriftEvent>) {
    let mut services = Vec::new();
    let mut drift = Vec::new();
    for line in output.lines() {
        if let Some(json) = line.strip_prefix("forge_service:") {
            if let Ok(event) = serde_json::from_str(json.trim()) {
                record_service(&mut services, event);
            }
        }
        if let Some(json) = line.strip_prefix("forge_drift:") {
            if let Ok(event) = serde_json::from_str(json.trim()) {
                record_drift(&mut drift, event);
            }
        }
    }
    (services, drift)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_service_and_drift_telemetry() {
        let output = "forge_service:{\"model\":\"iris\",\"version\":\"1\",\"requests\":9,\"errors\":1,\"p95_ms\":4.5}\nforge_drift:{\"model\":\"iris\",\"version\":\"1\",\"feature\":\"width\",\"score\":0.3,\"threshold\":0.2}";
        let (service, drift) = parse_runtime_output(output);
        assert_eq!(service[0].requests, 9);
        assert_eq!(drift[0].feature, "width");
        let encoded = snapshot_json(&service, &drift).unwrap();
        let restored = parse_snapshot(&encoded).unwrap();
        assert_eq!(restored.service_events, service);
        assert_eq!(restored.drift_events, drift);
    }

    #[test]
    fn rejects_invalid_monitoring_and_bounds_retention() {
        let mut events = Vec::new();
        for requests in 0..=MAX_MONITOR_EVENTS as u64 {
            record_service(
                &mut events,
                ServiceEvent {
                    model: "model".into(),
                    version: "1".into(),
                    requests,
                    errors: 0,
                    p95_ms: Some(2.0),
                },
            );
        }
        assert_eq!(events.len(), MAX_MONITOR_EVENTS);
        assert_eq!(events[0].requests, 1);
        record_service(
            &mut events,
            ServiceEvent {
                model: "bad".into(),
                version: "1".into(),
                requests: 1,
                errors: 2,
                p95_ms: None,
            },
        );
        assert_eq!(events.len(), MAX_MONITOR_EVENTS);
        assert!(parse_snapshot(&vec![b' '; MAX_MONITOR_JSON_BYTES + 1]).is_err());
    }
}
