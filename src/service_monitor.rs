use serde::{Deserialize, Serialize};

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

pub fn parse_runtime_output(output: &str) -> (Vec<ServiceEvent>, Vec<DriftEvent>) {
    let mut services = Vec::new();
    let mut drift = Vec::new();
    for line in output.lines() {
        if let Some(json) = line.strip_prefix("forge_service:") {
            if let Ok(event) = serde_json::from_str(json.trim()) {
                services.push(event);
            }
        }
        if let Some(json) = line.strip_prefix("forge_drift:") {
            if let Ok(event) = serde_json::from_str(json.trim()) {
                drift.push(event);
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
    }
}
