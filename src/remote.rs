use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

const MAX_REMOTE_CODE_BYTES: usize = 1024 * 1024;
const MAX_REMOTE_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_REMOTE_INPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteProfile {
    pub name: String,
    pub jupyter_url: String,
    pub agent_command: String,
    pub credential_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteKernelSession {
    pub id: String,
    pub name: String,
    #[serde(skip)]
    pub profile: RemoteProfile,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteExecution {
    pub output: String,
    pub mime: Vec<crate::notebook::RichOutput>,
    pub execution_count: Option<u64>,
    pub status: String,
}

pub struct RemoteInputRequest {
    pub prompt: String,
    pub password: bool,
}

pub fn store_token(profile: &RemoteProfile, token: &str) -> Result<(), String> {
    crate::database::store_secret(&profile.credential_key, token)
}

pub fn validate_profile(profile: &RemoteProfile) -> Result<(), String> {
    if profile.name.trim().is_empty() {
        return Err("Remote profile name cannot be empty.".into());
    }
    if profile.credential_key.trim().is_empty() || profile.credential_key.contains('\0') {
        return Err("Remote profile requires a valid credential key.".into());
    }
    let url = url::Url::parse(&profile.jupyter_url)
        .map_err(|error| format!("Invalid Jupyter URL: {error}"))?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err("Remote Jupyter requires HTTPS; HTTP is allowed only for localhost.".into());
    }
    if !url.username().is_empty() || url.password().is_some() || url.query().is_some() {
        return Err("Store remote credentials in the OS credential manager, not the URL.".into());
    }
    if url.fragment().is_some() {
        return Err("Jupyter URLs cannot contain fragments.".into());
    }
    Ok(())
}

pub fn test_jupyter(profile: &RemoteProfile) -> Result<String, String> {
    validate_profile(profile)?;
    let endpoint = kernelspec_endpoint(&profile.jupyter_url)?;
    let token = crate::database::load_secret(&profile.credential_key).unwrap_or_default();
    let mut child = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--max-time",
            "10",
            "--max-filesize",
            "1048576",
            "--header",
            "@-",
        ])
        .arg(endpoint.as_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start curl: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        if !token.is_empty() {
            writeln!(stdin, "Authorization: token {token}").map_err(|e| e.to_string())?;
        }
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(redact(&error, &token));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Invalid Jupyter response: {e}"))?;
    let kernels = value["kernelspecs"].as_object().ok_or_else(|| {
        "Jupyter kernelspec response did not contain a kernelspecs object.".to_owned()
    })?;
    let mut names = kernels.keys().cloned().collect::<Vec<_>>();
    names.sort();
    Ok(format!(
        "Connected to `{}`: {} kernelspec(s){}.",
        profile.name,
        names.len(),
        if names.is_empty() {
            String::new()
        } else {
            format!(" ({})", names.join(", "))
        }
    ))
}

pub fn start_kernel(
    profile: &RemoteProfile,
    kernel_name: &str,
) -> Result<RemoteKernelSession, String> {
    validate_profile(profile)?;
    validate_identifier(kernel_name, "Kernel name")?;
    let endpoint = api_endpoint(&profile.jupyter_url, "api/kernels")?;
    let body = serde_json::json!({ "name": kernel_name }).to_string();
    let output = curl_request(profile, "POST", &endpoint, Some(&body))?;
    let mut session: RemoteKernelSession = serde_json::from_slice(&output)
        .map_err(|e| format!("Invalid kernel creation response: {e}"))?;
    validate_identifier(&session.id, "Kernel session ID")?;
    if session.name.is_empty() {
        session.name = kernel_name.to_owned();
    }
    session.profile = profile.clone();
    Ok(session)
}

pub fn stop_kernel(session: &RemoteKernelSession) -> Result<String, String> {
    validate_profile(&session.profile)?;
    validate_identifier(&session.id, "Kernel session ID")?;
    let endpoint = api_endpoint(
        &session.profile.jupyter_url,
        &format!("api/kernels/{}", session.id),
    )?;
    curl_request(&session.profile, "DELETE", &endpoint, None)?;
    Ok(format!(
        "Stopped remote kernel `{}` ({}) on `{}`.",
        session.name, session.id, session.profile.name
    ))
}

pub fn interrupt_kernel(session: &RemoteKernelSession) -> Result<String, String> {
    validate_profile(&session.profile)?;
    validate_identifier(&session.id, "Kernel session ID")?;
    let endpoint = api_endpoint(
        &session.profile.jupyter_url,
        &format!("api/kernels/{}/interrupt", session.id),
    )?;
    curl_request(&session.profile, "POST", &endpoint, None)?;
    Ok(format!(
        "Interrupted remote kernel `{}` ({}) on `{}`.",
        session.name, session.id, session.profile.name
    ))
}

pub fn execute(
    session: &RemoteKernelSession,
    code: &str,
    input: &std::sync::mpsc::Receiver<String>,
    mut on_input: impl FnMut(RemoteInputRequest) -> Result<(), String>,
) -> Result<RemoteExecution, String> {
    validate_profile(&session.profile)?;
    validate_identifier(&session.id, "Kernel session ID")?;
    if code.trim().is_empty() {
        return Err("Remote execution requires non-empty code.".into());
    }
    if code.len() > MAX_REMOTE_CODE_BYTES {
        return Err("Remote code is limited to 1 MiB.".into());
    }
    let session_id = uuid::Uuid::new_v4().to_string();
    let msg_id = uuid::Uuid::new_v4().to_string();
    let endpoint = websocket_endpoint(&session.profile.jupyter_url, &session.id, &session_id)?;
    let token = crate::database::load_secret(&session.profile.credential_key).unwrap_or_default();
    let mut request =
        tungstenite::client::IntoClientRequest::into_client_request(endpoint.as_str())
            .map_err(|e| e.to_string())?;
    if !token.is_empty() {
        let value = tungstenite::http::HeaderValue::from_str(&format!("token {token}"))
            .map_err(|e| e.to_string())?;
        request
            .headers_mut()
            .insert(tungstenite::http::header::AUTHORIZATION, value);
    }
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_REMOTE_OUTPUT_BYTES));
    let (mut socket, _) = tungstenite::client::connect_with_config(request, Some(config), 0)
        .map_err(|e| redact(&e.to_string(), &token))?;
    set_socket_timeout(socket.get_mut(), Duration::from_secs(30))?;
    let message = execute_request(&session_id, &msg_id, code);
    socket
        .send(tungstenite::Message::text(message.to_string()))
        .map_err(|e| redact(&e.to_string(), &token))?;

    let mut execution = RemoteExecution {
        output: String::new(),
        mime: Vec::new(),
        execution_count: None,
        status: "running".into(),
    };
    let mut idle = false;
    let mut replied = false;
    while !(idle && replied) {
        let frame = socket
            .read()
            .map_err(|e| redact(&format!("Remote kernel channel failed: {e}"), &token))?;
        let text = match frame {
            tungstenite::Message::Text(text) => text.to_string(),
            tungstenite::Message::Close(_) => {
                return Err("Remote kernel channel closed before execution completed.".into())
            }
            _ => continue,
        };
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("Invalid Jupyter message: {e}"))?;
        if value["parent_header"]["msg_id"].as_str() != Some(msg_id.as_str()) {
            continue;
        }
        if message_type(&value) == "input_request" {
            let prompt = value["content"]["prompt"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if prompt.len() > MAX_REMOTE_INPUT_BYTES {
                return Err("Remote input prompt exceeded the 64 KiB limit.".into());
            }
            let password = value["content"]["password"].as_bool().unwrap_or(false);
            on_input(RemoteInputRequest { prompt, password })?;
            let reply = input.recv_timeout(Duration::from_secs(300)).map_err(|_| {
                "Remote input was cancelled or timed out after 5 minutes.".to_owned()
            })?;
            if reply.len() > MAX_REMOTE_INPUT_BYTES {
                return Err("Remote input is limited to 64 KiB.".into());
            }
            let reply = input_reply(&session_id, &value["header"], &reply);
            socket
                .send(tungstenite::Message::text(reply.to_string()))
                .map_err(|e| redact(&e.to_string(), &token))?;
            continue;
        }
        let state = apply_execution_message(&mut execution, &value)?;
        idle |= state.0;
        replied |= state.1;
    }
    let _ = socket.close(None);
    Ok(execution)
}

fn apply_execution_message(
    execution: &mut RemoteExecution,
    value: &serde_json::Value,
) -> Result<(bool, bool), String> {
    let msg_type = message_type(value);
    let mut idle = false;
    let mut replied = false;
    match msg_type {
        "stream" => append_execution_text(
            execution,
            value["content"]["text"].as_str().unwrap_or_default(),
        )?,
        "execute_result" | "display_data" => {
            append_mime_bundle(execution, &value["content"]["data"])?
        }
        "error" => {
            let traceback = value["content"]["traceback"]
                .as_array()
                .map(|lines| {
                    lines
                        .iter()
                        .filter_map(|line| line.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_else(|| "Remote kernel error".into());
            append_execution_text(execution, &traceback)?;
            execution.status = "error".into();
        }
        "execute_reply" => {
            replied = true;
            execution.execution_count = value["content"]["execution_count"].as_u64();
            execution.status = value["content"]["status"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned();
        }
        "status" if value["content"]["execution_state"].as_str() == Some("idle") => {
            idle = true;
        }
        _ => {}
    }
    Ok((idle, replied))
}

fn execute_request(session_id: &str, msg_id: &str, code: &str) -> serde_json::Value {
    serde_json::json!({
        "header": {
            "msg_id": msg_id,
            "username": "forge-ml",
            "session": session_id,
            "date": chrono::Utc::now().to_rfc3339(),
            "msg_type": "execute_request",
            "version": "5.3"
        },
        "parent_header": {},
        "metadata": {},
        "content": {
            "code": code,
            "silent": false,
            "store_history": true,
            "user_expressions": {},
            "allow_stdin": true,
            "stop_on_error": true
        },
        "channel": "shell",
        "buffers": []
    })
}

fn input_reply(
    session_id: &str,
    parent_header: &serde_json::Value,
    value: &str,
) -> serde_json::Value {
    serde_json::json!({
        "header": {
            "msg_id": uuid::Uuid::new_v4().to_string(),
            "username": "forge-ml",
            "session": session_id,
            "date": chrono::Utc::now().to_rfc3339(),
            "msg_type": "input_reply",
            "version": "5.3"
        },
        "parent_header": parent_header,
        "metadata": {},
        "content": { "value": value },
        "channel": "stdin",
        "buffers": []
    })
}

fn message_type(value: &serde_json::Value) -> &str {
    value["msg_type"]
        .as_str()
        .or_else(|| value["header"]["msg_type"].as_str())
        .unwrap_or_default()
}

fn append_bounded(output: &mut String, value: &str) -> Result<(), String> {
    let additional = value.len() + usize::from(!value.ends_with('\n'));
    if output.len().saturating_add(additional) > MAX_REMOTE_OUTPUT_BYTES {
        return Err("Remote output exceeded the 2 MiB limit.".into());
    }
    output.push_str(value);
    if !value.ends_with('\n') {
        output.push('\n');
    }
    Ok(())
}

fn append_mime_bundle(
    execution: &mut RemoteExecution,
    data: &serde_json::Value,
) -> Result<(), String> {
    let Some(bundle) = data.as_object() else {
        return Ok(());
    };
    if let Some(plain) = bundle.get("text/plain").and_then(mime_text) {
        append_execution_text(execution, &plain)?;
    }
    const SUPPORTED: [&str; 5] = [
        "text/html",
        "text/markdown",
        "image/svg+xml",
        "image/png",
        "application/json",
    ];
    for mime in SUPPORTED {
        let Some(value) = bundle.get(mime) else {
            continue;
        };
        let data = mime_text(value).unwrap_or_else(|| value.to_string());
        let used = execution.output.len()
            + execution
                .mime
                .iter()
                .map(|output| output.mime.len() + output.data.len())
                .sum::<usize>();
        if used.saturating_add(mime.len()).saturating_add(data.len()) > MAX_REMOTE_OUTPUT_BYTES {
            return Err("Remote output exceeded the 2 MiB limit.".into());
        }
        execution.mime.push(crate::notebook::RichOutput {
            mime: mime.into(),
            data,
        });
    }
    Ok(())
}

fn mime_text(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_owned());
    }
    value.as_array().and_then(|parts| {
        parts
            .iter()
            .map(|part| part.as_str())
            .collect::<Option<Vec<_>>>()
            .map(|parts| parts.concat())
    })
}

fn append_execution_text(execution: &mut RemoteExecution, value: &str) -> Result<(), String> {
    let rich_bytes = execution
        .mime
        .iter()
        .map(|output| output.mime.len() + output.data.len())
        .sum::<usize>();
    let additional = value.len() + usize::from(!value.ends_with('\n'));
    if execution
        .output
        .len()
        .saturating_add(rich_bytes)
        .saturating_add(additional)
        > MAX_REMOTE_OUTPUT_BYTES
    {
        return Err("Remote output exceeded the 2 MiB limit.".into());
    }
    append_bounded(&mut execution.output, value)
}

fn websocket_endpoint(base: &str, kernel_id: &str, session_id: &str) -> Result<url::Url, String> {
    let mut url = api_endpoint(base, &format!("api/kernels/{kernel_id}/channels"))?;
    url.set_scheme(if url.scheme() == "https" { "wss" } else { "ws" })
        .map_err(|_| "Could not construct Jupyter WebSocket URL.".to_owned())?;
    url.query_pairs_mut().append_pair("session_id", session_id);
    Ok(url)
}

fn set_socket_timeout(
    stream: &mut tungstenite::stream::MaybeTlsStream<std::net::TcpStream>,
    timeout: Duration,
) -> Result<(), String> {
    match stream {
        tungstenite::stream::MaybeTlsStream::Plain(stream) => stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| e.to_string())?,
        tungstenite::stream::MaybeTlsStream::Rustls(stream) => stream
            .sock
            .set_read_timeout(Some(timeout))
            .map_err(|e| e.to_string())?,
        _ => return Err("Unsupported remote TLS stream.".into()),
    }
    Ok(())
}

fn curl_request(
    profile: &RemoteProfile,
    method: &str,
    endpoint: &url::Url,
    body: Option<&str>,
) -> Result<Vec<u8>, String> {
    let token = crate::database::load_secret(&profile.credential_key).unwrap_or_default();
    let mut command = Command::new("curl");
    command.args([
        "--silent",
        "--show-error",
        "--fail",
        "--max-time",
        "10",
        "--max-filesize",
        "1048576",
        "--header",
        "@-",
        "--request",
        method,
    ]);
    if let Some(body) = body {
        command.args(["--data-binary", body]);
    }
    let mut child = command
        .arg(endpoint.as_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start curl: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        if !token.is_empty() {
            writeln!(stdin, "Authorization: token {token}").map_err(|e| e.to_string())?;
        }
        if body.is_some() {
            writeln!(stdin, "Content-Type: application/json").map_err(|e| e.to_string())?;
        }
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(redact(&error, &token));
    }
    Ok(output.stdout)
}

fn kernelspec_endpoint(base: &str) -> Result<url::Url, String> {
    api_endpoint(base, "api/kernelspecs")
}

fn api_endpoint(base: &str, route: &str) -> Result<url::Url, String> {
    let mut url = url::Url::parse(base).map_err(|e| e.to_string())?;
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    url.join(route).map_err(|e| e.to_string())
}

fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!(
            "{label} must contain only letters, numbers, dots, underscores, or hyphens."
        ));
    }
    Ok(())
}

fn redact(message: &str, token: &str) -> String {
    if token.is_empty() {
        message.to_owned()
    } else {
        message.replace(token, "[REDACTED]")
    }
}

pub fn generate_actions_workflow(root: &Path) -> Result<String, String> {
    let path = root.join(".github/workflows/remote-training.yml");
    if path.exists() {
        return Err(format!("{} already exists.", path.display()));
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, include_str!("../templates/remote-training.yml"))
        .map_err(|e| e.to_string())?;
    Ok(format!("Generated {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workflow_refuses_overwrite() {
        let root = std::env::temp_dir().join(format!("forge-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(generate_actions_workflow(&root).is_ok());
        assert!(generate_actions_workflow(&root).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn validates_secure_remote_urls_and_preserves_hub_paths() {
        let profile = RemoteProfile {
            name: "lab".into(),
            jupyter_url: "https://example.test/user/forge".into(),
            agent_command: String::new(),
            credential_key: "remote:test".into(),
        };
        validate_profile(&profile).unwrap();
        assert_eq!(
            kernelspec_endpoint(&profile.jupyter_url).unwrap().as_str(),
            "https://example.test/user/forge/api/kernelspecs"
        );

        let mut insecure = profile.clone();
        insecure.jupyter_url = "http://example.test".into();
        assert!(validate_profile(&insecure).is_err());
        insecure.jupyter_url = "http://localhost:8888".into();
        validate_profile(&insecure).unwrap();
        insecure.jupyter_url = "https://example.test/?token=secret".into();
        assert!(validate_profile(&insecure).is_err());
    }

    #[test]
    fn redacts_remote_tokens_from_errors() {
        assert_eq!(
            redact("request token-secret failed", "token-secret"),
            "request [REDACTED] failed"
        );
    }

    #[test]
    fn validates_kernel_identifiers_and_session_responses() {
        validate_identifier("python3", "Kernel name").unwrap();
        validate_identifier("session-id_1", "Session").unwrap();
        assert!(validate_identifier("../../escape", "Session").is_err());
        assert!(validate_identifier("name with spaces", "Kernel name").is_err());

        let session: RemoteKernelSession =
            serde_json::from_str(r#"{"id":"abc-123","name":"python3"}"#).unwrap();
        assert_eq!(session.id, "abc-123");
        assert_eq!(session.name, "python3");
    }

    #[test]
    fn builds_correlated_jupyter_execute_messages_and_bounded_websocket_urls() {
        let message = execute_request("session-1", "message-1", "1 + 1");
        assert_eq!(message["header"]["msg_type"], "execute_request");
        assert_eq!(message["content"]["code"], "1 + 1");
        assert_eq!(message["content"]["allow_stdin"], true);
        let reply = input_reply(
            "session-1",
            &serde_json::json!({"msg_id":"input-message-1"}),
            "secret",
        );
        assert_eq!(reply["header"]["msg_type"], "input_reply");
        assert_eq!(reply["parent_header"]["msg_id"], "input-message-1");
        assert_eq!(reply["content"]["value"], "secret");
        assert_eq!(reply["channel"], "stdin");
        let endpoint =
            websocket_endpoint("https://example.test/user/forge", "kernel-1", "session-1").unwrap();
        assert_eq!(endpoint.scheme(), "wss");
        assert_eq!(endpoint.path(), "/user/forge/api/kernels/kernel-1/channels");
        assert_eq!(endpoint.query(), Some("session_id=session-1"));

        let mut output = String::new();
        append_bounded(&mut output, "two").unwrap();
        assert_eq!(output, "two\n");

        let mut execution = RemoteExecution {
            output: String::new(),
            mime: Vec::new(),
            execution_count: None,
            status: "running".into(),
        };
        assert_eq!(
            apply_execution_message(
                &mut execution,
                &serde_json::json!({"msg_type":"stream","content":{"text":"hello"}})
            )
            .unwrap(),
            (false, false)
        );
        assert_eq!(execution.output, "hello\n");
        apply_execution_message(
            &mut execution,
            &serde_json::json!({
                "msg_type":"display_data",
                "content":{"data":{
                    "text/plain":"chart",
                    "text/html":["<strong>", "chart</strong>"],
                    "application/json":{"points":[1,2]}
                }}
            }),
        )
        .unwrap();
        assert_eq!(execution.output, "hello\nchart\n");
        assert_eq!(execution.mime.len(), 2);
        assert_eq!(execution.mime[0].mime, "text/html");
        assert_eq!(execution.mime[0].data, "<strong>chart</strong>");
        assert_eq!(execution.mime[1].data, r#"{"points":[1,2]}"#);
        assert_eq!(
            apply_execution_message(
                &mut execution,
                &serde_json::json!({"msg_type":"execute_reply","content":{"status":"ok","execution_count":7}})
            )
            .unwrap(),
            (false, true)
        );
        assert_eq!(execution.execution_count, Some(7));
        assert_eq!(execution.status, "ok");
        assert_eq!(
            apply_execution_message(
                &mut execution,
                &serde_json::json!({"msg_type":"status","content":{"execution_state":"idle"}})
            )
            .unwrap(),
            (true, false)
        );
    }
}
