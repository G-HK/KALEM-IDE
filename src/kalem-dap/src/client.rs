//! The wire protocol: `Content-Length` framed JSON, request/response matching, events.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a request may take before the adapter is considered stuck.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq)]
pub enum DapError {
    /// The adapter program could not be started.
    Spawn(String),
    /// Reading or writing the adapter's pipes failed.
    Io(String),
    /// The adapter did not answer in time.
    Timeout(String),
    /// The adapter answered with `success: false`.
    Adapter(String),
    /// The adapter went away.
    Closed,
}

impl std::fmt::Display for DapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DapError::Spawn(s) => write!(f, "could not start the debug adapter: {s}"),
            DapError::Io(s) => write!(f, "debug adapter I/O error: {s}"),
            DapError::Timeout(s) => write!(f, "the debug adapter did not answer `{s}`"),
            DapError::Adapter(s) => write!(f, "{s}"),
            DapError::Closed => write!(f, "the debug adapter exited"),
        }
    }
}

impl std::error::Error for DapError {}

/// An adapter event (`stopped`, `output`, `terminated`, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub name: String,
    pub body: Value,
}

/// Frame one JSON message for the wire.
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
                break; // end of the header block (a missing length is reported below)
            }
            continue; // stray blank line between messages
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

/// `KALEM_DAP_TRACE=1` prints every protocol message to stderr (for debugging adapters).
fn trace(dir: &str, msg: &Value) {
    if std::env::var_os("KALEM_DAP_TRACE").is_some() {
        let text = msg.to_string();
        let cut: String = text.chars().take(600).collect();
        eprintln!("dap {dir} {cut}");
    }
}

type Pending = Arc<Mutex<HashMap<u64, Sender<Result<Value, DapError>>>>>;

/// A request that has been sent; `wait` blocks for its answer.
pub struct PendingResponse {
    command: String,
    rx: Receiver<Result<Value, DapError>>,
    requeue: Sender<Result<Value, DapError>>,
}

impl PendingResponse {
    pub fn wait(self, timeout: Duration) -> Result<Value, DapError> {
        match self.rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => Err(DapError::Timeout(self.command)),
            Err(RecvTimeoutError::Disconnected) => Err(DapError::Closed),
        }
    }

    /// The error answer, if the request has already failed. A success is left in place
    /// for `wait`.
    pub fn peek_error(&self) -> Option<Result<Value, DapError>> {
        match self.rx.try_recv() {
            Ok(Err(e)) => Some(Err(e)),
            Ok(Ok(v)) => {
                // Put the success back for `wait`.
                let _ = self.requeue.send(Ok(v));
                None
            }
            Err(_) => None,
        }
    }
}

pub struct Client {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Pending,
    seq: Mutex<u64>,
    events: Mutex<Receiver<Event>>,
    child: Mutex<Option<Child>>,
}

impl Client {
    /// Start an adapter program and talk to it over its stdin/stdout.
    pub fn spawn(program: &str, args: &[String], cwd: Option<&Path>) -> Result<Client, DapError> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::piped())
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
            .map_err(|e| DapError::Spawn(format!("{program}: {e}")))?;
        let stdin = child.stdin.take().ok_or(DapError::Closed)?;
        let stdout = child.stdout.take().ok_or(DapError::Closed)?;
        Ok(Client::from_streams(
            Box::new(stdout),
            Box::new(stdin),
            Some(child),
        ))
    }

    /// Talk over any pair of streams (used by tests with an in-process adapter).
    pub fn from_streams(
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
        child: Option<Child>,
    ) -> Client {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (etx, erx) = channel();
        let writer = Arc::new(Mutex::new(writer));
        let (p2, w2) = (pending.clone(), writer.clone());
        std::thread::spawn(move || {
            let mut r = std::io::BufReader::new(reader);
            while let Ok(Some(msg)) = read_message(&mut r) {
                trace("<-", &msg);
                match msg["type"].as_str() {
                    Some("response") => {
                        let Some(id) = msg["request_seq"].as_u64() else {
                            continue;
                        };
                        let Some(tx) = p2.lock().unwrap().remove(&id) else {
                            continue;
                        };
                        let _ = tx.send(if msg["success"].as_bool().unwrap_or(false) {
                            Ok(msg["body"].clone())
                        } else {
                            Err(DapError::Adapter(
                                msg["message"]
                                    .as_str()
                                    .or_else(|| msg["body"]["error"]["format"].as_str())
                                    .unwrap_or("request failed")
                                    .to_string(),
                            ))
                        });
                    }
                    Some("event") => {
                        let _ = etx.send(Event {
                            name: msg["event"].as_str().unwrap_or_default().to_string(),
                            body: msg["body"].clone(),
                        });
                    }
                    Some("request") => {
                        // Reverse requests (runInTerminal, startDebugging) are not supported.
                        let reply = json!({
                            "type": "response",
                            "request_seq": msg["seq"],
                            "command": msg["command"],
                            "success": false,
                            "message": "not supported by Kalem IDE",
                        });
                        let _ = w2.lock().unwrap().write_all(&encode(&reply));
                    }
                    _ => {}
                }
            }
            // The adapter went away: fail everything still waiting, then tell the session.
            for (_, tx) in p2.lock().unwrap().drain() {
                let _ = tx.send(Err(DapError::Closed));
            }
            let _ = etx.send(Event {
                name: "kalem/adapterExited".into(),
                body: Value::Null,
            });
        });
        // `writer` is shared with the reader thread (for reverse-request replies); the client
        // writes through the same lock.
        let shared: Box<dyn Write + Send> = Box::new(SharedWriter(writer));
        Client {
            writer: Mutex::new(shared),
            pending,
            seq: Mutex::new(0),
            events: Mutex::new(erx),
            child: Mutex::new(child),
        }
    }

    /// Send a request without waiting for the answer.
    pub fn send(&self, command: &str, arguments: Value) -> Result<PendingResponse, DapError> {
        let id = {
            let mut s = self.seq.lock().unwrap();
            *s += 1;
            *s
        };
        let (tx, rx) = channel();
        let requeue = tx.clone();
        self.pending.lock().unwrap().insert(id, tx);
        let msg = json!({
            "seq": id,
            "type": "request",
            "command": command,
            "arguments": arguments,
        });
        trace("->", &msg);
        let res = {
            let mut w = self.writer.lock().unwrap();
            w.write_all(&encode(&msg)).and_then(|_| w.flush())
        };
        if let Err(e) = res {
            self.pending.lock().unwrap().remove(&id);
            return Err(DapError::Io(e.to_string()));
        }
        Ok(PendingResponse {
            command: command.to_string(),
            rx,
            requeue,
        })
    }

    /// Send a request and wait for its answer body.
    pub fn request(&self, command: &str, arguments: Value) -> Result<Value, DapError> {
        self.send(command, arguments)?.wait(REQUEST_TIMEOUT)
    }

    /// The next adapter event, if one arrives within `timeout`.
    pub fn next_event(&self, timeout: Duration) -> Option<Event> {
        self.events.lock().unwrap().recv_timeout(timeout).ok()
    }

    /// Stop the adapter process (after a polite `disconnect`, or when it is stuck).
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

struct SharedWriter(Arc<Mutex<Box<dyn Write + Send>>>);

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A pipe made of a channel, for an in-process mock adapter.
    pub struct ChanReader(Receiver<Vec<u8>>, Vec<u8>);
    pub struct ChanWriter(Sender<Vec<u8>>);

    impl Read for ChanReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.1.is_empty() {
                match self.0.recv() {
                    Ok(v) => self.1 = v,
                    Err(_) => return Ok(0),
                }
            }
            let n = buf.len().min(self.1.len());
            buf[..n].copy_from_slice(&self.1[..n]);
            self.1.drain(..n);
            Ok(n)
        }
    }

    impl Write for ChanWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let _ = self.0.send(buf.to_vec());
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn pipe() -> (ChanWriter, ChanReader) {
        let (tx, rx) = channel();
        (ChanWriter(tx), ChanReader(rx, Vec::new()))
    }

    #[test]
    fn frames_round_trip_and_survive_odd_input() {
        let a = json!({"seq": 1, "type": "request", "command": "initialize"});
        let b = json!({"type": "event", "event": "ünïcode ✓"});
        let mut wire = encode(&a);
        wire.extend_from_slice(b"\r\n"); // stray blank line
        wire.extend(encode(&b));
        let mut r = Cursor::new(wire);
        assert_eq!(read_message(&mut r).unwrap(), Some(a));
        assert_eq!(read_message(&mut r).unwrap(), Some(b));
        assert_eq!(read_message(&mut r).unwrap(), None);
        // A header without Content-Length is an error, not a hang.
        let mut bad = Cursor::new(b"X-Foo: 1\r\n\r\n{}".to_vec());
        assert!(read_message(&mut bad).is_err());
    }

    /// Run `adapter` against a client; the adapter reads requests and writes replies.
    fn with_mock(adapter: impl FnOnce(Box<dyn BufRead>, ChanWriter) + Send + 'static) -> Client {
        let (to_adapter_w, to_adapter_r) = pipe();
        let (from_adapter_w, from_adapter_r) = pipe();
        std::thread::spawn(move || {
            adapter(
                Box::new(std::io::BufReader::new(to_adapter_r)),
                from_adapter_w,
            )
        });
        Client::from_streams(Box::new(from_adapter_r), Box::new(to_adapter_w), None)
    }

    fn reply(w: &mut ChanWriter, req: &Value, ok: bool, body: Value) {
        let m = json!({"type": "response", "request_seq": req["seq"], "command": req["command"],
            "success": ok, "message": if ok { Value::Null } else { json!("boom") }, "body": body});
        w.write_all(&encode(&m)).unwrap();
    }

    #[test]
    fn matches_responses_out_of_order_and_routes_events() {
        let c = with_mock(|mut r, mut w| {
            let first = read_message(&mut r).unwrap().unwrap();
            let second = read_message(&mut r).unwrap().unwrap();
            // Answer the second request first.
            reply(&mut w, &second, true, json!({"which": "second"}));
            w.write_all(&encode(
                &json!({"type": "event", "event": "stopped", "body": {"reason": "breakpoint"}}),
            ))
            .unwrap();
            reply(&mut w, &first, false, Value::Null);
        });
        let p1 = c.send("one", json!({})).unwrap();
        let p2 = c.send("two", json!({})).unwrap();
        assert_eq!(
            p2.wait(Duration::from_secs(5)).unwrap(),
            json!({"which": "second"})
        );
        let e = c.next_event(Duration::from_secs(5)).unwrap();
        assert_eq!(e.name, "stopped");
        assert_eq!(e.body["reason"], "breakpoint");
        assert_eq!(
            p1.wait(Duration::from_secs(5)),
            Err(DapError::Adapter("boom".into()))
        );
    }

    #[test]
    fn reverse_requests_are_refused_and_exit_is_reported() {
        let c = with_mock(|mut r, mut w| {
            let req = read_message(&mut r).unwrap().unwrap();
            w.write_all(&encode(
                &json!({"seq": 9, "type": "request", "command": "runInTerminal"}),
            ))
            .unwrap();
            let refusal = read_message(&mut r).unwrap().unwrap();
            assert_eq!(refusal["success"], false);
            assert_eq!(refusal["request_seq"], 9);
            reply(&mut w, &req, true, json!({}));
            // Dropping the writer ends the stream.
        });
        c.request("initialize", json!({})).unwrap();
        let e = c.next_event(Duration::from_secs(5)).unwrap();
        assert_eq!(e.name, "kalem/adapterExited");
    }

    #[test]
    fn a_vanished_adapter_fails_waiting_requests() {
        let c = with_mock(|mut r, _w| {
            let _ = read_message(&mut r);
            // Exit without answering.
        });
        assert_eq!(c.request("threads", json!({})), Err(DapError::Closed));
    }

    #[test]
    fn missing_adapter_program_is_a_spawn_error() {
        let r = Client::spawn("definitely-not-an-adapter-kalem", &[], None);
        assert!(matches!(r, Err(DapError::Spawn(_))));
    }
}
