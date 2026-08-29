use crate::plot::{PlotKind, PlotSeries, PlotSpec, PLOT_SPEC_VERSION};
use serde::{Deserialize, Serialize};

pub const MAX_MONITOR_EVENTS: usize = 10_000;
const MAX_MONITOR_TEXT_BYTES: usize = 256;
const MAX_MONITOR_JSON_BYTES: usize = 16 * 1024 * 1024;
const MONITOR_SCHEMA: u16 = 1;
const MAX_REPORT_EVENTS_PER_STREAM: usize = 500;
const MAX_OVERVIEW_ROWS: usize = 128;

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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DriftEvent {
    pub model: String,
    pub version: String,
    pub feature: String,
    pub score: f64,
    #[serde(default)]
    pub threshold: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standardized_mean_shift: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MonitoringSnapshot {
    pub schema: u16,
    pub service_events: Vec<ServiceEvent>,
    pub drift_events: Vec<DriftEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeploymentHealth {
    pub model: String,
    pub version: String,
    pub requests: Option<u64>,
    pub errors: Option<u64>,
    pub error_rate: Option<f64>,
    pub p95_ms: Option<f64>,
    pub drift_features: usize,
    pub drift_breaches: usize,
    pub latest_drift_feature: Option<String>,
    pub drift_observed: Option<usize>,
    pub drift_mean_shift: Option<f64>,
    pub drift_scale_ratio: Option<f64>,
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
        && event.observed.is_none_or(|value| value > 0)
        && event
            .standardized_mean_shift
            .is_none_or(|value| value.is_finite() && value >= 0.0)
        && event
            .scale_ratio
            .is_none_or(|value| value.is_finite() && value >= 0.0)
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

pub fn monitoring_csv(
    service_events: &[ServiceEvent],
    drift_events: &[DriftEvent],
) -> Result<Vec<u8>, String> {
    if service_events.is_empty() && drift_events.is_empty() {
        return Err("No deployment monitoring events are available for CSV export".into());
    }
    if service_events.len() > MAX_MONITOR_EVENTS
        || drift_events.len() > MAX_MONITOR_EVENTS
        || service_events.iter().any(|event| !valid_service(event))
        || drift_events.iter().any(|event| !valid_drift(event))
    {
        return Err("Deployment monitoring CSV contains invalid or excessive events".into());
    }
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer
        .write_record([
            "stream",
            "index",
            "model",
            "version",
            "requests",
            "errors",
            "p95_ms",
            "feature",
            "score",
            "threshold",
            "observed",
            "standardized_mean_shift",
            "scale_ratio",
            "status",
        ])
        .map_err(|error| error.to_string())?;
    for (index, event) in service_events.iter().enumerate() {
        writer
            .write_record([
                "service".into(),
                (index + 1).to_string(),
                event.model.clone(),
                event.version.clone(),
                event.requests.to_string(),
                event.errors.to_string(),
                event
                    .p95_ms
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ])
            .map_err(|error| error.to_string())?;
    }
    for (index, event) in drift_events.iter().enumerate() {
        writer
            .write_record([
                "drift".into(),
                (index + 1).to_string(),
                event.model.clone(),
                event.version.clone(),
                String::new(),
                String::new(),
                String::new(),
                event.feature.clone(),
                event.score.to_string(),
                event.threshold.to_string(),
                event
                    .observed
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                event
                    .standardized_mean_shift
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                event
                    .scale_ratio
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                if event.score > event.threshold {
                    "breach".into()
                } else {
                    "ok".into()
                },
            ])
            .map_err(|error| error.to_string())?;
    }
    let output = writer.into_inner().map_err(|error| error.to_string())?;
    if output.len() > MAX_MONITOR_JSON_BYTES {
        return Err("Deployment monitoring CSV exceeds the 16 MiB export limit".into());
    }
    Ok(output)
}

pub fn monitoring_plots(
    service_events: &[ServiceEvent],
    drift_events: &[DriftEvent],
) -> Vec<PlotSpec> {
    let mut requests = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut error_rates = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut latency = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    for (index, event) in service_events.iter().enumerate() {
        let name = format!("{} {}", event.model, event.version);
        requests
            .entry(name.clone())
            .or_default()
            .push([index as f64, event.requests as f64]);
        let rate = if event.requests == 0 {
            0.0
        } else {
            event.errors as f64 * 100.0 / event.requests as f64
        };
        error_rates
            .entry(name.clone())
            .or_default()
            .push([index as f64, rate]);
        if let Some(p95) = event.p95_ms {
            latency.entry(name).or_default().push([index as f64, p95]);
        }
    }
    let mut drift = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut thresholds = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut mean_shift = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut mean_thresholds = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut scale_ratio = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut scale_thresholds = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    for (index, event) in drift_events.iter().enumerate() {
        let name = format!("{} {} · {}", event.model, event.version, event.feature);
        drift
            .entry(name.clone())
            .or_default()
            .push([index as f64, event.score]);
        thresholds
            .entry(format!("{name} threshold"))
            .or_default()
            .push([index as f64, event.threshold]);
        if let Some(value) = event.standardized_mean_shift {
            mean_shift
                .entry(name.clone())
                .or_default()
                .push([index as f64, value]);
            mean_thresholds
                .entry(format!("{name} 1σ boundary"))
                .or_default()
                .push([index as f64, 1.0]);
        }
        if let Some(value) = event.scale_ratio {
            scale_ratio
                .entry(name.clone())
                .or_default()
                .push([index as f64, value]);
            scale_thresholds
                .entry(format!("{name} lower boundary"))
                .or_default()
                .push([index as f64, 0.5]);
            scale_thresholds
                .entry(format!("{name} upper boundary"))
                .or_default()
                .push([index as f64, 2.0]);
        }
    }
    let mut plots = Vec::new();
    push_plot(
        &mut plots,
        "Service requests",
        "event",
        "requests",
        requests,
    );
    push_plot(
        &mut plots,
        "Service error rate",
        "event",
        "errors (%)",
        error_rates,
    );
    push_plot(&mut plots, "Service p95 latency", "event", "ms", latency);
    drift.extend(thresholds);
    push_plot(&mut plots, "Feature drift", "event", "score", drift);
    mean_shift.extend(mean_thresholds);
    push_plot(
        &mut plots,
        "Feature mean shift",
        "event",
        "standard deviations",
        mean_shift,
    );
    scale_ratio.extend(scale_thresholds);
    push_plot(
        &mut plots,
        "Feature scale ratio",
        "event",
        "inference / training scale",
        scale_ratio,
    );
    plots
}

pub fn deployment_overview(
    service_events: &[ServiceEvent],
    drift_events: &[DriftEvent],
) -> Vec<DeploymentHealth> {
    let mut services = std::collections::BTreeMap::new();
    for event in service_events.iter().filter(|event| valid_service(event)) {
        services.insert((event.model.clone(), event.version.clone()), event);
    }
    let mut latest_drift = std::collections::BTreeMap::new();
    let mut latest_model_drift = std::collections::BTreeMap::new();
    for event in drift_events.iter().filter(|event| valid_drift(event)) {
        latest_model_drift.insert((event.model.clone(), event.version.clone()), event);
        latest_drift.insert(
            (
                event.model.clone(),
                event.version.clone(),
                event.feature.clone(),
            ),
            event,
        );
    }
    let mut drift_counts = std::collections::BTreeMap::<(String, String), (usize, usize)>::new();
    for ((model, version, _), event) in latest_drift {
        let counts = drift_counts.entry((model, version)).or_default();
        counts.0 += 1;
        counts.1 += usize::from(event.score > event.threshold);
    }
    let keys = services
        .keys()
        .chain(drift_counts.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    keys.into_iter()
        .take(MAX_OVERVIEW_ROWS)
        .map(|(model, version)| {
            let service = services.get(&(model.clone(), version.clone())).copied();
            let (drift_features, drift_breaches) = drift_counts
                .get(&(model.clone(), version.clone()))
                .copied()
                .unwrap_or_default();
            let drift = latest_model_drift.get(&(model.clone(), version.clone()));
            DeploymentHealth {
                model,
                version,
                requests: service.map(|event| event.requests),
                errors: service.map(|event| event.errors),
                error_rate: service.map(|event| {
                    if event.requests == 0 {
                        0.0
                    } else {
                        event.errors as f64 * 100.0 / event.requests as f64
                    }
                }),
                p95_ms: service.and_then(|event| event.p95_ms),
                drift_features,
                drift_breaches,
                latest_drift_feature: drift.map(|event| event.feature.clone()),
                drift_observed: drift.and_then(|event| event.observed),
                drift_mean_shift: drift.and_then(|event| event.standardized_mean_shift),
                drift_scale_ratio: drift.and_then(|event| event.scale_ratio),
            }
        })
        .collect()
}

pub fn monitoring_report(
    service_events: &[ServiceEvent],
    drift_events: &[DriftEvent],
) -> Result<String, String> {
    if service_events.is_empty() && drift_events.is_empty() {
        return Err("No deployment monitoring events are available for a report".into());
    }
    if service_events.iter().any(|event| !valid_service(event))
        || drift_events.iter().any(|event| !valid_drift(event))
    {
        return Err("Deployment monitoring report contains invalid events".into());
    }
    let latest = service_events.last();
    let error_rate = latest.map_or(0.0, |event| {
        if event.requests == 0 {
            0.0
        } else {
            event.errors as f64 * 100.0 / event.requests as f64
        }
    });
    let breaches = drift_events
        .iter()
        .filter(|event| event.score > event.threshold)
        .count();
    let service_rows = service_events
        .iter()
        .enumerate()
        .rev()
        .take(MAX_REPORT_EVENTS_PER_STREAM)
        .map(|(index, event)| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                index + 1,
                html_escape(&event.model),
                html_escape(&event.version),
                event.requests,
                event.errors,
                event
                    .p95_ms
                    .map_or_else(|| "—".into(), |value| format!("{value:.3}"))
            )
        })
        .collect::<String>();
    let drift_rows = drift_events
        .iter()
        .enumerate()
        .rev()
        .take(MAX_REPORT_EVENTS_PER_STREAM)
        .map(|(index, event)| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:.6}</td><td>{:.6}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                index + 1,
                html_escape(&event.model),
                html_escape(&event.version),
                html_escape(&event.feature),
                event.score,
                event.threshold,
                event.observed.map_or_else(|| "—".into(), |value| value.to_string()),
                event.standardized_mean_shift.map_or_else(|| "—".into(), |value| format!("{value:.6}")),
                event.scale_ratio.map_or_else(|| "—".into(), |value| format!("{value:.6}")),
                if event.score > event.threshold { "breach" } else { "ok" }
            )
        })
        .collect::<String>();
    Ok(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><title>Forge ML deployment monitoring report</title><style>body{{font:14px system-ui,sans-serif;max-width:1100px;margin:32px auto;padding:0 16px;color:#20242b}}.cards{{display:flex;gap:10px;flex-wrap:wrap}}.card{{border:1px solid #ccd2da;border-radius:6px;padding:10px;min-width:130px}}table{{border-collapse:collapse;width:100%;margin:12px 0 24px}}th,td{{border:1px solid #ccd2da;padding:6px;text-align:left}}</style></head><body><h1>Deployment monitoring report</h1><div class=\"cards\"><div class=\"card\"><strong>Service events</strong><br>{}</div><div class=\"card\"><strong>Drift events</strong><br>{}</div><div class=\"card\"><strong>Latest requests</strong><br>{}</div><div class=\"card\"><strong>Latest error rate</strong><br>{error_rate:.3}%</div><div class=\"card\"><strong>Latest p95</strong><br>{}</div><div class=\"card\"><strong>Drift breaches</strong><br>{breaches}</div></div><h2>Recent service health</h2><p>Newest {} events shown.</p><table><tr><th>#</th><th>Model</th><th>Version</th><th>Requests</th><th>Errors</th><th>p95 ms</th></tr>{service_rows}</table><h2>Recent feature drift</h2><p>Newest {} events shown.</p><table><tr><th>#</th><th>Model</th><th>Version</th><th>Feature</th><th>Score</th><th>Threshold</th><th>Observed</th><th>Mean shift (σ)</th><th>Scale ratio</th><th>Status</th></tr>{drift_rows}</table></body></html>",
        service_events.len(),
        drift_events.len(),
        latest.map_or(0, |event| event.requests),
        latest.and_then(|event| event.p95_ms).map_or_else(|| "—".into(), |value| format!("{value:.3} ms")),
        service_events.len().min(MAX_REPORT_EVENTS_PER_STREAM),
        drift_events.len().min(MAX_REPORT_EVENTS_PER_STREAM),
    ))
}

pub fn monitoring_pdf_lines(
    service_events: &[ServiceEvent],
    drift_events: &[DriftEvent],
) -> Result<Vec<String>, String> {
    if service_events.is_empty() && drift_events.is_empty() {
        return Err("No deployment monitoring events are available for a report".into());
    }
    if service_events.iter().any(|event| !valid_service(event))
        || drift_events.iter().any(|event| !valid_drift(event))
    {
        return Err("Deployment monitoring report contains invalid events".into());
    }
    let latest = service_events.last();
    let error_rate = latest.map_or(0.0, |event| {
        if event.requests == 0 {
            0.0
        } else {
            event.errors as f64 * 100.0 / event.requests as f64
        }
    });
    let breaches = drift_events
        .iter()
        .filter(|event| event.score > event.threshold)
        .count();
    let mut lines = vec![
        "Forge ML deployment monitoring report".into(),
        format!("Service events: {}", service_events.len()),
        format!("Drift events: {}", drift_events.len()),
        format!(
            "Latest requests: {}",
            latest.map_or(0, |event| event.requests)
        ),
        format!("Latest error rate: {error_rate:.3}%"),
        format!(
            "Latest p95: {}",
            latest
                .and_then(|event| event.p95_ms)
                .map_or_else(|| "-".into(), |value| format!("{value:.3} ms"))
        ),
        format!("Drift breaches: {breaches}"),
        String::new(),
        format!(
            "Recent service health (newest {} of {})",
            service_events.len().min(MAX_REPORT_EVENTS_PER_STREAM),
            service_events.len()
        ),
    ];
    lines.extend(
        service_events
            .iter()
            .enumerate()
            .rev()
            .take(MAX_REPORT_EVENTS_PER_STREAM)
            .map(|(index, event)| {
                format!(
                    "#{} | {} {} | requests {} | errors {} | p95 {}",
                    index + 1,
                    event.model,
                    event.version,
                    event.requests,
                    event.errors,
                    event
                        .p95_ms
                        .map_or_else(|| "-".into(), |value| format!("{value:.3} ms"))
                )
            }),
    );
    lines.extend([
        String::new(),
        format!(
            "Recent feature drift (newest {} of {})",
            drift_events.len().min(MAX_REPORT_EVENTS_PER_STREAM),
            drift_events.len()
        ),
    ]);
    lines.extend(
        drift_events
            .iter()
            .enumerate()
            .rev()
            .take(MAX_REPORT_EVENTS_PER_STREAM)
            .map(|(index, event)| {
                format!(
                    "#{} | {} {} | {} | score {:.6} | threshold {:.6} | observed {} | mean shift {} | scale ratio {} | {}",
                    index + 1,
                    event.model,
                    event.version,
                    event.feature,
                    event.score,
                    event.threshold,
                    event.observed.map_or_else(|| "-".into(), |value| value.to_string()),
                    event.standardized_mean_shift.map_or_else(|| "-".into(), |value| format!("{value:.6}")),
                    event.scale_ratio.map_or_else(|| "-".into(), |value| format!("{value:.6}")),
                    if event.score > event.threshold {
                        "breach"
                    } else {
                        "ok"
                    }
                )
            }),
    );
    Ok(lines)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn push_plot(
    plots: &mut Vec<PlotSpec>,
    name: &str,
    x_label: &str,
    y_label: &str,
    series: std::collections::BTreeMap<String, Vec<[f64; 2]>>,
) {
    if series.is_empty() {
        return;
    }
    plots.push(PlotSpec {
        version: PLOT_SPEC_VERSION,
        name: name.into(),
        kind: PlotKind::Line,
        x_label: x_label.into(),
        y_label: y_label.into(),
        series: series
            .into_iter()
            .take(128)
            .map(|(name, points)| PlotSeries {
                name,
                points,
                values: Vec::new(),
                visible: true,
            })
            .collect(),
        matrix: Vec::new(),
        x_log: false,
        y_log: false,
    });
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
        let output = "forge_service:{\"model\":\"iris\",\"version\":\"1\",\"requests\":9,\"errors\":1,\"p95_ms\":4.5}\nforge_drift:{\"model\":\"iris\",\"version\":\"1\",\"feature\":\"width\",\"score\":0.3,\"threshold\":0.2}\nforge_drift:{\"model\":\"iris\",\"version\":\"1\",\"feature\":\"length\",\"score\":1.25,\"threshold\":1.0,\"observed\":42,\"standardized_mean_shift\":1.25,\"scale_ratio\":2.5}";
        let (service, drift) = parse_runtime_output(output);
        assert_eq!(service[0].requests, 9);
        assert_eq!(drift[0].feature, "width");
        assert_eq!(drift[0].observed, None);
        assert_eq!(drift[1].observed, Some(42));
        assert_eq!(drift[1].standardized_mean_shift, Some(1.25));
        assert_eq!(drift[1].scale_ratio, Some(2.5));
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
        assert!(!valid_drift(&DriftEvent {
            model: "model".into(),
            version: "1".into(),
            feature: "feature".into(),
            score: 1.0,
            threshold: 1.0,
            observed: Some(0),
            standardized_mean_shift: Some(f64::NAN),
            scale_ratio: Some(1.0),
        }));
    }

    #[test]
    fn monitoring_events_become_valid_native_plots() {
        let services = vec![ServiceEvent {
            model: "iris".into(),
            version: "1".into(),
            requests: 100,
            errors: 2,
            p95_ms: Some(4.0),
        }];
        let drift = vec![DriftEvent {
            model: "iris".into(),
            version: "1".into(),
            feature: "width".into(),
            score: 0.3,
            threshold: 0.2,
            ..Default::default()
        }];
        let plots = monitoring_plots(&services, &drift);
        assert_eq!(plots.len(), 4);
        assert!(plots.iter().all(|plot| plot.validate().is_ok()));
        assert_eq!(plots[1].series[0].points[0][1], 2.0);
        assert_eq!(plots[3].series.len(), 2);

        let enriched = vec![DriftEvent {
            observed: Some(100),
            standardized_mean_shift: Some(1.25),
            scale_ratio: Some(2.5),
            ..drift[0].clone()
        }];
        let plots = monitoring_plots(&services, &enriched);
        assert_eq!(plots.len(), 6);
        assert!(plots.iter().all(|plot| plot.validate().is_ok()));
        assert_eq!(plots[4].name, "Feature mean shift");
        assert_eq!(plots[4].series.len(), 2);
        assert_eq!(plots[5].name, "Feature scale ratio");
        assert_eq!(plots[5].series.len(), 3);
    }

    #[test]
    fn deployment_overview_uses_latest_deterministic_model_health() {
        let services = vec![
            ServiceEvent {
                model: "iris".into(),
                version: "1".into(),
                requests: 10,
                errors: 2,
                p95_ms: None,
            },
            ServiceEvent {
                model: "iris".into(),
                version: "1".into(),
                requests: 100,
                errors: 5,
                p95_ms: Some(4.0),
            },
        ];
        let drift = vec![
            DriftEvent {
                model: "iris".into(),
                version: "1".into(),
                feature: "width".into(),
                score: 0.4,
                threshold: 0.2,
                ..Default::default()
            },
            DriftEvent {
                model: "iris".into(),
                version: "1".into(),
                feature: "width".into(),
                score: 0.1,
                threshold: 0.2,
                observed: Some(75),
                standardized_mean_shift: Some(0.4),
                scale_ratio: Some(1.2),
            },
            DriftEvent {
                model: "drift-only".into(),
                version: "2".into(),
                feature: "age".into(),
                score: 0.3,
                threshold: 0.2,
                ..Default::default()
            },
        ];
        let overview = deployment_overview(&services, &drift);
        assert_eq!(overview.len(), 2);
        assert_eq!(overview[0].model, "drift-only");
        assert_eq!(overview[0].requests, None);
        assert_eq!(overview[0].drift_breaches, 1);
        assert_eq!(overview[1].requests, Some(100));
        assert_eq!(overview[1].error_rate, Some(5.0));
        assert_eq!(overview[1].drift_features, 1);
        assert_eq!(overview[1].drift_breaches, 0);
        assert_eq!(overview[1].latest_drift_feature.as_deref(), Some("width"));
        assert_eq!(overview[1].drift_observed, Some(75));
        assert_eq!(overview[1].drift_mean_shift, Some(0.4));
        assert_eq!(overview[1].drift_scale_ratio, Some(1.2));

        let fleet = (0..=MAX_OVERVIEW_ROWS)
            .map(|index| ServiceEvent {
                model: format!("model-{index:03}"),
                version: "1".into(),
                requests: 1,
                errors: 0,
                p95_ms: None,
            })
            .collect::<Vec<_>>();
        assert_eq!(deployment_overview(&fleet, &[]).len(), MAX_OVERVIEW_ROWS);
    }

    #[test]
    fn monitoring_csv_is_structured_escaped_and_validated() {
        let services = vec![ServiceEvent {
            model: "iris, \"wide\"".into(),
            version: "1".into(),
            requests: 100,
            errors: 2,
            p95_ms: Some(4.0),
        }];
        let drift = vec![DriftEvent {
            model: "iris".into(),
            version: "1".into(),
            feature: "petal\nwidth".into(),
            score: 0.3,
            threshold: 0.2,
            observed: Some(42),
            standardized_mean_shift: Some(1.25),
            scale_ratio: Some(2.5),
        }];
        let output = monitoring_csv(&services, &drift).unwrap();
        let mut reader = csv::Reader::from_reader(output.as_slice());
        assert_eq!(reader.headers().unwrap().len(), 14);
        let rows = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(&rows[0][2], "iris, \"wide\"");
        assert_eq!(&rows[1][7], "petal\nwidth");
        assert_eq!(&rows[1][10], "42");
        assert_eq!(&rows[1][11], "1.25");
        assert_eq!(&rows[1][12], "2.5");
        assert_eq!(&rows[1][13], "breach");
        assert!(monitoring_csv(&[], &[]).is_err());
    }

    #[test]
    fn monitoring_report_is_bounded_offline_and_escaped() {
        let mut services = vec![ServiceEvent {
            model: "<model>".into(),
            version: "1".into(),
            requests: 100,
            errors: 5,
            p95_ms: Some(4.5),
        }];
        services.extend((0..MAX_REPORT_EVENTS_PER_STREAM).map(|_| ServiceEvent {
            model: "model".into(),
            version: "1".into(),
            requests: 100,
            errors: 5,
            p95_ms: Some(4.5),
        }));
        let drift = vec![DriftEvent {
            model: "model".into(),
            version: "1".into(),
            feature: "<script>".into(),
            score: 0.3,
            threshold: 0.2,
            observed: Some(42),
            standardized_mean_shift: Some(1.25),
            scale_ratio: Some(2.5),
        }];
        let report = monitoring_report(&services, &drift).unwrap();
        assert!(report.contains("5.000%"));
        assert!(report.contains("Drift breaches</strong><br>1"));
        assert!(report.contains("<td>42</td><td>1.250000</td><td>2.500000</td>"));
        assert!(report.contains("&lt;script&gt;"));
        assert!(report.contains("Newest 500 events shown."));
        assert!(!report.contains("&lt;model&gt;"));
        assert!(!report.contains("<script>"));
        assert!(!report.contains("https://"));
    }

    #[test]
    fn monitoring_pdf_lines_are_bounded_and_summarized() {
        let services = (0..=MAX_REPORT_EVENTS_PER_STREAM)
            .map(|index| ServiceEvent {
                model: format!("model-{index}"),
                version: "1".into(),
                requests: 100,
                errors: 5,
                p95_ms: Some(4.5),
            })
            .collect::<Vec<_>>();
        let drift = vec![DriftEvent {
            model: "model".into(),
            version: "1".into(),
            feature: "width".into(),
            score: 0.3,
            threshold: 0.2,
            observed: Some(42),
            standardized_mean_shift: Some(1.25),
            scale_ratio: Some(2.5),
        }];
        let lines = monitoring_pdf_lines(&services, &drift).unwrap();
        assert!(lines.iter().any(|line| line == "Latest error rate: 5.000%"));
        assert!(lines.iter().any(|line| line == "Drift breaches: 1"));
        assert!(lines
            .iter()
            .any(|line| line.contains("observed 42 | mean shift 1.250000 | scale ratio 2.500000")));
        assert!(lines.iter().any(|line| line.contains("newest 500 of 501")));
        assert!(!lines.iter().any(|line| line.contains("model-0 ")));
        assert!(monitoring_pdf_lines(&[], &[]).is_err());
    }
}
