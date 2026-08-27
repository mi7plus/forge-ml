use forge_protocol::TableData;
use rusqlite::types::ValueRef;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

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
        match self.profile.kind {
            ConnectionKind::SQLite => {
                sqlite(&resolve(self.project_root, &self.profile.location), sql)
            }
            ConnectionKind::DuckDb => cli_csv(
                "duckdb",
                &[&self.profile.location, "-csv", "-c", sql],
                None,
                None,
            ),
            ConnectionKind::PostgreSql => {
                let password = load_secret(&self.profile.credential_key).ok();
                cli_csv(
                    "psql",
                    &[&self.profile.location, "--csv", "-c", sql],
                    password.as_deref(),
                    (!self.profile.username.is_empty()).then_some(self.profile.username.as_str()),
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
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(TableData { columns, rows })
}

fn cli_csv(
    program: &str,
    args: &[&str],
    password: Option<&str>,
    username: Option<&str>,
) -> Result<TableData, String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(password) = password {
        command.env("PGPASSWORD", password);
    }
    if let Some(username) = username {
        command.env("PGUSER", username);
    }
    let output = command
        .output()
        .map_err(|e| format!("{program} is not available: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().into());
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
        .collect::<Result<_, _>>()?;
    Ok(TableData { columns, rows })
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
}
