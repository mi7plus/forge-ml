use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub line: u64,
    pub column: u64,
    pub severity: u64,
    pub message: String,
}

/// A source location returned by a references request.
#[derive(Debug, Clone)]
pub struct Reference {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
}

pub enum LspCommand {
    Install,
    Sync {
        root: PathBuf,
        path: PathBuf,
        text: String,
        version: i32,
    },
    Complete {
        path: PathBuf,
        text: String,
        char_offset: usize,
    },
    Hover {
        path: PathBuf,
        text: String,
        char_offset: usize,
    },
    Definition {
        path: PathBuf,
        text: String,
        char_offset: usize,
    },
    ProbeDefinition {
        path: PathBuf,
        text: String,
        char_offset: usize,
    },
    References {
        path: PathBuf,
        text: String,
        char_offset: usize,
    },
    SignatureHelp {
        path: PathBuf,
        text: String,
        char_offset: usize,
    },
}

#[derive(Debug)]
pub enum LspEvent {
    Status(String),
    Diagnostics {
        path: PathBuf,
        items: Vec<Diagnostic>,
    },
    Completions(Vec<String>),
    Hover(String),
    Definition {
        path: PathBuf,
        line: usize,
    },
    DefinitionProbe {
        char_offset: usize,
        navigable: bool,
    },
    References(Vec<Reference>),
    Signature(String),
    Installed(bool),
}

pub struct LspHandle {
    commands: Sender<LspCommand>,
    events: Receiver<LspEvent>,
}

impl LspHandle {
    pub fn spawn() -> Self {
        let (commands, command_rx) = channel();
        let (event_tx, events) = channel();
        thread::spawn(move || worker(command_rx, event_tx));
        Self { commands, events }
    }
    pub fn send(&self, command: LspCommand) {
        let _ = self.commands.send(command);
    }
    pub fn try_recv(&self) -> Option<LspEvent> {
        self.events.try_recv().ok()
    }
    pub fn install(&self) {
        let _ = self.commands.send(LspCommand::Install);
    }
}

#[derive(Clone, Copy)]
enum Pending {
    Completion,
    Hover,
    Definition,
    ProbeDefinition(usize),
    References,
    Signature,
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    incoming: Receiver<Value>,
    root: PathBuf,
    next_id: u64,
    pending: HashMap<u64, Pending>,
    open_versions: HashMap<PathBuf, i32>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn worker(commands: Receiver<LspCommand>, events: Sender<LspEvent>) {
    let mut server: Option<Server> = None;
    loop {
        if let Some(active) = &mut server {
            while let Ok(message) = active.incoming.try_recv() {
                handle_message(active, message, &events);
            }
        }
        match commands.recv_timeout(Duration::from_millis(30)) {
            Ok(command) => {
                if matches!(command, LspCommand::Install) {
                    let _ = events.send(LspEvent::Status(
                        "Installing rust-analyzer and rust-src...".to_owned(),
                    ));
                    let result = Command::new("rustup")
                        .args(["component", "add", "rust-analyzer", "rust-src"])
                        .output();
                    let success = result.as_ref().is_ok_and(|output| output.status.success());
                    let detail = result
                        .ok()
                        .map(|output| String::from_utf8_lossy(&output.stderr).trim().to_owned())
                        .unwrap_or_default();
                    let _ = events.send(LspEvent::Status(if success {
                        "rust-analyzer installed. Language services are ready.".to_owned()
                    } else {
                        format!("Could not install rust-analyzer. {detail}")
                    }));
                    let _ = events.send(LspEvent::Installed(success));
                    continue;
                }
                let root = match &command {
                    LspCommand::Sync { root, .. } => Some(root.clone()),
                    _ => None,
                };
                if let Some(root) = root {
                    if server.as_ref().is_none_or(|active| active.root != root) {
                        server = start_server(root, &events).ok();
                    }
                }
                if let Some(active) = &mut server {
                    dispatch(active, command, &events);
                } else {
                    if let LspCommand::ProbeDefinition { char_offset, .. } = command {
                        let _ = events.send(LspEvent::DefinitionProbe {
                            char_offset,
                            navigable: false,
                        });
                    }
                    let _ = events.send(LspEvent::Status("rust-analyzer unavailable. Install it with `rustup component add rust-analyzer`.".to_owned()));
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn start_server(root: PathBuf, events: &Sender<LspEvent>) -> std::io::Result<Server> {
    let binary = resolve_binary();
    let version = Command::new(&binary).arg("--version").output()?;
    if !version.status.success() {
        let detail = String::from_utf8_lossy(&version.stderr).trim().to_owned();
        let _ = events.send(LspEvent::Status(format!(
            "rust-analyzer is not installed. Run `rustup component add rust-analyzer`. {detail}"
        )));
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "rust-analyzer component missing",
        ));
    }
    let mut child = Command::new(binary)
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let (incoming_tx, incoming) = channel();
    thread::spawn(move || read_messages(stdout, incoming_tx));
    let mut server = Server {
        child,
        stdin,
        incoming,
        root: root.clone(),
        next_id: 2,
        pending: HashMap::new(),
        open_versions: HashMap::new(),
    };
    let root_uri = file_uri(&root);
    send(
        &mut server.stdin,
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":std::process::id(),"rootUri":root_uri,"capabilities":{"textDocument":{"completion":{"completionItem":{"snippetSupport":false}},"hover":{},"definition":{},"references":{},"signatureHelp":{},"publishDiagnostics":{}}},"clientInfo":{"name":"forge-ml","version":env!("CARGO_PKG_VERSION")}}}),
    )?;
    send(
        &mut server.stdin,
        &json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
    )?;
    let _ = events.send(LspEvent::Status(format!(
        "rust-analyzer starting for {}",
        root.display()
    )));
    Ok(server)
}

fn dispatch(server: &mut Server, command: LspCommand, events: &Sender<LspEvent>) {
    let result = match command {
        LspCommand::Sync {
            path,
            text,
            version,
            ..
        } => sync(server, &path, text, version),
        LspCommand::Complete {
            path,
            text,
            char_offset,
        } => request_at(
            server,
            Pending::Completion,
            "textDocument/completion",
            &path,
            &text,
            char_offset,
        ),
        LspCommand::Hover {
            path,
            text,
            char_offset,
        } => request_at(
            server,
            Pending::Hover,
            "textDocument/hover",
            &path,
            &text,
            char_offset,
        ),
        LspCommand::Definition {
            path,
            text,
            char_offset,
        } => request_at(
            server,
            Pending::Definition,
            "textDocument/definition",
            &path,
            &text,
            char_offset,
        ),
        LspCommand::ProbeDefinition {
            path,
            text,
            char_offset,
        } => request_at(
            server,
            Pending::ProbeDefinition(char_offset),
            "textDocument/definition",
            &path,
            &text,
            char_offset,
        ),
        LspCommand::SignatureHelp {
            path,
            text,
            char_offset,
        } => request_at(
            server,
            Pending::Signature,
            "textDocument/signatureHelp",
            &path,
            &text,
            char_offset,
        ),
        LspCommand::References {
            path,
            text,
            char_offset,
        } => request_references(server, &path, &text, char_offset),
        LspCommand::Install => Ok(()),
    };
    if let Err(error) = result {
        let _ = events.send(LspEvent::Status(format!(
            "rust-analyzer transport error: {error}"
        )));
    }
}

fn resolve_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("FORGE_RUST_ANALYZER")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return path;
    }
    let name = if cfg!(windows) {
        "rust-analyzer.exe"
    } else {
        "rust-analyzer"
    };
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            for candidate in [
                parent.join(name),
                parent.join("resources").join(name),
                parent.join("..").join("Resources").join(name),
                PathBuf::from("/usr/lib").join(name),
                PathBuf::from("/usr/lib/forge-ml").join(name),
                PathBuf::from("/usr/lib/forge_ide").join(name),
            ] {
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    PathBuf::from(name)
}

fn sync(server: &mut Server, path: &Path, text: String, version: i32) -> std::io::Result<()> {
    let uri = file_uri(path);
    if server.open_versions.contains_key(path) {
        send(
            &mut server.stdin,
            &json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]}}),
        )?;
    } else {
        send(
            &mut server.stdin,
            &json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"rust","version":version,"text":text}}}),
        )?;
    }
    server.open_versions.insert(path.to_owned(), version);
    Ok(())
}

fn request_at(
    server: &mut Server,
    kind: Pending,
    method: &str,
    path: &Path,
    text: &str,
    char_offset: usize,
) -> std::io::Result<()> {
    let id = server.next_id;
    server.next_id += 1;
    server.pending.insert(id, kind);
    let (line, character) = position(text, char_offset);
    send(
        &mut server.stdin,
        &json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"textDocument":{"uri":file_uri(path)},"position":{"line":line,"character":character}}}),
    )
}

fn request_references(
    server: &mut Server,
    path: &Path,
    text: &str,
    char_offset: usize,
) -> std::io::Result<()> {
    let id = server.next_id;
    server.next_id += 1;
    server.pending.insert(id, Pending::References);
    let (line, character) = position(text, char_offset);
    send(
        &mut server.stdin,
        &json!({"jsonrpc":"2.0","id":id,"method":"textDocument/references","params":{"textDocument":{"uri":file_uri(path)},"position":{"line":line,"character":character},"context":{"includeDeclaration":true}}}),
    )
}

/// The active signature's label from a `signatureHelp` result, if any.
fn signature_text(result: &Value) -> String {
    let signatures = result.get("signatures").and_then(Value::as_array);
    let active = result
        .get("activeSignature")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    signatures
        .and_then(|sigs| sigs.get(active).or_else(|| sigs.first()))
        .and_then(|sig| sig.get("label").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

fn handle_message(server: &mut Server, message: Value, events: &Sender<LspEvent>) {
    if message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics") {
        let Some(params) = message.get("params") else {
            return;
        };
        let Some(path) = params.get("uri").and_then(Value::as_str).and_then(uri_path) else {
            return;
        };
        let items = params
            .get("diagnostics")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|item| Diagnostic {
                line: item
                    .pointer("/range/start/line")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                column: item
                    .pointer("/range/start/character")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                severity: item.get("severity").and_then(Value::as_u64).unwrap_or(3),
                message: item
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("diagnostic")
                    .to_owned(),
            })
            .collect();
        let _ = events.send(LspEvent::Diagnostics { path, items });
        return;
    }
    let Some(id) = message.get("id").and_then(Value::as_u64) else {
        return;
    };
    let Some(kind) = server.pending.remove(&id) else {
        return;
    };
    let result = message.get("result").cloned().unwrap_or(Value::Null);
    match kind {
        Pending::Completion => {
            let array = result
                .as_array()
                .or_else(|| result.get("items").and_then(Value::as_array));
            let labels = array
                .into_iter()
                .flatten()
                .filter_map(|item| item.get("label").and_then(Value::as_str))
                .take(80)
                .map(str::to_owned)
                .collect();
            let _ = events.send(LspEvent::Completions(labels));
        }
        Pending::Hover => {
            let _ = events.send(LspEvent::Hover(markup_text(&result)));
        }
        Pending::Definition => {
            if let Some((uri, line)) = definition_location(&result) {
                if let Some(path) = uri_path(uri) {
                    let _ = events.send(LspEvent::Definition {
                        path,
                        line: line as usize,
                    });
                }
            }
        }
        Pending::ProbeDefinition(char_offset) => {
            let _ = events.send(LspEvent::DefinitionProbe {
                char_offset,
                navigable: definition_location(&result).is_some(),
            });
        }
        Pending::References => {
            let mut references = Vec::new();
            if let Some(items) = result.as_array() {
                for location in items {
                    let Some(path) = location
                        .get("uri")
                        .and_then(Value::as_str)
                        .and_then(uri_path)
                    else {
                        continue;
                    };
                    let line = location
                        .pointer("/range/start/line")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    let column = location
                        .pointer("/range/start/character")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    references.push(Reference { path, line, column });
                }
            }
            let _ = events.send(LspEvent::References(references));
        }
        Pending::Signature => {
            let _ = events.send(LspEvent::Signature(signature_text(&result)));
        }
    }
}

fn definition_location(result: &Value) -> Option<(&str, u64)> {
    let location = result
        .as_array()
        .and_then(|items| items.first())
        .unwrap_or(result);
    let uri = location
        .get("uri")
        .or_else(|| location.get("targetUri"))?
        .as_str()?;
    let line = location
        .pointer("/range/start/line")
        .or_else(|| location.pointer("/targetRange/start/line"))?
        .as_u64()?;
    Some((uri, line))
}

fn read_messages(stdout: impl Read, sender: Sender<Value>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut length = 0usize;
        loop {
            let mut header = String::new();
            if reader
                .read_line(&mut header)
                .ok()
                .filter(|n| *n > 0)
                .is_none()
            {
                return;
            }
            if header == "\r\n" {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length:") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        if let Ok(value) = serde_json::from_slice(&body) {
            let _ = sender.send(value);
        }
    }
}

fn send(stdin: &mut ChildStdin, value: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(value)?;
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len())?;
    stdin.write_all(&body)?;
    stdin.flush()
}
fn file_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .unwrap_or_default()
}
fn uri_path(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}
fn position(text: &str, char_offset: usize) -> (usize, usize) {
    let prefix = text.chars().take(char_offset).collect::<String>();
    let line = prefix.matches('\n').count();
    let character = prefix
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .encode_utf16()
        .count();
    (line, character)
}
fn markup_text(value: &Value) -> String {
    let contents = value.get("contents").unwrap_or(value);
    if let Some(text) = contents.as_str() {
        return text.to_owned();
    }
    if let Some(text) = contents.get("value").and_then(Value::as_str) {
        return text.to_owned();
    }
    if let Some(items) = contents.as_array() {
        return items.iter().map(markup_text).collect::<Vec<_>>().join("\n");
    }
    "No hover information.".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_character_offset_to_utf16_lsp_position() {
        assert_eq!(position("let x = 1;\nabc", 12), (1, 1));
        assert_eq!(position("a😀b", 2), (0, 3));
    }

    #[test]
    fn extracts_hover_markup() {
        let value = json!({"contents":{"kind":"markdown","value":"`Vec<f32>`"}});
        assert_eq!(markup_text(&value), "`Vec<f32>`");
    }

    #[test]
    fn picks_active_signature_label() {
        let value = json!({
            "signatures": [
                {"label": "fn zero()"},
                {"label": "fn push(&mut self, value: T)"}
            ],
            "activeSignature": 1
        });
        assert_eq!(signature_text(&value), "fn push(&mut self, value: T)");
        // Missing signatures yields an empty string, not a panic.
        assert_eq!(signature_text(&json!({})), "");
    }
}
