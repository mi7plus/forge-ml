use crate::plot::{PlotKind, PlotSeries, PlotSpec, PLOT_SPEC_VERSION};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

pub const MAX_TRAINING_EVENTS: usize = 10_000;
const MAX_TRAINING_TEXT_BYTES: usize = 64 * 1024;
const MAX_TRAINING_IMPORT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TRAINING_EXPORT_BYTES: usize = 64 * 1024 * 1024;
const MAX_REPORT_EVENTS: usize = 1_000;
const MAX_REPORT_EVENT_CHARS: usize = 8_192;
const MAX_PDF_REPORT_EVENTS: usize = 500;
const MAX_PDF_EVENT_CHARS: usize = 512;
const MAX_RUN_OVERVIEW_ROWS: usize = 128;
const MAX_RUN_ID_BYTES: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TrainingEvent {
    RunContext {
        run_id: String,
    },
    Started {
        job: String,
        total_trials: usize,
    },
    TrialStarted {
        trial: usize,
        parameters: String,
    },
    FoldCompleted {
        trial: usize,
        fold: usize,
        folds: usize,
        score: f64,
    },
    TrialCompleted {
        trial: usize,
        score: f64,
    },
    Epoch {
        epoch: usize,
        total: usize,
        loss: f64,
        metric: Option<f64>,
    },
    Batch {
        epoch: usize,
        batch: usize,
        total: usize,
        loss: f64,
        samples_per_second: f64,
    },
    Checkpoint {
        path: String,
        epoch: usize,
    },
    EarlyStopping {
        epoch: usize,
        best_epoch: usize,
        best_score: f64,
    },
    Completed {
        best_score: f64,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrainingRunSummary {
    pub run_id: Option<String>,
    pub job: String,
    pub status: String,
    pub total_trials: usize,
    pub completed_trials: usize,
    pub epoch: Option<usize>,
    pub total_epochs: Option<usize>,
    pub latest_loss: Option<f64>,
    pub latest_metric: Option<f64>,
    pub best_score: Option<f64>,
}

impl TrainingRunSummary {
    fn unscoped() -> Self {
        Self {
            run_id: None,
            job: "Unscoped telemetry".into(),
            status: "Running".into(),
            total_trials: 0,
            completed_trials: 0,
            epoch: None,
            total_epochs: None,
            latest_loss: None,
            latest_metric: None,
            best_score: None,
        }
    }
}

pub fn validate_training_event(event: &TrainingEvent) -> bool {
    let text_ok = |value: &str| value.len() <= MAX_TRAINING_TEXT_BYTES && !value.contains('\0');
    match event {
        TrainingEvent::RunContext { run_id } => {
            !run_id.trim().is_empty()
                && run_id.len() <= MAX_RUN_ID_BYTES
                && !run_id.contains(['\0', ']'])
        }
        TrainingEvent::Started { job, .. } => text_ok(job),
        TrainingEvent::TrialStarted { parameters, .. } => text_ok(parameters),
        TrainingEvent::FoldCompleted { score, .. }
        | TrainingEvent::TrialCompleted { score, .. }
        | TrainingEvent::Completed { best_score: score } => score.is_finite(),
        TrainingEvent::Epoch { loss, metric, .. } => {
            loss.is_finite() && metric.is_none_or(f64::is_finite)
        }
        TrainingEvent::Batch {
            loss,
            samples_per_second,
            ..
        } => loss.is_finite() && samples_per_second.is_finite() && *samples_per_second >= 0.0,
        TrainingEvent::Checkpoint { path, .. } => text_ok(path),
        TrainingEvent::EarlyStopping { best_score, .. } => best_score.is_finite(),
        TrainingEvent::Failed { message } => text_ok(message),
    }
}

pub fn record_training_event(events: &mut Vec<TrainingEvent>, event: TrainingEvent) {
    if !validate_training_event(&event) {
        return;
    }
    if events.len() >= MAX_TRAINING_EVENTS {
        let remove = events.len() - MAX_TRAINING_EVENTS + 1;
        events.drain(..remove);
    }
    events.push(event);
}

pub fn training_run_overview(events: &[TrainingEvent]) -> Vec<TrainingRunSummary> {
    let mut runs = Vec::<(Option<String>, TrainingRunSummary, usize)>::new();
    let mut pending_context = None;
    let mut activity = 0usize;
    for event in events.iter().filter(|event| validate_training_event(event)) {
        if let TrainingEvent::RunContext { run_id } = event {
            pending_context = Some(run_id.clone());
            continue;
        }
        activity += 1;
        let context = pending_context.take();
        if let TrainingEvent::Started { job, total_trials } = event {
            let summary = TrainingRunSummary {
                run_id: context.clone(),
                job: job.clone(),
                status: "Running".into(),
                total_trials: *total_trials,
                ..TrainingRunSummary::unscoped()
            };
            if let Some(index) = context.as_ref().and_then(|run_id| {
                runs.iter()
                    .position(|(key, _, _)| key.as_ref() == Some(run_id))
            }) {
                runs[index] = (context, summary, activity);
            } else {
                runs.push((context, summary, activity));
            }
            continue;
        }
        let index = context
            .as_ref()
            .and_then(|run_id| {
                runs.iter()
                    .position(|(key, _, _)| key.as_ref() == Some(run_id))
            })
            .or_else(|| {
                context
                    .is_none()
                    .then(|| runs.iter().rposition(|(key, _, _)| key.is_none()))
                    .flatten()
            })
            .unwrap_or_else(|| {
                let mut summary = TrainingRunSummary::unscoped();
                if let Some(run_id) = &context {
                    summary.run_id = Some(run_id.clone());
                    summary.job = format!("Run {run_id}");
                }
                runs.push((context.clone(), summary, activity));
                runs.len() - 1
            });
        apply_run_event(&mut runs[index].1, event);
        runs[index].2 = activity;
    }
    runs.sort_by_key(|(_, _, last_activity)| std::cmp::Reverse(*last_activity));
    runs.into_iter()
        .take(MAX_RUN_OVERVIEW_ROWS)
        .map(|(_, summary, _)| summary)
        .collect()
}

fn apply_run_event(run: &mut TrainingRunSummary, event: &TrainingEvent) {
    match event {
        TrainingEvent::TrialCompleted { score, .. } => {
            run.completed_trials += 1;
            run.best_score = Some(run.best_score.map_or(*score, |best| best.max(*score)));
        }
        TrainingEvent::Epoch {
            epoch,
            total,
            loss,
            metric,
        } => {
            run.epoch = Some(*epoch);
            run.total_epochs = Some(*total);
            run.latest_loss = Some(*loss);
            if metric.is_some() {
                run.latest_metric = *metric;
            }
        }
        TrainingEvent::Batch { loss, .. } => run.latest_loss = Some(*loss),
        TrainingEvent::EarlyStopping { best_score, .. } => {
            run.status = "Early stopped".into();
            run.best_score = Some(*best_score);
        }
        TrainingEvent::Completed { best_score } => {
            run.status = "Completed".into();
            run.best_score = Some(*best_score);
        }
        TrainingEvent::Failed { .. } => run.status = "Failed".into(),
        TrainingEvent::RunContext { .. }
        | TrainingEvent::Started { .. }
        | TrainingEvent::TrialStarted { .. }
        | TrainingEvent::FoldCompleted { .. }
        | TrainingEvent::Checkpoint { .. } => {}
    }
}

pub fn training_run_csv(events: &[TrainingEvent]) -> Result<Vec<u8>, String> {
    if events.is_empty() {
        return Err("No training events are available for a run-summary export".into());
    }
    if events.len() > MAX_TRAINING_EVENTS
        || events.iter().any(|event| !validate_training_event(event))
    {
        return Err("Training run summary contains invalid or excessive events".into());
    }
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer
        .write_record([
            "newest_index",
            "run_id",
            "job",
            "status",
            "completed_trials",
            "total_trials",
            "epoch",
            "total_epochs",
            "latest_loss",
            "latest_metric",
            "best_score",
        ])
        .map_err(|error| error.to_string())?;
    for (index, run) in training_run_overview(events).into_iter().enumerate() {
        writer
            .write_record([
                (index + 1).to_string(),
                run.run_id.unwrap_or_default(),
                run.job,
                run.status,
                run.completed_trials.to_string(),
                run.total_trials.to_string(),
                run.epoch.map(|value| value.to_string()).unwrap_or_default(),
                run.total_epochs
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                run.latest_loss
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                run.latest_metric
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                run.best_score
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ])
            .map_err(|error| error.to_string())?;
    }
    let output = writer.into_inner().map_err(|error| error.to_string())?;
    if output.len() > MAX_TRAINING_IMPORT_BYTES {
        return Err("Training run-summary CSV exceeds the 16 MiB export limit".into());
    }
    Ok(output)
}

pub fn training_json(events: &[TrainingEvent]) -> Result<Vec<u8>, String> {
    let output = serde_json::to_vec_pretty(events).map_err(|error| error.to_string())?;
    if output.len() > MAX_TRAINING_EXPORT_BYTES {
        return Err("Training JSON exceeds the 64 MiB export limit".into());
    }
    Ok(output)
}

pub fn training_csv(events: &[TrainingEvent]) -> Result<Vec<u8>, String> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer
        .write_record(["index", "event"])
        .map_err(|error| error.to_string())?;
    for (index, event) in events.iter().enumerate() {
        writer
            .write_record([
                index.to_string(),
                serde_json::to_string(event).map_err(|error| error.to_string())?,
            ])
            .map_err(|error| error.to_string())?;
    }
    let output = writer.into_inner().map_err(|error| error.to_string())?;
    if output.len() > MAX_TRAINING_EXPORT_BYTES {
        return Err("Training CSV exceeds the 64 MiB export limit".into());
    }
    Ok(output)
}

pub fn parse_training_json(bytes: &[u8]) -> Result<Vec<TrainingEvent>, String> {
    if bytes.len() > MAX_TRAINING_IMPORT_BYTES {
        return Err("Training JSON exceeds the 16 MiB import limit".into());
    }
    let events: Vec<TrainingEvent> =
        serde_json::from_slice(bytes).map_err(|error| format!("Invalid training JSON: {error}"))?;
    if events.len() > MAX_TRAINING_EVENTS {
        return Err(format!(
            "Training JSON exceeds the {MAX_TRAINING_EVENTS}-event limit"
        ));
    }
    if let Some(index) = events
        .iter()
        .position(|event| !validate_training_event(event))
    {
        return Err(format!(
            "Training event {} is invalid or oversized",
            index + 1
        ));
    }
    Ok(events)
}

pub fn training_report(events: &[TrainingEvent]) -> Result<String, String> {
    if events.is_empty() {
        return Err("No training events are available for a report".into());
    }
    let mut epochs = 0usize;
    let mut batches = 0usize;
    let mut trials = 0usize;
    let mut failures = 0usize;
    let mut latest_loss = None;
    let mut latest_metric = None;
    let mut best_score: Option<f64> = None;
    for event in events {
        match event {
            TrainingEvent::Epoch { loss, metric, .. } => {
                epochs += 1;
                latest_loss = Some(*loss);
                if metric.is_some() {
                    latest_metric = *metric;
                }
            }
            TrainingEvent::Batch { loss, .. } => {
                batches += 1;
                latest_loss = Some(*loss);
            }
            TrainingEvent::TrialCompleted { score, .. } => {
                trials += 1;
                best_score = Some(best_score.map_or(*score, |best| best.max(*score)));
            }
            TrainingEvent::Completed { best_score: score } => best_score = Some(*score),
            TrainingEvent::Failed { .. } => failures += 1,
            _ => {}
        }
    }
    let rows = events
        .iter()
        .enumerate()
        .rev()
        .take(MAX_REPORT_EVENTS)
        .map(|(index, event)| {
            let detail = serde_json::to_string(event)
                .unwrap_or_default()
                .chars()
                .take(MAX_REPORT_EVENT_CHARS)
                .collect::<String>();
            format!(
                "<tr><td>{}</td><td>{}</td><td><code>{}</code></td></tr>",
                index + 1,
                event_kind(event),
                html_escape(&detail)
            )
        })
        .collect::<String>();
    let runs = training_run_overview(events);
    let run_count = runs.len();
    let run_rows = runs
        .iter()
        .map(|run| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}/{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&run.job),
                html_escape(run.run_id.as_deref().unwrap_or("—")),
                html_escape(&run.status),
                run.completed_trials,
                run.total_trials,
                match (run.epoch, run.total_epochs) {
                    (Some(epoch), Some(total)) => format!("{epoch}/{total}"),
                    _ => "—".into(),
                },
                report_number(run.latest_loss),
                report_number(run.latest_metric),
                report_number(run.best_score),
            )
        })
        .collect::<String>();
    Ok(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><title>Forge ML training report</title><style>body{{font:14px system-ui,sans-serif;max-width:1100px;margin:32px auto;padding:0 16px;color:#20242b}}.cards{{display:flex;gap:10px;flex-wrap:wrap}}.card{{border:1px solid #ccd2da;border-radius:6px;padding:10px;min-width:120px}}table{{border-collapse:collapse;width:100%;margin-top:16px}}th,td{{border:1px solid #ccd2da;padding:6px;text-align:left;vertical-align:top}}code{{white-space:pre-wrap;word-break:break-word}}</style></head><body><h1>Forge ML training report</h1><p>{} retained events; the audit table shows the newest {}.</p><div class=\"cards\"><div class=\"card\"><strong>Epoch events</strong><br>{epochs}</div><div class=\"card\"><strong>Batch events</strong><br>{batches}</div><div class=\"card\"><strong>Completed trials</strong><br>{trials}</div><div class=\"card\"><strong>Failures</strong><br>{failures}</div><div class=\"card\"><strong>Latest loss</strong><br>{}</div><div class=\"card\"><strong>Latest metric</strong><br>{}</div><div class=\"card\"><strong>Best score</strong><br>{}</div></div><h2>Training runs</h2><p>Newest {run_count} runs shown.</p><table><thead><tr><th>Job</th><th>Run ID</th><th>Status</th><th>Trials</th><th>Epoch</th><th>Loss</th><th>Metric</th><th>Best</th></tr></thead><tbody>{run_rows}</tbody></table><h2>Recent event audit</h2><table><thead><tr><th>#</th><th>Type</th><th>Event</th></tr></thead><tbody>{rows}</tbody></table></body></html>",
        events.len(),
        events.len().min(MAX_REPORT_EVENTS),
        report_number(latest_loss),
        report_number(latest_metric),
        report_number(best_score),
    ))
}

pub fn training_pdf_lines(events: &[TrainingEvent]) -> Result<Vec<String>, String> {
    if events.is_empty() {
        return Err("No training events are available for a report".into());
    }
    let epochs = events
        .iter()
        .filter(|event| matches!(event, TrainingEvent::Epoch { .. }))
        .count();
    let batches = events
        .iter()
        .filter(|event| matches!(event, TrainingEvent::Batch { .. }))
        .count();
    let trials = events
        .iter()
        .filter(|event| matches!(event, TrainingEvent::TrialCompleted { .. }))
        .count();
    let failures = events
        .iter()
        .filter(|event| matches!(event, TrainingEvent::Failed { .. }))
        .count();
    let latest_loss = events.iter().rev().find_map(|event| match event {
        TrainingEvent::Epoch { loss, .. } | TrainingEvent::Batch { loss, .. } => Some(*loss),
        _ => None,
    });
    let latest_metric = events.iter().rev().find_map(|event| match event {
        TrainingEvent::Epoch {
            metric: Some(metric),
            ..
        } => Some(*metric),
        _ => None,
    });
    let best_score = events
        .iter()
        .rev()
        .find_map(|event| match event {
            TrainingEvent::Completed { best_score } => Some(*best_score),
            _ => None,
        })
        .or_else(|| {
            events
                .iter()
                .filter_map(|event| match event {
                    TrainingEvent::TrialCompleted { score, .. } => Some(*score),
                    _ => None,
                })
                .reduce(f64::max)
        });
    let mut lines = vec![
        "Forge ML training report".into(),
        format!("Retained events: {}", events.len()),
        format!("Epoch events: {epochs}"),
        format!("Batch events: {batches}"),
        format!("Completed trials: {trials}"),
        format!("Failures: {failures}"),
        format!("Latest loss: {}", report_number(latest_loss)),
        format!("Latest metric: {}", report_number(latest_metric)),
        format!("Best score: {}", report_number(best_score)),
        String::new(),
        format!(
            "Training runs (newest {})",
            training_run_overview(events).len()
        ),
    ];
    lines.extend(training_run_overview(events).into_iter().map(|run| {
        format!(
            "{} | run {} | {} | trials {}/{} | epoch {} | loss {} | metric {} | best {}",
            run.job,
            run.run_id.as_deref().unwrap_or("-"),
            run.status,
            run.completed_trials,
            run.total_trials,
            match (run.epoch, run.total_epochs) {
                (Some(epoch), Some(total)) => format!("{epoch}/{total}"),
                _ => "-".into(),
            },
            report_number(run.latest_loss),
            report_number(run.latest_metric),
            report_number(run.best_score),
        )
        .chars()
        .take(MAX_PDF_EVENT_CHARS)
        .collect::<String>()
    }));
    lines.extend([
        String::new(),
        format!(
            "Recent event audit (newest {} of {})",
            events.len().min(MAX_PDF_REPORT_EVENTS),
            events.len()
        ),
    ]);
    lines.extend(
        events
            .iter()
            .enumerate()
            .rev()
            .take(MAX_PDF_REPORT_EVENTS)
            .map(|(index, event)| {
                let detail = serde_json::to_string(event)
                    .unwrap_or_default()
                    .chars()
                    .take(MAX_PDF_EVENT_CHARS)
                    .collect::<String>();
                format!("{} | {} | {detail}", index + 1, event_kind(event))
            }),
    );
    Ok(lines)
}

fn report_number(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{value:.6}"))
}

fn event_kind(event: &TrainingEvent) -> &'static str {
    match event {
        TrainingEvent::RunContext { .. } => "Run context",
        TrainingEvent::Started { .. } => "Started",
        TrainingEvent::TrialStarted { .. } => "Trial started",
        TrainingEvent::FoldCompleted { .. } => "Fold completed",
        TrainingEvent::TrialCompleted { .. } => "Trial completed",
        TrainingEvent::Epoch { .. } => "Epoch",
        TrainingEvent::Batch { .. } => "Batch",
        TrainingEvent::Checkpoint { .. } => "Checkpoint",
        TrainingEvent::EarlyStopping { .. } => "Early stopping",
        TrainingEvent::Completed { .. } => "Completed",
        TrainingEvent::Failed { .. } => "Failed",
    }
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn training_plots(events: &[TrainingEvent]) -> Vec<PlotSpec> {
    let mut epoch_loss = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut epoch_metric = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut batch_loss = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut throughput = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut trial_scores = std::collections::BTreeMap::<String, Vec<[f64; 2]>>::new();
    let mut run_names = std::collections::BTreeMap::<String, String>::new();
    let mut pending_context = None;
    let mut sequential_run = 0usize;
    let mut sequential_label = "Unscoped telemetry".to_owned();
    for (index, event) in events.iter().enumerate() {
        if let TrainingEvent::RunContext { run_id } = event {
            pending_context = Some(run_id.clone());
            continue;
        }
        let context = pending_context.take();
        if let TrainingEvent::Started { job, .. } = event {
            if let Some(run_id) = context {
                run_names.insert(run_id, job.clone());
            } else {
                sequential_run += 1;
                sequential_label = format!("{job} #{sequential_run}");
            }
            continue;
        }
        let label = context.map_or_else(
            || sequential_label.clone(),
            |run_id| {
                run_names
                    .get(&run_id)
                    .map_or_else(|| run_id.clone(), |job| format!("{job} [{run_id}]"))
            },
        );
        match event {
            TrainingEvent::Epoch {
                epoch,
                loss,
                metric,
                ..
            } => {
                epoch_loss
                    .entry(label.clone())
                    .or_default()
                    .push([*epoch as f64, *loss]);
                if let Some(metric) = metric {
                    epoch_metric
                        .entry(label)
                        .or_default()
                        .push([*epoch as f64, *metric]);
                }
            }
            TrainingEvent::Batch {
                loss,
                samples_per_second,
                ..
            } => {
                batch_loss
                    .entry(label.clone())
                    .or_default()
                    .push([index as f64, *loss]);
                throughput
                    .entry(label)
                    .or_default()
                    .push([index as f64, *samples_per_second]);
            }
            TrainingEvent::TrialCompleted { trial, score } => {
                trial_scores
                    .entry(label)
                    .or_default()
                    .push([*trial as f64, *score]);
            }
            _ => {}
        }
    }
    let mut plots = Vec::new();
    let mut loss = std::collections::BTreeMap::new();
    loss.extend(
        epoch_loss
            .into_iter()
            .map(|(name, points)| (format!("{name} · epoch"), points)),
    );
    loss.extend(
        batch_loss
            .into_iter()
            .map(|(name, points)| (format!("{name} · batch"), points)),
    );
    let loss_series = training_series(loss);
    if !loss_series.is_empty() {
        plots.push(training_plot("Training loss", "step", "loss", loss_series));
    }
    if !epoch_metric.is_empty() {
        plots.push(training_plot(
            "Training metric",
            "epoch",
            "metric",
            training_series(epoch_metric),
        ));
    }
    if !trial_scores.is_empty() {
        plots.push(training_plot(
            "Trial scores",
            "trial",
            "score",
            training_series(trial_scores),
        ));
    }
    if !throughput.is_empty() {
        plots.push(training_plot(
            "Training throughput",
            "batch event",
            "samples/s",
            training_series(throughput),
        ));
    }
    plots
}

fn training_series(series: std::collections::BTreeMap<String, Vec<[f64; 2]>>) -> Vec<PlotSeries> {
    series
        .into_iter()
        .take(128)
        .map(|(name, points)| PlotSeries {
            name,
            points,
            values: Vec::new(),
            visible: true,
        })
        .collect()
}

fn training_plot(name: &str, x_label: &str, y_label: &str, series: Vec<PlotSeries>) -> PlotSpec {
    PlotSpec {
        version: PLOT_SPEC_VERSION,
        name: name.into(),
        kind: PlotKind::Line,
        x_label: x_label.into(),
        y_label: y_label.into(),
        series,
        matrix: Vec::new(),
        x_log: false,
        y_log: false,
    }
}

pub trait TrainingObserver: Send + Sync {
    fn observe(&self, event: TrainingEvent);
}

#[derive(Clone, Default)]
pub struct ChannelObserver(Arc<Mutex<Vec<TrainingEvent>>>);
impl TrainingObserver for ChannelObserver {
    fn observe(&self, event: TrainingEvent) {
        if let Ok(mut events) = self.0.lock() {
            record_training_event(&mut events, event);
        }
    }
}
impl ChannelObserver {
    pub fn drain(&self) -> Vec<TrainingEvent> {
        self.0
            .lock()
            .map(|mut events| std::mem::take(&mut *events))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PipelineStep {
    Impute,
    Standardize,
    OneHotEncode,
    RandomForest,
    LogisticRegression,
    LinearRegression,
}

impl PipelineStep {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Impute => "Impute",
            Self::Standardize => "Standardize",
            Self::OneHotEncode => "One-hot encode",
            Self::RandomForest => "Random forest",
            Self::LogisticRegression => "Logistic regression",
            Self::LinearRegression => "Linear regression",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineDesign {
    pub name: String,
    pub target: String,
    pub steps: Vec<PipelineStep>,
}

impl PipelineDesign {
    pub fn rust_code(&self) -> String {
        let mut code = String::from("use millwright::prelude::*;\n\n");
        code.push_str("let table = Table::from_csv(\"data.csv\")?;\n");
        code.push_str(&format!(
            "let train = table.into_dataset(\"{}\")?;\n",
            self.target
        ));
        code.push_str("let mut pipeline = Pipeline::new()\n");
        for step in &self.steps {
            let line = match step {
                PipelineStep::Impute => "    .step(\"impute\", SimpleImputer::mean())\n",
                PipelineStep::Standardize => "    .step(\"scale\", StandardScaler::new())\n",
                PipelineStep::OneHotEncode => "    .step(\"encode\", OneHotEncoder::infer())\n",
                PipelineStep::RandomForest => "    .estimator(\"model\", RandomForest::new())\n",
                PipelineStep::LogisticRegression => {
                    "    .estimator(\"model\", LogisticRegression::new())\n"
                }
                PipelineStep::LinearRegression => {
                    "    .estimator(\"model\", LinearRegression::new())\n"
                }
            };
            code.push_str(line);
        }
        code.push_str(";\npipeline.fit(&train)?;\n");
        code
    }

    pub fn onnx_export_code(&self, artifact: &str) -> String {
        let mut code = self.rust_code();
        code.push_str("\n// Millwright 2.2's native, published-crate portability boundary.\n");
        code.push_str("use millwright::onnx::{ExportOnnx, InferenceModel};\n");
        code.push_str(&format!(
            "let artifact = std::path::Path::new({artifact:?});\npipeline.export_onnx(artifact)?;\n"
        ));
        code.push_str("let portable = InferenceModel::load(artifact)?;\n");
        code.push_str("let native = pipeline.predict(train.features())?;\n");
        code.push_str("let round_trip = portable.predict(train.features())?;\n");
        code.push_str("assert_eq!(native.len(), round_trip.len());\n");
        code.push_str("println!(\"Exported and verified {} predictions at {}\", round_trip.len(), artifact.display());\n");
        code
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub accuracy: Option<f64>,
    pub rmse: Option<f64>,
    pub confusion: Vec<Vec<u64>>,
    pub roc: Vec<[f64; 2]>,
    pub residuals: Vec<f64>,
    pub feature_importance: Vec<(String, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub model: String,
    pub score: f64,
    pub duration_ms: u128,
    pub parameters: String,
}

pub fn parse_runtime_output(output: &str) -> (Vec<TrainingEvent>, Vec<EvaluationReport>) {
    let mut events = Vec::new();
    let mut reports = Vec::new();
    for line in output.lines() {
        if let Some(scoped) = line.strip_prefix("forge_training[") {
            if let Some((run_id, json)) = scoped.split_once("]:") {
                let context = TrainingEvent::RunContext {
                    run_id: run_id.to_owned(),
                };
                if validate_training_event(&context) {
                    if let Ok(event) = serde_json::from_str(json.trim()) {
                        record_training_event(&mut events, context);
                        record_training_event(&mut events, event);
                    }
                }
            }
            continue;
        }
        if let Some(json) = line.strip_prefix("forge_training:") {
            if let Ok(event) = serde_json::from_str(json.trim()) {
                record_training_event(&mut events, event);
            }
        }
        if let Some(json) = line.strip_prefix("forge_evaluation:") {
            if let Ok(report) = serde_json::from_str(json.trim()) {
                reports.push(report);
            }
        }
    }
    (events, reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generates_pipeline_code_in_step_order() {
        let design = PipelineDesign {
            name: "demo".into(),
            target: "label".into(),
            steps: vec![PipelineStep::Impute, PipelineStep::RandomForest],
        };
        let code = design.rust_code();
        assert!(code.find("SimpleImputer").unwrap() < code.find("RandomForest").unwrap());
        assert!(code.contains("into_dataset(\"label\")"));
    }
    #[test]
    fn generates_native_onnx_export_and_round_trip() {
        let design = PipelineDesign {
            name: "portable".into(),
            target: "label".into(),
            steps: vec![PipelineStep::Standardize, PipelineStep::LinearRegression],
        };
        let code = design.onnx_export_code("models/portable.onnx");
        assert!(code.contains("millwright::onnx::{ExportOnnx, InferenceModel}"));
        assert!(code.contains("pipeline.export_onnx(artifact)?"));
        assert!(code.contains("InferenceModel::load(artifact)?"));
    }
    #[test]
    fn channel_observer_forwards_events() {
        let observer = ChannelObserver::default();
        observer.observe(TrainingEvent::Completed { best_score: 0.9 });
        assert_eq!(
            observer.drain(),
            vec![TrainingEvent::Completed { best_score: 0.9 }]
        );
    }
    #[test]
    fn parses_training_telemetry() {
        let (events, _) =
            parse_runtime_output("forge_training:{\"Completed\":{\"best_score\":0.9}}");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn training_stream_is_bounded_valid_and_exportable() {
        let mut events = Vec::new();
        for trial in 0..=MAX_TRAINING_EVENTS {
            record_training_event(
                &mut events,
                TrainingEvent::TrialCompleted {
                    trial,
                    score: trial as f64,
                },
            );
        }
        record_training_event(
            &mut events,
            TrainingEvent::Completed {
                best_score: f64::NAN,
            },
        );
        assert_eq!(events.len(), MAX_TRAINING_EVENTS);
        assert!(matches!(
            events.first(),
            Some(TrainingEvent::TrialCompleted { trial: 1, .. })
        ));
        assert!(String::from_utf8(training_json(&events[..2]).unwrap())
            .unwrap()
            .contains("TrialCompleted"));
        let csv = String::from_utf8(training_csv(&events[..2]).unwrap()).unwrap();
        assert!(csv.starts_with("index,event"));
        let encoded = training_json(&events[..2]).unwrap();
        assert_eq!(parse_training_json(&encoded).unwrap(), events[..2]);
        assert!(parse_training_json(&vec![b' '; MAX_TRAINING_IMPORT_BYTES + 1]).is_err());
        let invalid = serde_json::to_vec(&vec![TrainingEvent::Failed {
            message: "x".repeat(MAX_TRAINING_TEXT_BYTES + 1),
        }])
        .unwrap();
        assert!(parse_training_json(&invalid).is_err());
    }

    #[test]
    fn training_events_become_valid_native_plots() {
        let events = vec![
            TrainingEvent::Epoch {
                epoch: 1,
                total: 2,
                loss: 0.8,
                metric: Some(0.7),
            },
            TrainingEvent::Batch {
                epoch: 1,
                batch: 1,
                total: 4,
                loss: 0.75,
                samples_per_second: 120.0,
            },
            TrainingEvent::TrialCompleted {
                trial: 3,
                score: 0.91,
            },
        ];
        let plots = training_plots(&events);
        assert_eq!(plots.len(), 4);
        assert!(plots.iter().all(|plot| plot.validate().is_ok()));
        assert_eq!(plots[0].series.len(), 2);
    }

    #[test]
    fn interleaved_training_plots_keep_run_series_separate() {
        let events = vec![
            TrainingEvent::RunContext { run_id: "a".into() },
            TrainingEvent::Started {
                job: "alpha".into(),
                total_trials: 1,
            },
            TrainingEvent::RunContext { run_id: "b".into() },
            TrainingEvent::Started {
                job: "beta".into(),
                total_trials: 1,
            },
            TrainingEvent::RunContext { run_id: "a".into() },
            TrainingEvent::Epoch {
                epoch: 1,
                total: 2,
                loss: 0.8,
                metric: Some(0.7),
            },
            TrainingEvent::RunContext { run_id: "b".into() },
            TrainingEvent::Epoch {
                epoch: 1,
                total: 2,
                loss: 0.4,
                metric: Some(0.9),
            },
        ];
        let plots = training_plots(&events);
        assert_eq!(plots.len(), 2);
        assert_eq!(plots[0].series.len(), 2);
        assert_eq!(plots[1].series.len(), 2);
        assert_eq!(plots[0].series[0].name, "alpha [a] · epoch");
        assert_eq!(plots[0].series[0].points[0][1], 0.8);
        assert_eq!(plots[0].series[1].name, "beta [b] · epoch");
        assert_eq!(plots[0].series[1].points[0][1], 0.4);
        assert!(plots.iter().all(|plot| plot.validate().is_ok()));
    }

    #[test]
    fn training_report_summarizes_and_escapes_events() {
        let events = vec![
            TrainingEvent::RunContext {
                run_id: "run&1".into(),
            },
            TrainingEvent::Started {
                job: "<job>".into(),
                total_trials: 1,
            },
            TrainingEvent::RunContext {
                run_id: "run&1".into(),
            },
            TrainingEvent::Epoch {
                epoch: 1,
                total: 1,
                loss: 0.25,
                metric: Some(0.9),
            },
            TrainingEvent::TrialCompleted {
                trial: 1,
                score: 0.91,
            },
            TrainingEvent::Failed {
                message: "<script>alert(1)</script>".into(),
            },
        ];
        let report = training_report(&events).unwrap();
        assert!(report.contains("0.250000"));
        assert!(report.contains("0.910000"));
        assert!(report.contains("&lt;job&gt;"));
        assert!(report.contains("run&amp;1"));
        assert!(report.contains("<h2>Training runs</h2>"));
        assert!(report.contains("&lt;script&gt;"));
        assert!(!report.contains("<script>"));
        assert!(!report.contains("https://"));
        let lines = training_pdf_lines(&events).unwrap();
        assert!(lines.iter().any(|line| line == "Failures: 1"));
        assert!(lines.iter().any(|line| line == "Best score: 0.910000"));
        assert!(lines.iter().any(|line| line.contains("run run&1")));
    }

    #[test]
    fn training_run_overview_summarizes_sequential_jobs_and_bounds_history() {
        let events = vec![
            TrainingEvent::Started {
                job: "search".into(),
                total_trials: 4,
            },
            TrainingEvent::TrialCompleted {
                trial: 1,
                score: 0.8,
            },
            TrainingEvent::Epoch {
                epoch: 2,
                total: 10,
                loss: 0.4,
                metric: Some(0.9),
            },
            TrainingEvent::Completed { best_score: 0.95 },
            TrainingEvent::Started {
                job: "fine-tune".into(),
                total_trials: 1,
            },
            TrainingEvent::Batch {
                epoch: 1,
                batch: 2,
                total: 8,
                loss: 0.7,
                samples_per_second: 50.0,
            },
            TrainingEvent::Failed {
                message: "stopped".into(),
            },
        ];
        let overview = training_run_overview(&events);
        assert_eq!(overview.len(), 2);
        assert_eq!(overview[0].job, "fine-tune");
        assert_eq!(overview[0].status, "Failed");
        assert_eq!(overview[0].latest_loss, Some(0.7));
        assert_eq!(overview[1].status, "Completed");
        assert_eq!(overview[1].completed_trials, 1);
        assert_eq!(overview[1].epoch, Some(2));
        assert_eq!(overview[1].best_score, Some(0.95));

        let fleet = (0..=MAX_RUN_OVERVIEW_ROWS)
            .map(|index| TrainingEvent::Started {
                job: format!("job-{index}"),
                total_trials: 1,
            })
            .collect::<Vec<_>>();
        assert_eq!(training_run_overview(&fleet).len(), MAX_RUN_OVERVIEW_ROWS);
    }

    #[test]
    fn training_run_csv_is_structured_bounded_and_escaped() {
        let events = vec![
            TrainingEvent::Started {
                job: "search, \"wide\"".into(),
                total_trials: 2,
            },
            TrainingEvent::TrialCompleted {
                trial: 1,
                score: 0.8,
            },
            TrainingEvent::Epoch {
                epoch: 2,
                total: 4,
                loss: 0.3,
                metric: Some(0.9),
            },
            TrainingEvent::Completed { best_score: 0.95 },
        ];
        let output = training_run_csv(&events).unwrap();
        let mut reader = csv::Reader::from_reader(output.as_slice());
        assert_eq!(reader.headers().unwrap().len(), 11);
        let rows = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(&rows[0][2], "search, \"wide\"");
        assert_eq!(&rows[0][3], "Completed");
        assert_eq!(&rows[0][4], "1");
        assert_eq!(&rows[0][6], "2");
        assert_eq!(&rows[0][10], "0.95");
        assert!(training_run_csv(&[]).is_err());
    }

    #[test]
    fn scoped_training_lines_support_interleaved_run_progress() {
        let output = concat!(
            "forge_training[a]:{\"Started\":{\"job\":\"alpha\",\"total_trials\":2}}\n",
            "forge_training[b]:{\"Started\":{\"job\":\"beta\",\"total_trials\":1}}\n",
            "forge_training[a]:{\"TrialCompleted\":{\"trial\":1,\"score\":0.8}}\n",
            "forge_training[b]:{\"Completed\":{\"best_score\":0.9}}"
        );
        let (events, _) = parse_runtime_output(output);
        assert_eq!(events.len(), 8);
        let overview = training_run_overview(&events);
        assert_eq!(overview.len(), 2);
        assert_eq!(overview[0].run_id.as_deref(), Some("b"));
        assert_eq!(overview[0].status, "Completed");
        assert_eq!(overview[1].run_id.as_deref(), Some("a"));
        assert_eq!(overview[1].completed_trials, 1);
        assert!(!validate_training_event(&TrainingEvent::RunContext {
            run_id: "]".into()
        }));
    }
}
