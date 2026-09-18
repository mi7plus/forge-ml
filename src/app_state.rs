//! Grouped sub-state structs for [`crate::ForgeApp`].
//!
//! Each of these bundles a cohesive slice of the IDE's UI/session state (a form
//! plus its results, or one subsystem's live handles) that used to live as a
//! flat run of `prefix_*` fields on `ForgeApp`. Fields are `pub(crate)` so the
//! app shell and the `ui::*` view code reach them directly.

use crate::*;

/// AutoML hyperparameter-search state: the search form, the live cancel handle
/// and trial count while a search runs, and the last result. Grouped out of
/// [`crate::ForgeApp`].
pub(crate) struct AutomlState {
    pub(crate) trials: u64,
    pub(crate) seed: u64,
    pub(crate) cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) progress: usize,
    pub(crate) outcome: Option<automl::AutomlOutcome>,
    pub(crate) status: String,
}

impl Default for AutomlState {
    fn default() -> Self {
        Self {
            trials: 15,
            seed: 42,
            cancel: None,
            progress: 0,
            outcome: None,
            status: String::new(),
        }
    }
}

/// rust-analyzer (LSP) client state: the handle plus the diagnostics, hover
/// signature, references, and status it drives. Grouped out of [`crate::ForgeApp`].
pub(crate) struct LspState {
    pub(crate) handle: LspHandle,
    pub(crate) status: String,
    pub(crate) diagnostics: HashMap<PathBuf, Vec<LspDiagnostic>>,
    pub(crate) references: Vec<lsp::Reference>,
    pub(crate) signature: String,
    pub(crate) ready: bool,
    pub(crate) enabled: bool,
}

/// SQL workbench state (editor buffer, last output, query history), grouped out
/// of [`ForgeApp`].
pub(crate) struct SqlState {
    pub(crate) editor: String,
    pub(crate) output: String,
    pub(crate) history: Vec<String>,
}

impl Default for SqlState {
    fn default() -> Self {
        Self {
            editor: "SELECT 1 AS value;".into(),
            output: String::new(),
            history: Vec::new(),
        }
    }
}

/// GitHub integration form state, grouped out of [`ForgeApp`].
#[derive(Default)]
pub(crate) struct GithubState {
    pub(crate) input: String,
    pub(crate) output: String,
    pub(crate) enterprise_host: String,
}

/// Classical classification (softmax) training form + fitted model, grouped out
/// of [`ForgeApp`].
pub(crate) struct ClassificationState {
    pub(crate) features: String,
    pub(crate) target: String,
    pub(crate) epochs: usize,
    pub(crate) lr: f64,
    pub(crate) test_fraction: f64,
    pub(crate) result: String,
    pub(crate) model: Option<classification::Classifier>,
    pub(crate) playground: Vec<f64>,
}

impl Default for ClassificationState {
    fn default() -> Self {
        Self {
            features: String::new(),
            target: String::new(),
            epochs: 300,
            lr: 0.5,
            test_fraction: 0.25,
            result: String::new(),
            model: None,
            playground: Vec::new(),
        }
    }
}

/// Native Burn (deep-learning) training configuration + cancel handle, grouped
/// out of [`ForgeApp`].
pub(crate) struct BurnState {
    pub(crate) training_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) training_epochs: usize,
    pub(crate) training_learning_rate: f64,
    pub(crate) training_validation_fraction: f64,
    pub(crate) training_use_dataset: bool,
    pub(crate) training_feature: String,
    pub(crate) training_target: String,
}

/// ONNX inference form + loaded model, grouped out of [`ForgeApp`].
#[derive(Default)]
pub(crate) struct OnnxState {
    pub(crate) model: Option<millwright::onnx::InferenceModel>,
    pub(crate) model_name: String,
    pub(crate) input: String,
    pub(crate) result: String,
}

/// Feature-drift monitoring policy + collected events, grouped out of
/// [`ForgeApp`].
pub(crate) struct DriftState {
    pub(crate) mean_shift_threshold: f64,
    pub(crate) scale_ratio_lower: f64,
    pub(crate) scale_ratio_upper: f64,
    pub(crate) events: Vec<DriftEvent>,
}

/// Dataset-preparation form state, grouped out of [`ForgeApp`].
pub(crate) struct PrepState {
    pub(crate) categorical: String,
    pub(crate) encoding: prep::Encoding,
    pub(crate) missing: prep::Missing,
    pub(crate) scaling: prep::Scaling,
    pub(crate) result: String,
}

impl Default for PrepState {
    fn default() -> Self {
        Self {
            categorical: String::new(),
            encoding: prep::Encoding::OneHot,
            missing: prep::Missing::Mean,
            scaling: prep::Scaling::None,
            result: String::new(),
        }
    }
}

/// Object-storage connection state (the connection form + saved profiles),
/// grouped out of [`ForgeApp`].
pub(crate) struct ObjectState {
    pub(crate) profiles: Vec<object_storage::ObjectProfile>,
    pub(crate) name: String,
    pub(crate) provider: object_storage::Provider,
    pub(crate) bucket: String,
    pub(crate) prefix: String,
    pub(crate) endpoint: String,
    pub(crate) key: String,
    pub(crate) output: String,
}

impl Default for ObjectState {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            name: "datasets".into(),
            provider: object_storage::Provider::S3,
            bucket: String::new(),
            prefix: String::new(),
            endpoint: String::new(),
            key: String::new(),
            output: String::new(),
        }
    }
}

/// Model-registry form state, grouped out of [`ForgeApp`].
pub(crate) struct RegistryState {
    pub(crate) model: String,
    pub(crate) version: String,
    pub(crate) format: String,
    pub(crate) alias: String,
    pub(crate) artifact: String,
    pub(crate) output: String,
}

impl Default for RegistryState {
    fn default() -> Self {
        Self {
            model: "model".into(),
            version: "0.1.0".into(),
            format: "onnx".into(),
            alias: "production".into(),
            artifact: String::new(),
            output: String::new(),
        }
    }
}

/// Local Git working state (staging/commit/branch UI), grouped out of
/// [`ForgeApp`].
#[derive(Default)]
pub(crate) struct GitState {
    pub(crate) output: String,
    pub(crate) commit_message: String,
    pub(crate) branch_name: String,
    pub(crate) conflicts: Vec<String>,
    pub(crate) branches: Vec<git::BranchInfo>,
    pub(crate) selected_branch: Option<String>,
}

/// Experiment-run metadata form state, grouped out of [`ForgeApp`].
#[derive(Default)]
pub(crate) struct ExperimentState {
    pub(crate) name: String,
    pub(crate) tags: String,
    pub(crate) notes: String,
    pub(crate) github_issue: String,
    pub(crate) github_pr: String,
    pub(crate) github_action: String,
}

/// SQL database connection state (the connection form + saved profiles),
/// grouped out of [`ForgeApp`].
pub(crate) struct DatabaseState {
    pub(crate) profiles: Vec<ConnectionProfile>,
    pub(crate) selected: usize,
    pub(crate) name: String,
    pub(crate) kind: ConnectionKind,
    pub(crate) location: String,
    pub(crate) username: String,
    pub(crate) secret: String,
}

impl Default for DatabaseState {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            selected: 0,
            name: "local".into(),
            kind: ConnectionKind::SQLite,
            location: "data.sqlite3".into(),
            username: String::new(),
            secret: String::new(),
        }
    }
}

/// Python bridge state (managed runtime discovery + the Python console/kernel),
/// grouped out of [`ForgeApp`].
pub(crate) struct PythonState {
    pub(crate) registry: String,
    pub(crate) runtime_output: String,
    pub(crate) runtimes: Vec<python_runtime::PythonRuntime>,
    pub(crate) kernel: Option<python_kernel::PythonKernel>,
    pub(crate) console_input: String,
    pub(crate) console_output: String,
    pub(crate) execution_id: usize,
    pub(crate) mime_outputs: Vec<RichOutput>,
    pub(crate) environment_fingerprint: String,
}

impl Default for PythonState {
    fn default() -> Self {
        Self {
            registry: "https://pypi.org".into(),
            runtime_output: String::new(),
            runtimes: Vec::new(),
            kernel: None,
            console_input: String::new(),
            console_output: String::new(),
            execution_id: 0,
            mime_outputs: Vec::new(),
            environment_fingerprint: String::new(),
        }
    }
}

/// Remote Jupyter / kernel execution state, grouped out of [`ForgeApp`].
pub(crate) struct RemoteState {
    pub(crate) profiles: Vec<remote::RemoteProfile>,
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) command: String,
    pub(crate) token: String,
    pub(crate) kernel_name: String,
    /// Kernel names discovered by the last "Test Jupyter" probe, offered as
    /// one-click fills for `kernel_name` (e.g. `rust` for remote Evcxr).
    pub(crate) kernelspecs: Vec<String>,
    pub(crate) kernel_session: Option<remote::RemoteKernelSession>,
    pub(crate) code: String,
    pub(crate) mime_outputs: Vec<RichOutput>,
    pub(crate) execution_pending: bool,
    pub(crate) interrupt_pending: bool,
    pub(crate) notebook_execution: bool,
    pub(crate) input_sender: Option<Sender<String>>,
    pub(crate) input_prompt: Option<String>,
    pub(crate) input_response: String,
    pub(crate) input_password: bool,
}

impl Default for RemoteState {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            name: "remote".into(),
            url: String::new(),
            command: "cargo run --release".into(),
            token: String::new(),
            kernel_name: "python3".into(),
            kernelspecs: Vec::new(),
            kernel_session: None,
            code: "print(\"hello from Forge ML\")".into(),
            mime_outputs: Vec::new(),
            execution_pending: false,
            interrupt_pending: false,
            notebook_execution: false,
            input_sender: None,
            input_prompt: None,
            input_response: String::new(),
            input_password: false,
        }
    }
}

// --- Constructors that build a sub-state from persisted session/workspace data,
// keeping ForgeApp::new's initializer to grouped one-liners. ---

impl LspState {
    pub(crate) fn from_session(session: &SessionState) -> Self {
        Self {
            handle: LspHandle::spawn(),
            status: "rust-analyzer waiting for a Rust file.".to_owned(),
            diagnostics: HashMap::new(),
            references: Vec::new(),
            signature: String::new(),
            ready: false,
            enabled: session.lsp_enabled,
        }
    }
}

impl SqlState {
    pub(crate) fn load(store: &Option<WorkspaceStore>) -> Self {
        Self {
            history: database::bounded_query_history(
                store
                    .as_ref()
                    .and_then(|store| store.load_query_history().ok())
                    .unwrap_or_default(),
            ),
            ..Default::default()
        }
    }
}

impl PythonState {
    pub(crate) fn from_session(session: &SessionState) -> Self {
        Self {
            environment_fingerprint: session.python_environment_fingerprint.clone(),
            ..Default::default()
        }
    }
}

impl DriftState {
    pub(crate) fn from_session(session: &SessionState) -> Self {
        let policy = session.validated_drift_policy();
        Self {
            mean_shift_threshold: policy.mean_shift_threshold,
            scale_ratio_lower: policy.scale_ratio_lower,
            scale_ratio_upper: policy.scale_ratio_upper,
            events: Vec::new(),
        }
    }
}

impl BurnState {
    pub(crate) fn new(
        config: &deep_learning::NativeTrainingConfig,
        session: &SessionState,
    ) -> Self {
        let (feature, target) = session.validated_training_columns();
        Self {
            training_cancel: None,
            training_epochs: config.epochs,
            training_learning_rate: config.learning_rate,
            training_validation_fraction: config.validation_fraction,
            training_use_dataset: session.native_training_use_dataset,
            training_feature: feature,
            training_target: target,
        }
    }
}

impl ObjectState {
    pub(crate) fn load(store: &Option<WorkspaceStore>) -> Self {
        Self {
            profiles: store
                .as_ref()
                .and_then(|store| store.load_object_profiles().ok())
                .unwrap_or_default(),
            ..Default::default()
        }
    }
}

impl ExperimentState {
    pub(crate) fn from_session(session: &SessionState) -> Self {
        Self {
            name: session.experiment_name.clone(),
            ..Default::default()
        }
    }
}

impl DatabaseState {
    pub(crate) fn load(store: &Option<WorkspaceStore>) -> Self {
        Self {
            profiles: store
                .as_ref()
                .and_then(|store| store.load_connections().ok())
                .unwrap_or_default(),
            ..Default::default()
        }
    }
}

impl RemoteState {
    pub(crate) fn load(store: &Option<WorkspaceStore>) -> Self {
        Self {
            profiles: store
                .as_ref()
                .and_then(|store| store.load_remote_profiles().ok())
                .unwrap_or_default(),
            ..Default::default()
        }
    }
}
