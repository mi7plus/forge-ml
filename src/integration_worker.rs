use crate::{
    data::Dataset,
    database::{self, ConnectionProfile, DatabaseConnector, ProfileConnector},
    object_storage::ObjectProfile,
};
use arrow::record_batch::RecordBatch;
use forge_protocol::TableData;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

pub enum Request {
    DataImport(PathBuf),
    MillwrightImport(PathBuf),
    DataExport {
        name: String,
        batch: RecordBatch,
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
}

pub enum ResultEvent {
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
}

pub struct IntegrationWorker {
    sender: Sender<Request>,
    receiver: Receiver<ResultEvent>,
}

impl IntegrationWorker {
    pub fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                let result = execute(request);
                if result_tx.send(result).is_err() {
                    break;
                }
            }
        });
        Self {
            sender: request_tx,
            receiver: result_rx,
        }
    }

    pub fn submit(&self, request: Request) -> Result<(), String> {
        self.sender.send(request).map_err(|error| error.to_string())
    }

    pub fn try_recv(&self) -> Option<ResultEvent> {
        self.receiver.try_recv().ok()
    }

    #[cfg(test)]
    fn recv_timeout(&self, timeout: std::time::Duration) -> Option<ResultEvent> {
        self.receiver.recv_timeout(timeout).ok()
    }
}

fn execute(request: Request) -> ResultEvent {
    match request {
        Request::DataImport(path) => ResultEvent::DataImport {
            result: prepared_import(crate::data::load_table(&path)),
            path,
        },
        Request::MillwrightImport(path) => ResultEvent::DataImport {
            result: prepared_import(crate::data::load_millwright_table(&path)),
            path,
        },
        Request::DataExport {
            name,
            batch,
            path,
            format,
        } => ResultEvent::DataExport {
            result: crate::export::dataset_batch(&batch, &path, format),
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
                batch: dataset.batch,
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
