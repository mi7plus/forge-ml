use crate::{
    database::{self, ConnectionProfile, DatabaseConnector, ProfileConnector},
    object_storage::ObjectProfile,
};
use forge_protocol::TableData;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

pub enum Request {
    DataImport(PathBuf),
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
}

pub enum ResultEvent {
    DataImport {
        path: PathBuf,
        result: Result<(String, TableData, String), String>,
    },
    DatabaseMessage(Result<String, String>),
    DatabaseTable {
        dataset_name: String,
        source: String,
        query: Option<String>,
        result: Result<TableData, String>,
    },
    ObjectMessage(Result<String, String>),
    ObjectDownload(Result<PathBuf, String>),
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
            result: crate::data::load_table(&path),
            path,
        },
        Request::DatabaseTest { profile, root } => {
            ResultEvent::DatabaseMessage(database::test_connection(&profile, &root))
        }
        Request::DatabaseSchema {
            profile,
            root,
            dataset_name,
        } => ResultEvent::DatabaseTable {
            dataset_name,
            source: format!("{} schema", profile.name),
            query: None,
            result: ProfileConnector {
                profile: &profile,
                project_root: &root,
            }
            .schema(),
        },
        Request::DatabaseQuery {
            profile,
            root,
            dataset_name,
            sql,
        } => ResultEvent::DatabaseTable {
            dataset_name,
            source: format!("{} SQL", profile.name),
            query: Some(sql.clone()),
            result: ProfileConnector {
                profile: &profile,
                project_root: &root,
            }
            .query(&sql),
        },
        Request::ObjectTest(profile) => ResultEvent::ObjectMessage(profile.test()),
        Request::ObjectList { profile, limit } => ResultEvent::ObjectMessage(profile.list(limit)),
        Request::ObjectDownload { profile, key, root } => {
            ResultEvent::ObjectDownload(profile.download(&key, &root))
        }
    }
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
                let (name, table, source) = result.unwrap();
                assert_eq!(actual, path);
                assert_eq!(name, "sample");
                assert_eq!(table.rows, [vec!["42", "answer"]]);
                assert!(source.ends_with("sample.csv"));
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
                assert_eq!(result.unwrap().rows, [vec!["42"]]);
            }
            _ => panic!("unexpected integration result"),
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
