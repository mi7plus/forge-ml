use crate::experiment::ExperimentRun;
use crate::plot::PlotSpec;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_explorer_height() -> f32 {
    280.0
}

fn default_editor_font_size() -> f32 {
    14.0
}

fn default_dataset_pane_height() -> f32 {
    280.0
}

fn default_true() -> bool {
    true
}

fn default_experiment_name() -> String {
    "run_1".to_owned()
}

fn default_comparison_metric() -> String {
    "loss".to_owned()
}

fn default_drift_mean_shift_threshold() -> f64 {
    1.0
}

fn default_drift_scale_ratio_lower() -> f64 {
    0.5
}

fn default_drift_scale_ratio_upper() -> f64 {
    2.0
}

fn default_training_backend() -> crate::deep_learning::Backend {
    crate::deep_learning::Backend::Cpu
}

fn default_training_epochs() -> usize {
    40
}

fn default_training_learning_rate() -> f64 {
    0.05
}

fn default_training_validation_fraction() -> f64 {
    0.2
}

fn default_training_patience() -> usize {
    5
}

const MAX_SESSION_PLOTS: usize = 128;
const MAX_SESSION_PLOT_BYTES: usize = 16 * 1024 * 1024;

pub fn bounded_plots(plots: &[PlotSpec]) -> Vec<PlotSpec> {
    let mut result = Vec::new();
    let mut bytes = 0usize;
    for plot in plots.iter().take(MAX_SESSION_PLOTS) {
        if plot.validate().is_err() {
            continue;
        }
        let Ok(size) = serde_json::to_vec(plot).map(|value| value.len()) else {
            continue;
        };
        if bytes.saturating_add(size) > MAX_SESSION_PLOT_BYTES {
            break;
        }
        bytes += size;
        result.push(plot.clone());
    }
    result
}

#[derive(Serialize, Deserialize)]
pub struct SessionState {
    pub project_root: Option<PathBuf>,
    pub open_files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
    #[serde(default)]
    pub dark_mode: bool,
    #[serde(default = "default_explorer_height")]
    pub explorer_height: f32,
    #[serde(default)]
    pub recent_projects: Vec<PathBuf>,
    #[serde(default = "default_editor_font_size")]
    pub editor_font_size: f32,
    #[serde(default = "default_true")]
    pub caret_blink: bool,
    #[serde(default)]
    pub format_on_save: bool,
    #[serde(default = "default_true")]
    pub show_welcome: bool,
    #[serde(default)]
    pub keymap: Vec<crate::keymap::ChordDto>,
    #[serde(default)]
    pub high_contrast: bool,
    #[serde(default = "default_lsp_enabled")]
    pub lsp_enabled: bool,
    #[serde(default)]
    pub reduced_motion: bool,
    #[serde(default)]
    pub diagnostics_opt_in: bool,
    #[serde(default)]
    pub saved_runs: Vec<ExperimentRun>,
    #[serde(default = "default_experiment_name")]
    pub experiment_name: String,
    #[serde(default = "default_comparison_metric")]
    pub comparison_metric: String,
    #[serde(default = "default_true")]
    pub dataset_viewer_docked: bool,
    #[serde(default = "default_dataset_pane_height")]
    pub dataset_pane_height: f32,
    #[serde(default)]
    pub selected_python: Option<PathBuf>,
    #[serde(default)]
    pub selected_jupyter_kernel: String,
    #[serde(default)]
    pub python_environment_fingerprint: String,
    #[serde(default)]
    pub structured_plots: Vec<PlotSpec>,
    #[serde(default)]
    pub native_regression_artifact: Option<crate::deep_learning::NativeRegressionArtifact>,
    #[serde(default)]
    pub native_inference_feature: f64,
    #[serde(default = "default_drift_mean_shift_threshold")]
    pub drift_mean_shift_threshold: f64,
    #[serde(default = "default_drift_scale_ratio_lower")]
    pub drift_scale_ratio_lower: f64,
    #[serde(default = "default_drift_scale_ratio_upper")]
    pub drift_scale_ratio_upper: f64,
    #[serde(default = "default_training_backend")]
    pub native_training_backend: crate::deep_learning::Backend,
    #[serde(default = "default_training_epochs")]
    pub native_training_epochs: usize,
    #[serde(default = "default_training_learning_rate")]
    pub native_training_learning_rate: f64,
    #[serde(default = "default_training_validation_fraction")]
    pub native_training_validation_fraction: f64,
    #[serde(default = "default_training_patience")]
    pub native_training_patience: usize,
    #[serde(default)]
    pub native_training_use_dataset: bool,
    #[serde(default)]
    pub native_training_feature: String,
    #[serde(default)]
    pub native_training_target: String,
    /// Serialized `egui_tiles` dock layout (JSON). `None` restores the default.
    #[serde(default)]
    pub dock_layout: Option<String>,
    /// Name of the active custom theme (`None` = built-in Dark/Light).
    #[serde(default)]
    pub active_theme: Option<String>,
    /// User-authored themes available in the theme builder.
    #[serde(default)]
    pub custom_themes: Vec<crate::ui::theme::NamedTheme>,
    /// Global UI zoom factor scaling all text and widgets (1.0 = 100%).
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_lsp_enabled() -> bool {
    true
}

impl SessionState {
    pub fn validated_native_artifact(
        &self,
    ) -> Option<crate::deep_learning::NativeRegressionArtifact> {
        self.native_regression_artifact
            .clone()
            .filter(|artifact| artifact.validate().is_ok())
    }

    pub fn validated_drift_policy(&self) -> crate::deep_learning::DriftPolicy {
        crate::deep_learning::DriftPolicy {
            mean_shift_threshold: self.drift_mean_shift_threshold,
            scale_ratio_lower: self.drift_scale_ratio_lower,
            scale_ratio_upper: self.drift_scale_ratio_upper,
        }
        .validate()
        .unwrap_or_default()
    }

    pub fn validated_native_training_config(&self) -> crate::deep_learning::NativeTrainingConfig {
        crate::deep_learning::NativeTrainingConfig {
            epochs: self.native_training_epochs,
            learning_rate: self.native_training_learning_rate,
            validation_fraction: self.native_training_validation_fraction,
            early_stopping_patience: self.native_training_patience,
        }
        .validate()
        .unwrap_or(crate::deep_learning::NativeTrainingConfig {
            early_stopping_patience: default_training_patience(),
            ..Default::default()
        })
    }

    pub fn validated_training_columns(&self) -> (String, String) {
        fn safe(value: &str) -> String {
            if value.len() <= 128 && !value.contains('\0') {
                value.to_owned()
            } else {
                String::new()
            }
        }
        (
            safe(&self.native_training_feature),
            safe(&self.native_training_target),
        )
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            project_root: None,
            open_files: Vec::new(),
            active_file: None,
            dark_mode: false,
            explorer_height: default_explorer_height(),
            recent_projects: Vec::new(),
            editor_font_size: default_editor_font_size(),
            caret_blink: true,
            format_on_save: false,
            show_welcome: true,
            keymap: Vec::new(),
            high_contrast: false,
            lsp_enabled: true,
            reduced_motion: false,
            diagnostics_opt_in: false,
            saved_runs: Vec::new(),
            experiment_name: default_experiment_name(),
            comparison_metric: default_comparison_metric(),
            dataset_viewer_docked: true,
            dataset_pane_height: default_dataset_pane_height(),
            selected_python: None,
            selected_jupyter_kernel: String::new(),
            python_environment_fingerprint: String::new(),
            structured_plots: Vec::new(),
            native_regression_artifact: None,
            native_inference_feature: 0.0,
            drift_mean_shift_threshold: default_drift_mean_shift_threshold(),
            drift_scale_ratio_lower: default_drift_scale_ratio_lower(),
            drift_scale_ratio_upper: default_drift_scale_ratio_upper(),
            native_training_backend: default_training_backend(),
            native_training_epochs: default_training_epochs(),
            native_training_learning_rate: default_training_learning_rate(),
            native_training_validation_fraction: default_training_validation_fraction(),
            native_training_patience: default_training_patience(),
            native_training_use_dataset: false,
            native_training_feature: String::new(),
            native_training_target: String::new(),
            dock_layout: None,
            active_theme: None,
            custom_themes: Vec::new(),
            ui_scale: default_ui_scale(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_sessions_receive_new_defaults() {
        let state: SessionState =
            serde_json::from_str(r#"{"project_root":null,"open_files":[],"active_file":null}"#)
                .unwrap();
        assert_eq!(state.experiment_name, "run_1");
        assert_eq!(state.comparison_metric, "loss");
        assert!(state.dataset_viewer_docked);
        assert_eq!(state.dataset_pane_height, 280.0);
        assert!(!state.high_contrast);
        assert!(!state.reduced_motion);
        assert!(!state.diagnostics_opt_in);
        assert!(state.native_regression_artifact.is_none());
        assert_eq!(state.validated_drift_policy(), Default::default());
        assert_eq!(state.validated_native_training_config().epochs, 40);
    }

    #[test]
    fn new_sessions_match_persisted_docking_defaults() {
        let mut state = SessionState::default();
        assert!(state.dataset_viewer_docked);
        assert_eq!(state.dataset_pane_height, 280.0);

        state.dataset_viewer_docked = false;
        state.dataset_pane_height = 412.0;
        let restored: SessionState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert!(!restored.dataset_viewer_docked);
        assert_eq!(restored.dataset_pane_height, 412.0);
    }

    #[test]
    fn persisted_plots_are_valid_and_bounded() {
        let plot = PlotSpec {
            version: crate::plot::PLOT_SPEC_VERSION,
            name: "loss".into(),
            kind: crate::plot::PlotKind::Line,
            x_label: String::new(),
            y_label: String::new(),
            series: Vec::new(),
            matrix: Vec::new(),
            x_log: false,
            y_log: false,
        };
        let plots = vec![plot; MAX_SESSION_PLOTS + 10];
        assert_eq!(bounded_plots(&plots).len(), MAX_SESSION_PLOTS);
        let restored: SessionState = serde_json::from_str(
            &serde_json::to_string(&SessionState {
                structured_plots: bounded_plots(&plots),
                ..SessionState::default()
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            bounded_plots(&restored.structured_plots).len(),
            MAX_SESSION_PLOTS
        );
    }

    #[test]
    fn native_model_and_drift_policy_round_trip_safely() {
        let artifact = crate::deep_learning::NativeRegressionArtifact {
            schema: 1,
            run_id: "burn-flex-session".into(),
            dataset: "sample".into(),
            feature: "x".into(),
            target: "y".into(),
            slope: 2.0,
            intercept: 1.0,
            feature_scale: 1.0,
            target_scale: 1.0,
            best_score: 0.1,
            epochs_completed: 2,
            ..Default::default()
        };
        let state = SessionState {
            native_regression_artifact: Some(artifact.clone()),
            native_inference_feature: 4.5,
            drift_mean_shift_threshold: 1.5,
            drift_scale_ratio_lower: 0.25,
            drift_scale_ratio_upper: 3.0,
            native_training_backend: crate::deep_learning::Backend::Wgpu,
            native_training_epochs: 250,
            native_training_learning_rate: 0.01,
            native_training_validation_fraction: 0.3,
            native_training_patience: 12,
            native_training_use_dataset: true,
            native_training_feature: "temperature".into(),
            native_training_target: "demand".into(),
            ..Default::default()
        };
        let restored: SessionState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(restored.validated_native_artifact(), Some(artifact));
        assert_eq!(restored.native_inference_feature, 4.5);
        assert_eq!(restored.validated_drift_policy().mean_shift_threshold, 1.5);
        assert_eq!(
            restored.native_training_backend,
            crate::deep_learning::Backend::Wgpu
        );
        assert_eq!(restored.validated_native_training_config().epochs, 250);
        assert_eq!(
            restored
                .validated_native_training_config()
                .early_stopping_patience,
            12
        );
        assert!(restored.native_training_use_dataset);
        assert_eq!(
            restored.validated_training_columns(),
            ("temperature".into(), "demand".into())
        );

        let invalid = SessionState {
            native_regression_artifact: Some(crate::deep_learning::NativeRegressionArtifact {
                schema: 99,
                ..Default::default()
            }),
            drift_mean_shift_threshold: 0.0,
            native_training_epochs: 0,
            native_training_feature: "x".repeat(129),
            native_training_target: "bad\0target".into(),
            ..Default::default()
        };
        assert!(invalid.validated_native_artifact().is_none());
        assert_eq!(invalid.validated_drift_policy(), Default::default());
        assert_eq!(invalid.validated_native_training_config().epochs, 40);
        assert_eq!(
            invalid.validated_training_columns(),
            (String::new(), String::new())
        );
    }
}
