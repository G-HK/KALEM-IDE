//! Talks to a real language server (`pyright-langserver`) when one is installed; skipped
//! otherwise, so CI stays green on machines without it.

use kalem_lsp::{path_to_uri, Notification, Server};
use std::process::Command;
use std::time::{Duration, Instant};

const SOURCE: &str =
    "def double(n: int) -> int:\n    return n * 2\n\nvalue: str = double(3)\nprint(value)\n";

fn have_pyright() -> bool {
    Command::new("pyright-langserver")
        .arg("--version")
        .output()
        .is_ok()
}

#[test]
fn diagnostics_hover_completion_and_definition() {
    if !have_pyright() || cfg!(windows) {
        eprintln!("skipped: pyright-langserver is not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("kalem-lsp-pyright-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.py");
    std::fs::write(&file, SOURCE).unwrap();
    let uri = path_to_uri(&file);

    let server =
        Server::start("pyright-langserver", &["--stdio".to_string()], &dir).expect("server starts");
    server.did_open(&uri, "python", 1, SOURCE).unwrap();

    // `value: str = double(3)` is a type error on line 4.
    let end = Instant::now() + Duration::from_secs(60);
    let mut found = false;
    while Instant::now() < end && !found {
        if let Some(Notification::Diagnostics {
            uri: u,
            diagnostics,
        }) = server.next_notification(Duration::from_millis(500))
        {
            found = u == uri && diagnostics.iter().any(|d| d.range.start.line == 3);
        }
    }
    assert!(found, "no diagnostic for the type error");

    // Hover over `double` in `double(3)` (line 3, column 14).
    let hover = server.hover(&uri, 3, 14).unwrap().expect("hover text");
    assert!(format!("{hover:?}").contains("double"));

    // Go to definition: the `def` on line 0.
    let defs = server.definition(&uri, 3, 14).unwrap();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].target_selection_range.start.line, 0);

    // Completion after `pri` on a new line.
    let text2 = format!("{SOURCE}pri");
    server.did_change(&uri, 2, &text2).unwrap();
    let items = server
        .completion(&uri, 5, 3, None)
        .unwrap()
        .expect("completions");
    let labels: Vec<String> = match items {
        kalem_lsp::lsp_types::CompletionResponse::Array(a) => {
            a.into_iter().map(|i| i.label).collect()
        }
        kalem_lsp::lsp_types::CompletionResponse::List(l) => {
            l.items.into_iter().map(|i| i.label).collect()
        }
    };
    assert!(labels.iter().any(|l| l == "print"), "got {labels:?}");

    server.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
