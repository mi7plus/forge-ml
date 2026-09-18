//! AutoML hyperparameter search over the embedded Burn regressor.
//!
//! Forge drives the framework-agnostic [`automl_core`] engine (TPE-style search
//! with a seeded RNG) and uses **its own Burn 0.22 trainer**
//! ([`crate::deep_learning::native_burn_training_demo_with_progress`]) as the
//! objective — so every trial trains on the same CPU/GPU backend as manual
//! training, with no second deep-learning stack.
//!
//! The sibling `automl-burn` adapter is deliberately *not* used: it pins
//! `burn = 0.21`, whose ndarray backend now transitively pulls an LLVM/MLIR
//! compiler toolchain (`tracel-mlir-sys`, ~100 extra crates) that cannot build in
//! Forge's offline, self-contained model. `automl-core` carries none of that.

use crate::deep_learning::{Backend, NativeTrainingConfig, NativeTrainingData};
// Import specifics rather than the glob prelude: the prelude re-exports a 1-arg
// `Result` alias that would shadow `std::result::Result` in this module's own
// signatures.
use automl_core::prelude::{Distribution, NamedMetrics, ParamSet, ReportSink, SearchSpace, Study};
use std::sync::Mutex;

/// One evaluated hyperparameter trial.
#[derive(Clone, Debug, PartialEq)]
pub struct AutomlTrial {
    pub trial: usize,
    pub learning_rate: f64,
    pub epochs: usize,
    /// Validation score (higher is better; the trainer reports `-loss`).
    pub score: f64,
}

/// The result of an AutoML search: the best configuration found plus the full
/// per-trial history.
#[derive(Clone, Debug, Default)]
pub struct AutomlOutcome {
    pub best_learning_rate: f64,
    pub best_epochs: usize,
    pub best_score: f64,
    pub trials_completed: usize,
    pub history: Vec<AutomlTrial>,
}

/// The learning-rate and epoch bounds the search explores.
const LR_MIN: f64 = 1e-4;
const LR_MAX: f64 = 1e-1;
const EPOCHS_MIN: i64 = 10;
const EPOCHS_MAX: i64 = 80;

/// Search `trials` hyperparameter configurations (learning rate + epochs) for the
/// embedded Burn regressor on `data`, maximizing the validation score.
///
/// `on_trial` is called once per completed trial (for live progress), and
/// `cancelled` is polled between trials to stop early. Returns the best
/// configuration found so far even when cancelled.
pub fn search(
    data: &NativeTrainingData,
    backend: Backend,
    trials: u64,
    seed: u64,
    cancelled: impl Fn() -> bool + Sync,
    mut on_trial: impl FnMut(AutomlTrial),
) -> Result<AutomlOutcome, String> {
    if trials == 0 {
        return Err("AutoML needs at least one trial".to_owned());
    }
    let space = SearchSpace::new()
        .add("lr", Distribution::log_float(LR_MIN, LR_MAX))
        .add("epochs", Distribution::int(EPOCHS_MIN, EPOCHS_MAX));
    let mut study = Study::builder(space)
        .maximize("score")
        .seed(seed)
        .build()
        .map_err(|error| error.to_string())?;

    // The objective writes each evaluation here so the driving loop below can
    // report it and record history (the objective itself is `Fn`, so it can only
    // reach the outer state through interior mutability).
    let last: Mutex<Option<AutomlTrial>> = Mutex::new(None);
    let objective =
        |params: &ParamSet, _sink: &mut dyn ReportSink| -> automl_core::Result<NamedMetrics> {
            let learning_rate = params.float("lr")?;
            let epochs = params.int("epochs")?.max(1) as usize;
            let config = NativeTrainingConfig {
                epochs,
                learning_rate,
                validation_fraction: 0.2,
                early_stopping_patience: 0,
            };
            // A configuration that fails to train scores worst rather than aborting
            // the whole search.
            let score = crate::deep_learning::native_burn_training_demo_with_progress(
                backend,
                config,
                Some(data.clone()),
                &cancelled,
                |_| {},
            )
            .map(|outcome| outcome.artifact.best_score)
            .unwrap_or(f64::MIN);
            *last.lock().expect("automl trial mutex") = Some(AutomlTrial {
                trial: 0,
                learning_rate,
                epochs,
                score,
            });
            Ok(NamedMetrics::single("score", score))
        };

    let mut history = Vec::new();
    for index in 0..trials {
        if cancelled() {
            break;
        }
        study
            .optimize_n(&objective, 1)
            .map_err(|error| error.to_string())?;
        if let Some(mut trial) = last.lock().expect("automl trial mutex").take() {
            trial.trial = index as usize;
            history.push(trial.clone());
            on_trial(trial);
        }
    }

    let best = study.best_trial().map_err(|error| error.to_string())?;
    let mut outcome = AutomlOutcome {
        trials_completed: history.len(),
        history,
        ..Default::default()
    };
    if let Some(best) = best {
        outcome.best_learning_rate = best.params.float("lr").unwrap_or(0.0);
        outcome.best_epochs = best.params.int("epochs").unwrap_or(0).max(0) as usize;
        outcome.best_score = best.final_value("score").unwrap_or(f64::MIN);
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_rejects_zero_trials() {
        let data = crate::deep_learning::native_training_data(
            "d",
            &forge_protocol::TableData {
                columns: vec!["x".into(), "y".into()],
                rows: (0..8)
                    .map(|i| vec![i.to_string(), (2 * i + 1).to_string()])
                    .collect(),
            },
            "x",
            "y",
        )
        .unwrap();
        assert!(search(&data, Backend::Cpu, 0, 1, || false, |_| {}).is_err());
    }

    #[test]
    fn search_explores_and_returns_a_best_configuration() {
        // A small linear dataset: y = 2x + 1. Two trials keep the test fast while
        // still exercising the full engine + Burn objective wiring.
        let table = forge_protocol::TableData {
            columns: vec!["x".into(), "y".into()],
            rows: (0..24)
                .map(|i| {
                    let x = i as f64;
                    vec![format!("{x}"), format!("{}", 2.0 * x + 1.0)]
                })
                .collect(),
        };
        let data = crate::deep_learning::native_training_data("demo", &table, "x", "y").unwrap();
        let outcome = search(&data, Backend::Cpu, 2, 7, || false, |_| {}).unwrap();
        assert_eq!(outcome.trials_completed, 2);
        assert_eq!(outcome.history.len(), 2);
        assert!((LR_MIN..=LR_MAX).contains(&outcome.best_learning_rate));
        assert!((EPOCHS_MIN as usize..=EPOCHS_MAX as usize).contains(&outcome.best_epochs));
        assert!(outcome.best_score.is_finite());
    }

    #[test]
    fn cancellation_stops_the_search_early() {
        let table = forge_protocol::TableData {
            columns: vec!["x".into(), "y".into()],
            rows: (0..16)
                .map(|i| vec![format!("{i}"), format!("{}", 3 * i)])
                .collect(),
        };
        let data = crate::deep_learning::native_training_data("demo", &table, "x", "y").unwrap();
        // Cancel immediately: no trial should run.
        let outcome = search(&data, Backend::Cpu, 10, 1, || true, |_| {}).unwrap();
        assert_eq!(outcome.trials_completed, 0);
    }
}
