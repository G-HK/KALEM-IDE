//! JSON-RPC over a server's stdin/stdout, with the LSP `Content-Length` framing.
//!
//! A reader thread routes what the server says: answers go to the waiting request,
//! notifications (diagnostics, messages) onto a channel, and the requests a server makes of
//! its client (`workspace/configuration`, progress registration, ...) are answered here so
//! the IDE never has to.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum LspError {
    /// The server program could not be started.
    Spawn(String),
    Io(String),
    /// The server did not answer in time.
    Timeout(String),
    /// The server answered with an error.
    Server {
        code: i64,
        message: String,
    },
    /// The server went away.
    Closed,
}

impl std::fmt::Display for LspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LspError::Spawn(s) => write!(f, "could not start the language server: {s}"),
            LspError::Io(s) => write!(f, "language server I/O error: {s}"),
            LspError::Timeout(m) => write!(f, "the language server did not answer `{m}`"),
            LspError::Server { message, .. } => write!(f, "{message}"),
            LspError::Closed => write!(f, "the language server exited"),
        }
    }
}

impl std::error::Error for LspError {}

/// What a server tells the IDE without being asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Notification {
    Diagnostics {
        uri: String,
        diagnostics: Vec<lsp_types::Diagnostic>,
    },
    /// `window/showMessage`: 1 error, 2 warning, 3 info, 4 log.
    Message { level: u8, text: String },
    /// The server process ended.
    Exited,
}

/// Frame one JSON message.
pub fn encode(msg: &Value) -> Vec<u8> {
    let body = msg.to_string();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// Read one framed message. `Ok(None)` at a clean end of stream.
pub fn read_message<R: BufRead>(r: &mut R) -> std::io::Result<Option<Value>> {
    let mut len: Option<usize> = None;
    let mut saw_header = false;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            if len.is_some() || saw_header {
                break;
            }
            continue;
        }
        saw_header = true;
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().ok();
            }
        }
    }
    let n = len.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length")
    })?;
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    serde_json::from_slice(&buf)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// `KALEM_LSP_TRACE=1` prints every message to stderr.
fn trace(dir: &str, msg: &Value) {
    if std::env::var_os("KALEM_LSP_TRACE").is_some() {
        let text = msg.to_string();
        let cut: String = text.chars().take(500).collect();
        eprintln!("lsp {dir} {cut}");
    }
}

type Pending = Arc<Mutex<HashMap<u64, Sender<Result<Value, LspError>>>>>;
type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

fn send_raw(w: &Writer, msg: &Value) -> std::io::Result<()> {
    trace("->", msg);
    let mut w = w.lock().unwrap();
    w.write_all(&encode(msg))?;
    w.flush()
}

/// The answer to a request the server made of the client.
fn client_reply(method: &str, params: &Value) -> Result<Value, (i64, String)> {
    match method {
        // One (empty) settings object per requested item: servers fall back to defaults.
        "workspace/configuration" => Ok(Value::Array(
            params["items"]
                .as_array()
                .map(|a| vec![Value::Null; a.len()])
                .unwrap_or_default(),
        )),
        "workspace/workspaceFolders" => Ok(Value::Null),
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create"
        | "window/showMessageRequest"
        | "window/showDocument" => Ok(Value::Null),
        "workspace/applyEdit" => Ok(json!({"applied": false})),
        _ => Err((-32601, format!("{method} is not supported by Kalem IDE"))),
    }
}

/// How to start `program`. On Windows, servers installed with npm are `.cmd` scripts, which
/// only `cmd /C` can run.
fn launcher(program: &str, args: &[String]) -> Command {
    #[cfg(windows)]
    {
        if let Some(script) = find_script(program) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(script).args(args);
            return c;
        }
    }
    let mut c = Command::new(program);
    c.args(args);
    c
}

#[cfg(windows)]
fn find_script(program: &str) -> Option<std::path::PathBuf> {
    if program.contains(['/', '\\']) {
        return None;
    }
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        if dir.join(format!("{program}.exe")).is_file() {
            return None;
        }
        for ext in ["cmd", "bat"] {
            let p = dir.join(format!("{program}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub struct Client {
    writer: Writer,
    pending: Pending,
    next_id: AtomicU64,
    notes: Mutex<Receiver<Notification>>,
    child: Mutex<Option<Child>>,
}

impl Client {
    pub fn spawn(program: &str, args: &[String], cwd: Option<&Path>) -> Result<Client, LspError> {
        let mut cmd = launcher(program, args);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(c) = cwd {
            cmd.current_dir(c);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| LspError::Spawn(format!("{program}: {e}")))?;
        let stdin = child.stdin.take().ok_or(LspError::Closed)?;
        let stdout = child.stdout.take().ok_or(LspError::Closed)?;
        Ok(Client::from_streams(
            Box::new(stdout),
            Box::new(stdin),
            Some(child),
        ))
    }

    /// Talk over any pair of streams (tests use an in-process server).
    pub fn from_streams(
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
        child: Option<Child>,
    ) -> Client {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let writer: Writer = Arc::new(Mutex::new(writer));
        let (ntx, nrx) = channel();
        let (p2, w2) = (pending.clone(), writer.clone());
        std::thread::spawn(move || {
            let mut r = std::io::BufReader::new(reader);
            while let Ok(Some(msg)) = read_message(&mut r) {
                trace("<-", &msg);
                let method = msg["method"].as_str();
                match (method, msg.get("id").filter(|i| !i.is_null())) {
                    (Some(m), Some(id)) => {
                        let reply = match client_reply(m, &msg["params"]) {
                            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                            Err((code, message)) => json!({
                                "jsonrpc": "2.0", "id": id,
                                "error": {"code": code, "message": message}
                            }),
                        };
                        let _ = send_raw(&w2, &reply);
                    }
                    (Some(m), None) => match m {
                        "textDocument/publishDiagnostics" => {
                            let uri = msg["params"]["uri"].as_str().unwrap_or("").to_string();
                            let diagnostics =
                                serde_json::from_value(msg["params"]["diagnostics"].clone())
                                    .unwrap_or_default();
                            let _ = ntx.send(Notification::Diagnostics { uri, diagnostics });
                        }
                        "window/showMessage" => {
                            let _ = ntx.send(Notification::Message {
                                level: msg["params"]["type"].as_u64().unwrap_or(3) as u8,
                                text: msg["params"]["message"].as_str().unwrap_or("").to_string(),
                            });
                        }
                        _ => {}
                    },
                    (None, Some(id)) => {
                        let Some(tx) = id.as_u64().and_then(|i| p2.lock().unwrap().remove(&i))
                        else {
                            continue;
                        };
                        let _ = tx.send(match msg.get("error").filter(|e| !e.is_null()) {
                            Some(e) => Err(LspError::Server {
                                code: e["code"].as_i64().unwrap_or(0),
                                message: e["message"]
                                    .as_str()
                                    .unwrap_or("request failed")
                                    .to_string(),
                            }),
                            None => Ok(msg["result"].clone()),
                        });
                    }
                    (None, None) => {}
                }
            }
            for (_, tx) in p2.lock().unwrap().drain() {
                let _ = tx.send(Err(LspError::Closed));
            }
            let _ = ntx.send(Notification::Exited);
        });
        Client {
            writer,
            pending,
            next_id: AtomicU64::new(1),
            notes: Mutex::new(nrx),
            child: Mutex::new(child),
        }
    }

    /// Send a request and wait for its result (`null` is `Value::Null`).
    pub fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LspError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id, tx);
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(e) = send_raw(&self.writer, &msg) {
            self.pending.lock().unwrap().remove(&id);
            return Err(LspError::Io(e.to_string()));
        }
        match rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => {
                self.pending.lock().unwrap().remove(&id);
                // Tell the server we no longer care.
                let _ = self.notify("$/cancelRequest", json!({"id": id}));
                Err(LspError::Timeout(method.to_string()))
            }
            Err(RecvTimeoutError::Disconnected) => Err(LspError::Closed),
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        send_raw(&self.writer, &msg).map_err(|e| LspError::Io(e.to_string()))
    }

    /// The next notification, if one arrives within `timeout`.
    pub fn next_notification(&self, timeout: Duration) -> Option<Notification> {
        self.notes.lock().unwrap().recv_timeout(timeout).ok()
    }

    pub fn kill(&self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn framing_round_trips() {
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "x", "params": {"s": "é"}});
        let bytes = encode(&msg);
        assert!(bytes.starts_with(b"Content-Length: "));
        let got = read_message(&mut Cursor::new(bytes)).unwrap().unwrap();
        assert_eq!(got, msg);
    }

    #[test]
    fn extra_headers_and_end_of_stream() {
        let body = "{\"a\":1}";
        let raw = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let mut c = Cursor::new(raw.into_bytes());
        assert_eq!(read_message(&mut c).unwrap().unwrap()["a"], 1);
        assert!(read_message(&mut c).unwrap().is_none());
        assert!(read_message(&mut Cursor::new(b"X: 1\r\n\r\n{}".to_vec())).is_err());
    }

    #[test]
    fn configuration_requests_get_one_answer_per_item() {
        let r = client_reply(
            "workspace/configuration",
            &json!({"items": [{"section": "a"}, {"section": "b"}]}),
        )
        .unwrap();
        assert_eq!(r, json!([null, null]));
        assert!(client_reply("no/such", &Value::Null).is_err());
    }
}
