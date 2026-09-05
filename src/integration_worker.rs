use crate::{
    data::Dataset,
    database::{self, ConnectionProfile, DatabaseConnector, ProfileConnector},
    object_storage::ObjectProfile,
};
use arrow::record_batch::RecordBatch;
use forge_protocol::TableData;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
};

pub enum Request {
    BurnTraining {
        backend: crate::deep_learning::Backend,
        config: crate::deep_learning::NativeTrainingConfig,
        data: Option<crate::deep_learning::NativeTrainingData>,
        cancelled: Arc<AtomicBool>,
    },
    NativeRegressionPredict {
        artifact: crate::deep_learning::NativeRegressionArtifact,
        dataset_name: String,
        table: std::sync::Arc<TableData>,
        drift_policy: crate::deep_learning::DriftPolicy,
    },
    DataImport(PathBuf),
    MillwrightImport(PathBuf),
    DataExport {
        name: String,
        batches: Vec<RecordBatch>,
        path: PathBuf,
        format: crate::export::DataFormat,
    },
    DatabaseTest {
        profile: ConnectionProfile,
        root: PathBuf,
    },
    DatabaseSchema {
        profile: ConnectionProfile,
        root: PathBuf,
        dataset_name: String,
    },
    DatabaseQuery {
        profile: ConnectionProfile,
        root: PathBuf,
        dataset_name: String,
        sql: String,
    },
    ObjectTest(ObjectProfile),
    ObjectList {
        profile: ObjectProfile,
        limit: usize,
    },
    ObjectDownload {
        profile: ObjectProfile,
        key: String,
        root: PathBuf,
    },
    RemoteTest(crate::remote::RemoteProfile),
    RemoteKernelStart {
        profile: crate::remote::RemoteProfile,
        kernel_name: String,
    },
    RemoteKernelStop(crate::remote::RemoteKernelSession),
    RemoteKernelInterrupt(crate::remote::RemoteKernelSession),
    RemoteExecute {
        session: crate::remote::RemoteKernelSession,
        code: String,
        cell_id: Option<usize>,
        input: Receiver<String>,
    },
}

pub enum ResultEvent {
    BurnTrainingProgress(crate::millwright_studio::TrainingEvent),
    BurnTrainingFinished(Result<crate::deep_learning::NativeTrainingOutcome, String>),
    NativeRegressionPredicted(
        Result<
            (
                String,
                Dataset,
                usize,
                Option<crate::deep_learning::RegressionDiagnostics>,
                crate::deep_learning::FeatureDriftDiagnostics,
            ),
            String,
        >,
    ),
    DataImportProgress {
        path: PathBuf,
        rows: usize,
    },
    DataImport {
        path: PathBuf,
        result: Result<(String, Dataset), String>,
    },
    DataExport {
        name: String,
        path: PathBuf,
        result: Result<(), String>,
    },
    DatabaseMessage(Result<String, String>),
    DatabaseTable {
        dataset_name: String,
        query: Option<String>,
        result: Result<Dataset, String>,
    },
    ObjectMessage(Result<String, String>),
    ObjectDownload(Result<PathBuf, String>),
    RemoteMessage(Result<String, String>),
    RemoteKernelStarted(Result<crate::remote::RemoteKernelSession, String>),
    RemoteKernelStopped(Result<String, String>),
    RemoteKernelInterrupted(Result<String, String>),
    RemoteInputRequested {
        cell_id: Option<usize>,
        prompt: String,
        password: bool,
    },
    RemoteExecuted {
        cell_id: Option<usize>,
        result: Result<crate::remote::RemoteExecution, String>,
    },
}

pub struct IntegrationWorker {
    sender: Sender<Request>,
    control_sender: Sender<Request>,
    receiver: Receiver<ResultEvent>,
}

impl IntegrationWorker {
    pub fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let (control_tx, control_rx) = mpsc::channel();

        // A small pool drains the request channel so a long operation (a big
        // import or export) doesn't block a quick one (a query) queued behind
        // it. The lock is held only around `recv`, never during `execute`, so
        // the workers run concurrently. Each request is still handled start to
        // finish by a single thread, so a run's progress events stay ordered;
        // only unrelated requests can now interleave, which nothing relies on.
        let pool = thread::available_parallelism()
            .map(|n| n.get().clamp(2, 4))
            .unwrap_or(2);
        let shared_rx = Arc::new(Mutex::new(request_rx));
        for _ in 0..pool {
            let rx = Arc::clone(&shared_rx);
            let tx = result_tx.clone();
            thread::spawn(move || loop {
                let request = {
                    let guard = match rx.lock() {
                        Ok(guard) => guard,
                        Err(_) => break, // a panicked peer poisoned the lock
                    };
                    guard.recv()
                };
                match request {
                    Ok(request) => {
                        let result = execute(request, &tx);
                        if tx.send(result).is_err() {
                            break;
                        }
                    }
                    Err(_) => break, // all senders dropped
                }
            });
        }

        // Interrupts run on a dedicated thread so a cancel is never stuck behind
        // in-flight work in the pool.
        let control_result_tx = result_tx;
        thread::spawn(move || {
            while let Ok(request) = control_rx.recv() {
                let result = execute(request, &control_result_tx);
                if control_result_tx.send(result).is_err() {
                    break;
                }
            }
        });
        Self {
            sender: request_tx,
            control_sender: control_tx,
            receiver: result_rx,
        }
    }

    pub fn submit(&self, request: Request) -> Result<(), String> {
        let sender = if matches!(request, Request::RemoteKernelInterrupt(_)) {
            &self.control_sender
        } else {
            &self.sender
        };
        sender.send(request).map_err(|error| error.to_string())
    }

    pub fn try_recv(&self) -> Option<ResultEvent> {
        self.receiver.try_recv().ok()
    }

    #[cfg(test)]
    fn recv_timeout(&self, timeout: std::time::Duration) -> Option<ResultEvent> {
        self.receiver.recv_timeout(timeout).ok()
    }
}

fn execute(request: Request, events: &Sender<ResultEvent>) -> ResultEvent {
    match request {
        Request::BurnTraining {
            backend,
            config,
            data,
            cancelled,
        } => {
            let result = crate::deep_learning::native_burn_training_demo_with_progress(
                backend,
                config,
                data,
                || cancelled.load(Ordering::Relaxed),
                |event| {
                    let _ = events.send(ResultEvent::BurnTrainingProgress(event));
                },
            );
            ResultEvent::BurnTrainingFinished(result)
        }
        Request::NativeRegressionPredict {
            artifact,
            dataset_name,
            table,
            drift_policy,
        } => {
            let result = crate::deep_learning::native_regression_predictions(
                &artifact,
                &dataset_name,
                &table,
                drift_policy,
            )
            .and_then(|outcome| {
                prepared_dataset(outcome.table, format!("native model {}", artifact.run_id)).map(
                    |dataset| {
                        (
                            outcome.name,
                            dataset,
                            outcome.predicted,
                            outcome.diagnostics,
                            outcome.drift,
                        )
                    },
                )
            });
            ResultEvent::NativeRegressionPredicted(result)
        }
        Request::DataImport(path) => {
            let progress_path = path.clone();
            let result = crate::data::load_table_with_progress(&path, |rows| {
                let _ = events.send(ResultEvent::DataImportProgress {
                    path: progress_path.clone(),
                    rows,
                });
            });
            ResultEvent::DataImport {
                result: prepared_import(result),
                path,
            }
        }
        Request::MillwrightImport(path) => ResultEvent::DataImport {
            result: prepared_import(crate::data::load_millwright_table(&path)),
            path,
        },
        Request::DataExport {
            name,
            batches,
            path,
            format,
        } => ResultEvent::DataExport {
            result: crate::export::dataset_batches(&batches, &path, format),
            name,
            path,
        },
        Request::DatabaseTest { profile, root } => {
            ResultEvent::DatabaseMessage(database::test_connection(&profile, &root))
        }
        Request::DatabaseSchema {
            profile,
            root,
            dataset_name,
        } => {
            let source = format!("{} schema", profile.name);
            let result = ProfileConnector {
                profile: &profile,
                project_root: &root,
            }
            .schema()
            .and_then(|table| prepared_dataset(table, source));
            ResultEvent::DatabaseTable {
                dataset_name,
                query: None,
                result,
            }
        }
        Request::DatabaseQuery {
            profile,
            root,
            dataset_name,
            sql,
        } => {
            let source = format!("{} SQL", profile.name);
            let result = ProfileConnector {
                profile: &profile,
                project_root: &root,
            }
            .query(&sql)
            .and_then(|table| prepared_dataset(table, source));
            ResultEvent::DatabaseTable {
                dataset_name,
                query: Some(sql),
                result,
            }
        }
        Request::ObjectTest(profile) => ResultEvent::ObjectMessage(profile.test()),
        Request::ObjectList { profile, limit } => ResultEvent::ObjectMessage(profile.list(limit)),
        Request::ObjectDownload { profile, key, root } => {
            ResultEvent::ObjectDownload(profile.download(&key, &root))
        }
        Request::RemoteTest(profile) => {
            ResultEvent::RemoteMessage(crate::remote::test_jupyter(&profile))
        }
        Request::RemoteKernelStart {
            profile,
            kernel_name,
        } => ResultEvent::RemoteKernelStarted(crate::remote::start_kernel(&profile, &kernel_name)),
        Request::RemoteKernelStop(session) => {
            ResultEvent::RemoteKernelStopped(crate::remote::stop_kernel(&session))
        }
        Request::RemoteKernelInterrupt(session) => {
            ResultEvent::RemoteKernelInterrupted(crate::remote::interrupt_kernel(&session))
        }
        Request::RemoteExecute {
            session,
            code,
            cell_id,
            input,
        } => ResultEvent::RemoteExecuted {
            cell_id,
            result: crate::remote::execute(&session, &code, &input, |request| {
                events
                    .send(ResultEvent::RemoteInputRequested {
                        cell_id,
                        prompt: request.prompt,
                        password: request.password,
                    })
                    .map_err(|error| error.to_string())
            }),
        },
    }
}

fn prepared_import(
    result: Result<(String, TableData, String), String>,
) -> Result<(String, Dataset), String> {
    result.and_then(|(name, table, source)| {
        prepared_dataset(table, source).map(|dataset| (name, dataset))
    })
}

fn prepared_dataset(table: TableData, source: String) -> Result<Dataset, String> {
    let dataset = Dataset::from_table(table, Some(source))?;
    dataset.prepare_quality();
    Ok(dataset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::ConnectionKind;

    #[test]
    fn worker_streams_and_completes_embedded_burn_training() {
        let worker = IntegrationWorker::new();
        worker
            .submit(Request::BurnTraining {
                backend: crate::deep_learning::Backend::Cpu,
                config: crate::deep_learning::NativeTrainingConfig::default(),
                data: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            })
            .unwrap();
        let mut progress = 0;
        loop {
            match worker
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("Burn worker result")
            {
                ResultEvent::BurnTrainingProgress(_) => progress += 1,
                ResultEvent::BurnTrainingFinished(result) => {
                    assert_eq!(result.unwrap().events.len(), progress);
                    break;
                }
                _ => panic!("unexpected worker result"),
            }
        }
        assert_eq!(progress, 84);
    }

    #[test]
    fn worker_materializes_native_regression_predictions() {
        let worker = IntegrationWorker::new();
        let artifact = crate::deep_learning::NativeRegressionArtifact {
            schema: 1,
            run_id: "burn-flex-worker".into(),
            dataset: "source".into(),
            feature: "x".into(),
            target: "y".into(),
            slope: 2.0,
            intercept: 1.0,
            feature_mean: 0.0,
            feature_scale: 1.0,
            target_mean: 0.0,
            target_scale: 1.0,
            best_score: -0.1,
            epochs_completed: 2,
            ..Default::default()
        };
        worker
            .submit(Request::NativeRegressionPredict {
                artifact,
                dataset_name: "source".into(),
                table: Arc::new(TableData {
                    columns: vec!["x".into()],
                    rows: vec![vec!["2".into()], vec!["missing".into()]],
                }),
                drift_policy: Default::default(),
            })
            .unwrap();
        match worker
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("prediction worker result")
        {
            ResultEvent::NativeRegressionPredicted(Ok((
                name,
                dataset,
                predicted,
                diagnostics,
                drift,
            ))) => {
                assert_eq!(name, "source_predictions");
                assert_eq!(predicted, 1);
                assert!(diagnostics.is_none());
                assert_eq!(drift.observed, 1);
                assert!(drift.breached);
                assert_eq!(dataset.rows[0][1], "5");
                assert_eq!(dataset.rows[1][1], "");
            }
            _ => panic!("unexpected worker result"),
        }
    }

    #[test]
    fn worker_imports_csv_without_mutating_the_workspace() {
        let root = std::env::temp_dir().join(format!("forge-import-worker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("sample.csv");
        std::fs::write(&path, "value,label\n42,answer\n").unwrap();
        let worker = IntegrationWorker::new();
        worker.submit(Request::DataImport(path.clone())).unwrap();
        match worker
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
        {
            ResultEvent::DataImportProgress { path: actual, rows } => {
                assert_eq!(actual, path);
                assert_eq!(rows, 1);
            }
            _ => panic!("expected import progress"),
        }
        match worker
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
        {
            ResultEvent::DataImport {
                path: actual,
                result,
            } => {
                let (name, dataset) = result.unwrap();
                assert_eq!(actual, path);
                assert_eq!(name, "sample");
                assert_eq!(dataset.rows, [vec!["42", "answer"]]);
                assert!(dataset.source.as_deref().unwrap().ends_with("sample.csv"));
                assert_eq!(dataset.profile()[0].numeric_count, 1);
            }
            _ => panic!("unexpected integration result"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn worker_exports_record_batches_atomically() {
        let root = std::env::temp_dir().join(format!("forge-export-worker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("result.csv");
        std::fs::write(&path, "old").unwrap();
        let dataset = crate::data::Dataset::from_table(
            TableData {
                columns: vec!["value".into()],
                rows: vec![vec!["42".into()]],
            },
            None,
        )
        .unwrap();
        let worker = IntegrationWorker::new();
        worker
            .submit(Request::DataExport {
                name: "result".into(),
                batches: dataset.batches,
                path: path.clone(),
                format: crate::export::DataFormat::Csv,
            })
            .unwrap();
        match worker
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
        {
            ResultEvent::DataExport { result, .. } => result.unwrap(),
            _ => panic!("unexpected integration result"),
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "value\n42\n");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn worker_imports_through_published_millwright() {
        let root =
            std::env::temp_dir().join(format!("forge-millwright-worker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("native.csv");
        std::fs::write(&path, "value\n42\n").unwrap();
        let worker = IntegrationWorker::new();
        worker
            .submit(Request::MillwrightImport(path.clone()))
            .unwrap();
        match worker
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
        {
            ResultEvent::DataImport { result, .. } => {
                let (name, dataset) = result.unwrap();
                assert_eq!(name, "native");
                assert_eq!(dataset.rows, [vec!["42"]]);
                assert!(dataset
                    .source
                    .as_deref()
                    .unwrap()
                    .contains("published Millwright 2.2.1"));
                assert_eq!(dataset.profile()[0].numeric_count, 1);
            }
            _ => panic!("unexpected integration result"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn worker_pool_drains_multiple_concurrent_requests() {
        // Submit several exports at once and confirm they all come back, in any
        // order — the pool must not serialize or drop requests.
        let root = std::env::temp_dir().join(format!("forge-pool-worker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let worker = IntegrationWorker::new();
        let count = 5;
        for i in 0..count {
            let dataset = crate::data::Dataset::from_table(
                TableData {
                    columns: vec!["value".into()],
                    rows: vec![vec![i.to_string()]],
                },
                None,
            )
            .unwrap();
            worker
                .submit(Request::DataExport {
                    name: format!("r{i}"),
                    batches: dataset.batches,
                    path: root.join(format!("r{i}.csv")),
                    format: crate::export::DataFormat::Csv,
                })
                .unwrap();
        }
        let mut done = 0;
        while done < count {
            match worker
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("export result")
            {
                ResultEvent::DataExport { result, .. } => {
                    result.unwrap();
                    done += 1;
                }
                _ => panic!("unexpected worker result"),
            }
        }
        assert_eq!(done, count);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn worker_returns_typed_sqlite_tables() {
        let root = std::env::temp_dir().join(format!("forge-worker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let database = root.join("data.sqlite");
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute_batch("create table sample(value integer); insert into sample values (42);")
            .unwrap();
        drop(connection);
        let profile = ConnectionProfile {
            name: "local".into(),
            kind: ConnectionKind::SQLite,
            location: database.display().to_string(),
            username: String::new(),
            credential_key: "worker-test".into(),
        };
        let worker = IntegrationWorker::new();
        worker
            .submit(Request::DatabaseQuery {
                profile,
                root: root.clone(),
                dataset_name: "result".into(),
                sql: "select * from sample".into(),
            })
            .unwrap();
        let result = worker
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        match result {
            ResultEvent::DatabaseTable { result, .. } => {
                let dataset = result.unwrap();
                assert_eq!(dataset.rows, [vec!["42"]]);
                assert_eq!(dataset.profile()[0].numeric_count, 1);
            }
            _ => panic!("unexpected integration result"),
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
