//! A debugging session: the usual conversation with a debug adapter.

use crate::client::{Client, DapError, Event, PendingResponse};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// How long to wait for the adapter to finish starting up.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// Turn a language pack's `adapter` (and `args`) into the command to run.
/// `gdb` has no separate adapter binary: it speaks DAP in a special mode.
pub fn adapter_command(adapter: &str, args: &[String]) -> (String, Vec<String>) {
    let stem = Path::new(adapter)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if stem == "gdb" && args.is_empty() {
        return (adapter.to_string(), vec!["--interpreter=dap".to_string()]);
    }
    (adapter.to_string(), args.to_vec())
}

/// What to start under the debugger.
#[derive(Debug, Clone, Default)]
pub struct LaunchConfig {
    /// Executable (or script, for interpreted languages).
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Stop at the first instruction / `main` instead of running to the first breakpoint.
    pub stop_on_entry: bool,
}

impl LaunchConfig {
    /// The `launch` arguments. Adapters ignore fields they do not know, so one set serves
    /// `lldb-dap`, `gdb` and the common script debuggers.
    fn to_json(&self) -> Value {
        json!({
            "program": self.program,
            "args": self.args,
            "cwd": self.cwd.as_ref().map(|c| c.display().to_string()),
            "stopOnEntry": self.stop_on_entry,
            "stopAtBeginningOfMainSubprogram": self.stop_on_entry,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    /// The adapter could place it.
    pub verified: bool,
    /// Where it really is (adapters may move a breakpoint to the next code line).
    pub line: Option<u32>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    pub id: i64,
    pub name: String,
    /// Absolute path of the source file, when the adapter knows it.
    pub file: Option<String>,
    /// 1-based.
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub name: String,
    pub variables_reference: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub type_name: Option<String>,
    /// Non-zero when the value has children (struct fields, array items).
    pub variables_reference: i64,
}

/// What the IDE cares about from the adapter's event stream.
#[derive(Debug, Clone, PartialEq)]
pub enum DebugEvent {
    Stopped {
        thread_id: Option<i64>,
        reason: String,
        description: Option<String>,
    },
    Continued,
    /// The debuggee ended.
    Exited(i64),
    /// The debug session ended.
    Terminated,
    Output {
        category: String,
        text: String,
    },
    /// The adapter process went away.
    AdapterExited,
    Other(String),
}

impl DebugEvent {
    pub fn from_event(e: &Event) -> DebugEvent {
        let s = |k: &str| e.body[k].as_str().map(str::to_string);
        match e.name.as_str() {
            "stopped" => DebugEvent::Stopped {
                thread_id: e.body["threadId"].as_i64(),
                reason: s("reason").unwrap_or_else(|| "unknown".into()),
                description: s("description").or_else(|| s("text")),
            },
            "continued" => DebugEvent::Continued,
            "exited" => DebugEvent::Exited(e.body["exitCode"].as_i64().unwrap_or(0)),
            "terminated" => DebugEvent::Terminated,
            "output" => DebugEvent::Output {
                category: s("category").unwrap_or_else(|| "console".into()),
                text: s("output").unwrap_or_default(),
            },
            "kalem/adapterExited" => DebugEvent::AdapterExited,
            other => DebugEvent::Other(other.to_string()),
        }
    }
}

pub struct Session {
    client: Client,
    /// Events that arrived during start-up, before the IDE was listening.
    backlog: Mutex<VecDeque<Event>>,
}

fn thread_of(v: &Value) -> Option<Thread> {
    Some(Thread {
        id: v["id"].as_i64()?,
        name: v["name"].as_str().unwrap_or("thread").to_string(),
    })
}

impl Session {
    /// Start the adapter, launch the program and set the initial breakpoints
    /// (`file -> 1-based lines`). The program runs (or stops on entry) when this returns.
    pub fn start(
        adapter: &str,
        adapter_args: &[String],
        launch: &LaunchConfig,
        breakpoints: &HashMap<String, Vec<u32>>,
    ) -> Result<Session, DapError> {
        let (program, args) = adapter_command(adapter, adapter_args);
        let client = Client::spawn(&program, &args, launch.cwd.as_deref())?;
        let session = Session {
            client,
            backlog: Mutex::new(VecDeque::new()),
        };
        match session.handshake(launch, breakpoints) {
            Ok(()) => Ok(session),
            Err(e) => {
                session.client.kill();
                Err(e)
            }
        }
    }

    fn handshake(
        &self,
        launch: &LaunchConfig,
        breakpoints: &HashMap<String, Vec<u32>>,
    ) -> Result<(), DapError> {
        self.client.request(
            "initialize",
            json!({
                "clientID": "kalem-ide",
                "clientName": "Kalem IDE",
                "adapterID": "kalem",
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "pathFormat": "path",
                "supportsVariableType": true,
                "supportsRunInTerminalRequest": false,
            }),
        )?;
        // Two families of adapters:
        //  * gdb says `initialized` right away and starts the program as soon as it gets
        //    `launch`, so breakpoints must be set first.
        //  * lldb-dap, debugpy, ... say `initialized` after `launch`, and only start the
        //    program at `configurationDone`.
        let early = self.wait_initialized(Duration::from_millis(250), None)?;
        let set_all = |me: &Session| -> Result<(), DapError> {
            for (file, lines) in breakpoints {
                if !lines.is_empty() {
                    me.set_breakpoints(file, lines)?;
                }
            }
            Ok(())
        };
        if early {
            set_all(self)?;
            let launch_req = self.client.send("launch", launch.to_json())?;
            // gdb answers `configurationDone` with "notStopped" while the program runs; there
            // is nothing to configure anyway.
            let _ = self.client.request("configurationDone", json!({}));
            launch_req.wait(STARTUP_TIMEOUT)?;
        } else {
            // The launch answer may only come after configurationDone: do not wait for it yet.
            let launch_req = self.client.send("launch", launch.to_json())?;
            self.wait_initialized(STARTUP_TIMEOUT, Some(&launch_req))?;
            set_all(self)?;
            let _ = self.client.request("configurationDone", json!({}));
            launch_req.wait(STARTUP_TIMEOUT)?;
        }
        Ok(())
    }

    /// Wait up to `timeout` for the `initialized` event. Adapter chatter before it (gdb's
    /// start-up banner) is dropped, other events are kept for the IDE. A failed `launch`
    /// ends the wait with its error. Returns whether the event arrived.
    fn wait_initialized(
        &self,
        timeout: Duration,
        launch: Option<&PendingResponse>,
    ) -> Result<bool, DapError> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(Err(e)) = launch.and_then(|l| l.peek_error()) {
                return Err(e);
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return if launch.is_some() {
                    Err(DapError::Timeout("initialized".into()))
                } else {
                    Ok(false)
                };
            }
            match self.client.next_event(left.min(Duration::from_millis(100))) {
                Some(e) if e.name == "initialized" => return Ok(true),
                Some(e) if e.name == "kalem/adapterExited" => return Err(DapError::Closed),
                Some(e) if e.name == "output" => {}
                Some(e) => self.backlog.lock().unwrap().push_back(e),
                None => {}
            }
        }
    }

    /// The next event from the adapter, within `timeout`.
    pub fn next_event(&self, timeout: Duration) -> Option<DebugEvent> {
        if let Some(e) = self.backlog.lock().unwrap().pop_front() {
            return Some(DebugEvent::from_event(&e));
        }
        self.client
            .next_event(timeout)
            .map(|e| DebugEvent::from_event(&e))
    }

    /// Replace the breakpoints of one file (`lines` are 1-based; empty clears the file).
    pub fn set_breakpoints(&self, file: &str, lines: &[u32]) -> Result<Vec<Breakpoint>, DapError> {
        let body = self.client.request(
            "setBreakpoints",
            json!({
                "source": {"path": file},
                "breakpoints": lines.iter().map(|l| json!({"line": l})).collect::<Vec<_>>(),
            }),
        )?;
        Ok(body["breakpoints"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|b| Breakpoint {
                        verified: b["verified"].as_bool().unwrap_or(false),
                        line: b["line"].as_u64().map(|l| l as u32),
                        message: b["message"].as_str().map(str::to_string),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn threads(&self) -> Result<Vec<Thread>, DapError> {
        let body = self.client.request("threads", json!({}))?;
        Ok(body["threads"]
            .as_array()
            .map(|a| a.iter().filter_map(thread_of).collect())
            .unwrap_or_default())
    }

    pub fn stack_trace(&self, thread_id: i64) -> Result<Vec<StackFrame>, DapError> {
        let body = self.client.request(
            "stackTrace",
            json!({"threadId": thread_id, "startFrame": 0, "levels": 50}),
        )?;
        Ok(body["stackFrames"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| {
                        Some(StackFrame {
                            id: f["id"].as_i64()?,
                            name: f["name"].as_str().unwrap_or("?").to_string(),
                            file: f["source"]["path"].as_str().map(str::to_string),
                            line: f["line"].as_u64().unwrap_or(0) as u32,
                            column: f["column"].as_u64().unwrap_or(0) as u32,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn scopes(&self, frame_id: i64) -> Result<Vec<Scope>, DapError> {
        let body = self
            .client
            .request("scopes", json!({"frameId": frame_id}))?;
        Ok(body["scopes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| {
                        Some(Scope {
                            name: s["name"].as_str().unwrap_or("?").to_string(),
                            variables_reference: s["variablesReference"].as_i64()?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn variables(&self, variables_reference: i64) -> Result<Vec<Variable>, DapError> {
        let body = self.client.request(
            "variables",
            json!({"variablesReference": variables_reference}),
        )?;
        Ok(body["variables"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|v| Variable {
                        name: v["name"].as_str().unwrap_or("?").to_string(),
                        value: v["value"].as_str().unwrap_or("").to_string(),
                        type_name: v["type"].as_str().map(str::to_string),
                        variables_reference: v["variablesReference"].as_i64().unwrap_or(0),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Evaluate an expression in a frame (watch / debug console).
    pub fn evaluate(&self, expression: &str, frame_id: Option<i64>) -> Result<String, DapError> {
        let body = self.client.request(
            "evaluate",
            json!({"expression": expression, "frameId": frame_id, "context": "repl"}),
        )?;
        Ok(body["result"].as_str().unwrap_or("").to_string())
    }

    pub fn resume(&self, thread_id: i64) -> Result<(), DapError> {
        self.client
            .request("continue", json!({"threadId": thread_id}))
            .map(|_| ())
    }

    /// Step over.
    pub fn next(&self, thread_id: i64) -> Result<(), DapError> {
        self.client
            .request("next", json!({"threadId": thread_id}))
            .map(|_| ())
    }

    pub fn step_in(&self, thread_id: i64) -> Result<(), DapError> {
        self.client
            .request("stepIn", json!({"threadId": thread_id}))
            .map(|_| ())
    }

    pub fn step_out(&self, thread_id: i64) -> Result<(), DapError> {
        self.client
            .request("stepOut", json!({"threadId": thread_id}))
            .map(|_| ())
    }

    pub fn pause(&self, thread_id: i64) -> Result<(), DapError> {
        self.client
            .request("pause", json!({"threadId": thread_id}))
            .map(|_| ())
    }

    /// End the session and the debuggee, then the adapter.
    pub fn stop(&self) {
        if let Ok(p) = self
            .client
            .send("disconnect", json!({"terminateDebuggee": true}))
        {
            let _ = p.wait(Duration::from_secs(3));
        }
        self.client.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gdb_is_started_in_dap_mode() {
        assert_eq!(
            adapter_command("gdb", &[]),
            ("gdb".to_string(), vec!["--interpreter=dap".to_string()])
        );
        assert_eq!(
            adapter_command("/usr/bin/gdb", &[]).1,
            vec!["--interpreter=dap".to_string()]
        );
        let args = vec!["--port".to_string(), "1".to_string()];
        assert_eq!(adapter_command("dlv", &args), ("dlv".to_string(), args));
        assert_eq!(
            adapter_command("gdb", &["-q".to_string()]).1,
            vec!["-q".to_string()]
        );
    }

    #[test]
    fn events_are_classified() {
        let ev = |name: &str, body: Value| {
            DebugEvent::from_event(&Event {
                name: name.into(),
                body,
            })
        };
        assert_eq!(
            ev(
                "stopped",
                json!({"reason": "breakpoint", "threadId": 2, "text": "hit"})
            ),
            DebugEvent::Stopped {
                thread_id: Some(2),
                reason: "breakpoint".into(),
                description: Some("hit".into())
            }
        );
        assert_eq!(ev("exited", json!({"exitCode": 3})), DebugEvent::Exited(3));
        assert_eq!(ev("terminated", Value::Null), DebugEvent::Terminated);
        assert_eq!(
            ev("output", json!({"output": "hi\n"})),
            DebugEvent::Output {
                category: "console".into(),
                text: "hi\n".into()
            }
        );
        assert_eq!(
            ev("thread", Value::Null),
            DebugEvent::Other("thread".into())
        );
    }

    #[test]
    fn launch_arguments_cover_common_adapters() {
        let l = LaunchConfig {
            program: "/x/app".into(),
            args: vec!["-v".into()],
            cwd: Some("/x".into()),
            stop_on_entry: true,
        }
        .to_json();
        assert_eq!(l["program"], "/x/app");
        assert_eq!(l["stopOnEntry"], true);
        assert_eq!(l["stopAtBeginningOfMainSubprogram"], true);
        assert_eq!(l["args"], json!(["-v"]));
    }
}
