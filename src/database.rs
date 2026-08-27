use forge_protocol::TableData;
use rusqlite::types::ValueRef;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_PREVIEW_ROWS: usize = 10_000;
const MAX_CLI_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const CLI_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ConnectionKind {
    SQLite,
    DuckDb,
    PostgreSql,
    Adbc,
}
impl ConnectionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::SQLite => "SQLite",
            Self::DuckDb => "DuckDB",
            Self::PostgreSql => "PostgreSQL",
            Self::Adbc => "ADBC driver",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub name: String,
    pub kind: ConnectionKind,
    pub location: String,
    pub username: String,
    pub credential_key: String,
}

pub trait DatabaseConnector {
    fn query(&self, sql: &str) -> Result<TableData, String>;
    fn schema(&self) -> Result<TableData, String>;
}

pub struct ProfileConnector<'a> {
    pub profile: &'a ConnectionProfile,
    pub project_root: &'a Path,
}
impl DatabaseConnector for ProfileConnector<'_> {
    fn query(&self, sql: &str) -> Result<TableData, String> {
        validate_profile(self.profile)?;
        validate_query(sql)?;
        match self.profile.kind {
            ConnectionKind::SQLite => {
                sqlite(&resolve(self.project_root, &self.profile.location), sql)
            }
            ConnectionKind::DuckDb => cli_csv(
                "duckdb",
                &[&self.profile.location, "-csv", "-c", sql],
                None,
                None,
                &self.profile.location,
            ),
            ConnectionKind::PostgreSql => {
                let password = load_secret(&self.profile.credential_key).ok();
                cli_csv(
                    "psql",
                    &[&self.profile.location, "--csv", "-c", sql],
                    password.as_deref(),
                    (!self.profile.username.is_empty()).then_some(self.profile.username.as_str()),
                    &self.profile.location,
                )
            }
            ConnectionKind::Adbc => Err(format!(
                "Install/configure an ADBC driver manager for {}. Core API: {}",
                self.profile.location,
                adbc_marker()
            )),
        }
    }
    fn schema(&self) -> Result<TableData, String> {
        let sql = match self.profile.kind { ConnectionKind::SQLite | ConnectionKind::DuckDb => "SELECT table_name, table_type FROM information_schema.tables ORDER BY table_name", ConnectionKind::PostgreSql => "SELECT table_schema, table_name, table_type FROM information_schema.tables ORDER BY table_schema, table_name", ConnectionKind::Adbc => return Err("ADBC schema discovery requires a configured driver.".into()) };
        self.query(sql).or_else(|_| if self.profile.kind == ConnectionKind::SQLite { self.query("SELECT name, type FROM sqlite_master WHERE type IN ('table','view') ORDER BY name") } else { Err("Schema discovery failed.".into()) })
    }
}

pub fn test_connection(profile: &ConnectionProfile, project_root: &Path) -> Result<String, String> {
    validate_profile(profile)?;
    let connector = ProfileConnector {
        profile,
        project_root,
    };
    let sql = match profile.kind {
        ConnectionKind::SQLite => "SELECT sqlite_version() AS version",
        ConnectionKind::DuckDb => "SELECT version() AS version",
        ConnectionKind::PostgreSql => "SELECT version() AS version",
        ConnectionKind::Adbc => {
            return Err(format!(
                "ADBC core is available as `{}`; install a concrete driver manager to test this profile.",
                adbc_marker()
            ))
        }
    };
    let result = connector.query(sql)?;
    let version = result
        .rows
        .first()
        .and_then(|row| row.first())
        .map(String::as_str)
        .unwrap_or("version unavailable");
    Ok(format!(
        "{} connection succeeded: {version}",
        profile.kind.label()
    ))
}

fn resolve(root: &Path, location: &str) -> PathBuf {
    let path = PathBuf::from(location);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn sqlite(path: &Path, sql: &str) -> Result<TableData, String> {
    let connection = rusqlite::Connection::open(path).map_err(|e| e.to_string())?;
    let mut statement = connection.prepare(sql).map_err(|e| e.to_string())?;
    let columns = statement
        .column_names()
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let count = columns.len();
    let rows = statement
        .query_map([], |row| {
            Ok((0..count)
                .map(|index| match row.get_ref(index).unwrap_or(ValueRef::Null) {
                    ValueRef::Null => String::new(),
                    ValueRef::Integer(v) => v.to_string(),
                    ValueRef::Real(v) => v.to_string(),
                    ValueRef::Text(v) => String::from_utf8_lossy(v).into(),
                    ValueRef::Blob(v) => format!("<{} bytes>", v.len()),
                })
                .collect::<Vec<_>>())
        })
        .map_err(|e| e.to_string())?
        .take(MAX_PREVIEW_ROWS)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(TableData { columns, rows })
}

fn cli_csv(
    program: &str,
    args: &[&str],
    password: Option<&str>,
    username: Option<&str>,
    sensitive_location: &str,
) -> Result<TableData, String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(password) = password {
        command.env("PGPASSWORD", password);
    }
    if let Some(username) = username {
        command.env("PGUSER", username);
    }
    let output = command_output(command, program)?;
    if !output.status.success() {
        return Err(redact_error(
            String::from_utf8_lossy(&output.stderr).trim(),
            sensitive_location,
            password,
        ));
    }
    if output.stdout.len() > MAX_CLI_OUTPUT_BYTES {
        return Err(format!(
            "{program} returned more than {} MiB; narrow the query before loading a preview.",
            MAX_CLI_OUTPUT_BYTES / (1024 * 1024)
        ));
    }
    let mut reader = csv::Reader::from_reader(output.stdout.as_slice());
    let columns = reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(str::to_owned)
        .collect();
    let rows = reader
        .records()
        .map(|row| {
            row.map(|row| row.iter().map(str::to_owned).collect())
                .map_err(|e| e.to_string())
        })
        .take(MAX_PREVIEW_ROWS)
        .collect::<Result<_, _>>()?;
    Ok(TableData { columns, rows })
}

fn command_output(mut command: Command, program: &str) -> Result<std::process::Output, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("{program} is not available: {e}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or("Could not capture database output")?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or("Could not capture database errors")?;
    let stdout_reader =
        std::thread::spawn(move || read_bounded(&mut stdout, MAX_CLI_OUTPUT_BYTES + 1));
    let stderr_reader = std::thread::spawn(move || read_bounded(&mut stderr, 1024 * 1024));
    let started = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if started.elapsed() >= CLI_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!(
                    "{program} exceeded the {} second database command timeout",
                    CLI_TIMEOUT.as_secs()
                ));
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "Database output reader failed".to_owned())?
        .map_err(|e| e.to_string())?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "Database error reader failed".to_owned())?
        .map_err(|e| e.to_string())?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn read_bounded(reader: &mut impl std::io::Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut stored = Vec::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(stored.len());
        stored.extend_from_slice(&chunk[..read.min(remaining)]);
    }
    Ok(stored)
}

pub fn validate_profile(profile: &ConnectionProfile) -> Result<(), String> {
    if profile.name.trim().is_empty() {
        return Err("Connection profile name cannot be empty.".into());
    }
    if profile.location.trim().is_empty() {
        return Err("Connection location cannot be empty.".into());
    }
    if profile.location.contains('\0') || profile.username.contains('\0') {
        return Err("Connection fields may not contain NUL bytes.".into());
    }
    if profile.kind == ConnectionKind::PostgreSql {
        let lower = profile.location.to_ascii_lowercase();
        let url_password = url::Url::parse(&profile.location)
            .ok()
            .is_some_and(|url| !url.password().unwrap_or_default().is_empty());
        if url_password || lower.contains("password=") {
            return Err(
                "Do not place PostgreSQL passwords in the connection string; use Forge's credential field and OS credential store."
                    .into(),
            );
        }
    }
    Ok(())
}

fn redact_error(message: &str, location: &str, password: Option<&str>) -> String {
    let mut safe = if location.is_empty() {
        message.to_owned()
    } else {
        message.replace(location, "<connection>")
    };
    if let Some(password) = password.filter(|value| !value.is_empty()) {
        safe = safe.replace(password, "<redacted>");
    }
    for marker in ["password=", "pwd="] {
        let mut search_from = 0;
        while let Some(relative) = safe[search_from..].to_ascii_lowercase().find(marker) {
            let start = search_from + relative;
            let value_start = start + marker.len();
            let value_end = safe[value_start..]
                .find(|character: char| character.is_whitespace() || character == ';')
                .map(|offset| value_start + offset)
                .unwrap_or(safe.len());
            safe.replace_range(value_start..value_end, "<redacted>");
            search_from = value_start + "<redacted>".len();
        }
    }
    safe
}

fn validate_query(sql: &str) -> Result<(), String> {
    if sql.trim().is_empty() {
        return Err("Enter a SQL statement first.".into());
    }
    if sql.contains('\0') {
        return Err("SQL may not contain NUL bytes.".into());
    }
    if sql.len() > 1_000_000 {
        return Err("SQL statements are limited to 1 MB.".into());
    }
    Ok(())
}

pub fn store_secret(key: &str, secret: &str) -> Result<(), String> {
    keyring::Entry::new("forge-ml", key)
        .map_err(|e| e.to_string())?
        .set_password(secret)
        .map_err(|e| e.to_string())
}
pub fn load_secret(key: &str) -> Result<String, String> {
    keyring::Entry::new("forge-ml", key)
        .map_err(|e| e.to_string())?
        .get_password()
        .map_err(|e| e.to_string())
}
#[cfg(feature = "adbc")]
pub fn adbc_marker() -> &'static str {
    std::any::type_name::<adbc_core::error::Error>()
}
#[cfg(not(feature = "adbc"))]
pub fn adbc_marker() -> &'static str {
    "optional feature disabled"
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queries_sqlite_to_table() {
        let path = std::env::temp_dir().join("forge-db-test.sqlite");
        let _ = std::fs::remove_file(&path);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "create table sample(x integer, y text); insert into sample values(1,'a');",
        )
        .unwrap();
        drop(db);
        let table = sqlite(&path, "select * from sample").unwrap();
        assert_eq!(table.rows[0], ["1", "a"]);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn rejects_empty_and_oversized_queries() {
        assert!(validate_query("").is_err());
        assert!(validate_query(&"x".repeat(1_000_001)).is_err());
    }

    #[test]
    fn rejects_plaintext_postgres_passwords() {
        let base = ConnectionProfile {
            name: "warehouse".into(),
            kind: ConnectionKind::PostgreSql,
            location: "postgresql://user:secret@db.example/data".into(),
            username: "user".into(),
            credential_key: "test".into(),
        };
        assert!(validate_profile(&base).unwrap_err().contains("credential"));
        let mut keyword = base;
        keyword.location = "host=db.example password=secret dbname=data".into();
        assert!(validate_profile(&keyword).is_err());
    }

    #[test]
    fn database_errors_redact_locations_and_secrets() {
        let safe = redact_error(
            "could not open postgresql://db password=secret pwd=second",
            "postgresql://db",
            Some("secret"),
        );
        assert!(!safe.contains("postgresql://db"));
        assert!(!safe.contains("secret"));
        assert!(!safe.contains("second"));
        assert!(safe.contains("<connection>"));
        assert!(safe.contains("<redacted>"));
    }

    #[test]
    fn bounded_reader_drains_but_caps_stored_bytes() {
        let mut source = std::io::Cursor::new(vec![7_u8; 100]);
        assert_eq!(read_bounded(&mut source, 10).unwrap().len(), 10);
        assert_eq!(source.position(), 100);
    }

    #[test]
    fn tests_sqlite_connection_without_mutating_data() {
        let path = std::env::temp_dir().join("forge-db-probe.sqlite");
        let _ = std::fs::remove_file(&path);
        rusqlite::Connection::open(&path).unwrap();
        let profile = ConnectionProfile {
            name: "probe".into(),
            kind: ConnectionKind::SQLite,
            location: path.display().to_string(),
            username: String::new(),
            credential_key: "probe".into(),
        };
        assert!(test_connection(&profile, Path::new("."))
            .unwrap()
            .contains("succeeded"));
        let _ = std::fs::remove_file(path);
    }
}
