//! Drives a real `gdb --interpreter=dap` through a whole session. Skipped (with a note) on
//! machines without gdb 14+ or a C compiler, so CI on macOS and Windows stays green.

use kalem_dap::{DebugEvent, LaunchConfig, Session, SourceBreakpoint};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const PROGRAM: &str = "#include <stdio.h>\n\
int add(int a, int b) {\n\
    int sum = a + b;\n\
    return sum;\n\
}\n\
int main(void) {\n\
    int x = 3;\n\
    int y = 4;\n\
    int z = add(x, y);\n\
    printf(\"%d\\n\", z);\n\
    return 0;\n\
}\n";

fn have(cmd: &str, arg: &str) -> Option<String> {
    let out = Command::new(cmd).arg(arg).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// gdb added `--interpreter=dap` in version 14.
fn gdb_has_dap() -> bool {
    let Some(v) = have("gdb", "--version") else {
        return false;
    };
    v.lines()
        .next()
        .and_then(|l| l.split_whitespace().last())
        .and_then(|n| n.split('.').next())
        .and_then(|m| m.parse::<u32>().ok())
        .is_some_and(|m| m >= 14)
}

/// Every variable of a frame, across all its scopes (gdb splits arguments from locals).
fn all_vars(s: &Session, frame: i64) -> Vec<kalem_dap::Variable> {
    s.scopes(frame)
        .unwrap()
        .iter()
        .flat_map(|sc| s.variables(sc.variables_reference).unwrap())
        .collect()
}

fn wait_for(s: &Session, secs: u64, mut want: impl FnMut(&DebugEvent) -> bool) -> DebugEvent {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if let Some(e) = s.next_event(Duration::from_millis(200)) {
            if want(&e) {
                return e;
            }
        }
    }
    panic!("timed out waiting for a debug event");
}

#[test]
fn breakpoint_stack_variables_step_and_exit() {
    if !gdb_has_dap() || have("cc", "--version").is_none() || cfg!(windows) {
        eprintln!("skipped: needs gdb >= 14 and a C compiler");
        return;
    }
    let dir = std::env::temp_dir().join(format!("kalem-dap-gdb-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src: PathBuf = dir.join("prog.c");
    let exe = dir.join("prog");
    std::fs::write(&src, PROGRAM).unwrap();
    let ok = Command::new("cc")
        .args(["-g", "-O0", "-o"])
        .arg(&exe)
        .arg(&src)
        .status()
        .unwrap()
        .success();
    assert!(ok, "could not compile the test program");

    let mut bps = HashMap::new();
    bps.insert(src.display().to_string(), vec![SourceBreakpoint::at(3)]); // `int sum = a + b;`
    let session = Session::start(
        "gdb",
        &[],
        &LaunchConfig {
            program: exe.display().to_string(),
            cwd: Some(dir.clone()),
            ..Default::default()
        },
        &bps,
    )
    .expect("session starts");

    // Runs to the breakpoint inside add().
    let stopped = wait_for(&session, 30, |e| matches!(e, DebugEvent::Stopped { .. }));
    let DebugEvent::Stopped {
        thread_id, reason, ..
    } = stopped
    else {
        unreachable!()
    };
    assert_eq!(reason, "breakpoint");
    let tid = thread_id
        .or_else(|| session.threads().unwrap().first().map(|t| t.id))
        .expect("a thread");

    let frames = session.stack_trace(tid).unwrap();
    assert_eq!(frames[0].name, "add");
    assert_eq!(frames[0].line, 3);
    assert!(frames[0].file.as_deref().unwrap_or("").ends_with("prog.c"));
    assert_eq!(frames[1].name, "main");

    let vars = all_vars(&session, frames[0].id);
    let get = |n: &str| vars.iter().find(|v| v.name == n).map(|v| v.value.clone());
    assert_eq!(get("a").as_deref(), Some("3"));
    assert_eq!(get("b").as_deref(), Some("4"));

    // Step over: line 3 -> 4, and `sum` now holds 7.
    session.next(tid).unwrap();
    wait_for(&session, 30, |e| matches!(e, DebugEvent::Stopped { .. }));
    let frames = session.stack_trace(tid).unwrap();
    assert_eq!(frames[0].line, 4);
    let vars = all_vars(&session, frames[0].id);
    assert_eq!(
        vars.iter()
            .find(|v| v.name == "sum")
            .map(|v| v.value.as_str()),
        Some("7")
    );

    // Run to the end.
    session.resume(tid).unwrap();
    wait_for(&session, 30, |e| {
        matches!(e, DebugEvent::Exited(_) | DebugEvent::Terminated)
    });
    session.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn conditions_and_watch_expressions() {
    if !gdb_has_dap() || have("cc", "--version").is_none() || cfg!(windows) {
        eprintln!("skipped: needs gdb >= 14 and a C compiler");
        return;
    }
    let dir = std::env::temp_dir().join(format!("kalem-dap-gdb-cond-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (src, exe) = (dir.join("prog.c"), dir.join("prog"));
    std::fs::write(&src, PROGRAM).unwrap();
    let ok = Command::new("cc")
        .args(["-g", "-O0", "-o"])
        .arg(&exe)
        .arg(&src)
        .status()
        .unwrap()
        .success();
    assert!(ok, "could not compile the test program");

    // Line 3 only when a == 3 (true for the one call), and line 9 never (x == 99).
    let mut bps = HashMap::new();
    bps.insert(
        src.display().to_string(),
        vec![
            SourceBreakpoint {
                line: 3,
                condition: Some("a == 3".into()),
            },
            SourceBreakpoint {
                line: 9,
                condition: Some("x == 99".into()),
            },
        ],
    );
    let session = Session::start(
        "gdb",
        &[],
        &LaunchConfig {
            program: exe.display().to_string(),
            cwd: Some(dir.clone()),
            ..Default::default()
        },
        &bps,
    )
    .expect("session starts");

    let stopped = wait_for(&session, 30, |e| matches!(e, DebugEvent::Stopped { .. }));
    let DebugEvent::Stopped { thread_id, .. } = stopped else {
        unreachable!()
    };
    let tid = thread_id
        .or_else(|| session.threads().unwrap().first().map(|t| t.id))
        .expect("a thread");
    let frames = session.stack_trace(tid).unwrap();
    assert_eq!(frames[0].name, "add");
    assert_eq!(frames[0].line, 3);

    let w = session.evaluate("a + b", Some(frames[0].id)).unwrap();
    assert_eq!(w.value, "7");
    assert!(session
        .evaluate("no_such_name", Some(frames[0].id))
        .is_err());

    session.resume(tid).unwrap();
    wait_for(&session, 30, |e| {
        matches!(e, DebugEvent::Exited(_) | DebugEvent::Terminated)
    });
    session.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
