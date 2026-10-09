//! An in-process "language server" drives the client through the handshake and the requests.

use kalem_lsp::{encode, read_message, Client, Notification, Server};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

struct ChanReader(Receiver<Vec<u8>>, Vec<u8>);
struct ChanWriter(Sender<Vec<u8>>);

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

/// Starts the mock; returns the client end.
fn mock_server() -> Client {
    let (to_server, server_in) = channel();
    let (to_client, client_in) = channel();
    std::thread::spawn(move || {
        let mut r = std::io::BufReader::new(ChanReader(server_in, Vec::new()));
        let mut w = ChanWriter(to_client);
        let mut send = |v: Value| {
            w.write_all(&encode(&v)).unwrap();
        };
        while let Ok(Some(m)) = read_message(&mut r) {
            let id = m.get("id").cloned();
            match m["method"].as_str().unwrap_or("") {
                "initialize" => send(json!({"jsonrpc":"2.0","id":id,"result":{"capabilities":{
                    "completionProvider":{"triggerCharacters":[".", ":"]},
                    "hoverProvider": true,
                    "definitionProvider": true}}})),
                "textDocument/didOpen" => {
                    // The server asks the client for configuration, then publishes diagnostics.
                    send(
                        json!({"jsonrpc":"2.0","id":99,"method":"workspace/configuration",
                        "params":{"items":[{"section":"x"}]}}),
                    );
                    send(
                        json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics",
                        "params":{"uri": m["params"]["textDocument"]["uri"],
                        "diagnostics":[{"range":{"start":{"line":0,"character":0},
                        "end":{"line":0,"character":3}},"severity":1,"message":"bad"}]}}),
                    );
                }
                "textDocument/completion" => send(json!({"jsonrpc":"2.0","id":id,"result":
                    [{"label":"push"},{"label":"pop"}]})),
                "textDocument/hover" => send(json!({"jsonrpc":"2.0","id":id,"result":
                    {"contents":{"kind":"markdown","value":"**doc**"}}})),
                "textDocument/definition" => send(json!({"jsonrpc":"2.0","id":id,"result":
                    {"uri":"file:///a/b.rs","range":{"start":{"line":3,"character":4},
                    "end":{"line":3,"character":9}}}})),
                "shutdown" => send(json!({"jsonrpc":"2.0","id":id,"result":null})),
                "exit" => break,
                _ => {
                    if id.is_some() {
                        send(json!({"jsonrpc":"2.0","id":id,"result":null}));
                    }
                }
            }
        }
    });
    Client::from_streams(
        Box::new(ChanReader(client_in, Vec::new())),
        Box::new(ChanWriter(to_server)),
        None,
    )
}

#[test]
fn handshake_requests_and_notifications() {
    let server = Server::connect(mock_server(), std::path::Path::new("/work/space")).unwrap();
    assert!(server.can_complete() && server.can_hover() && server.can_define());
    assert_eq!(server.trigger_characters(), vec![".", ":"]);

    server
        .did_open("file:///work/space/x.rs", "rust", 1, "let x = 1;")
        .unwrap();
    match server.next_notification(Duration::from_secs(5)) {
        Some(Notification::Diagnostics { uri, diagnostics }) => {
            assert_eq!(uri, "file:///work/space/x.rs");
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].message, "bad");
        }
        other => panic!("expected diagnostics, got {other:?}"),
    }

    let c = server
        .completion("file:///work/space/x.rs", 0, 4, Some("."))
        .unwrap()
        .expect("items");
    let labels: Vec<String> = match c {
        lsp_types_reexport::CompletionResponse::Array(a) => {
            a.into_iter().map(|i| i.label).collect()
        }
        lsp_types_reexport::CompletionResponse::List(l) => {
            l.items.into_iter().map(|i| i.label).collect()
        }
    };
    assert_eq!(labels, vec!["push", "pop"]);

    let h = server
        .hover("file:///work/space/x.rs", 0, 1)
        .unwrap()
        .expect("hover");
    assert!(format!("{:?}", h.contents).contains("**doc**"));

    let d = server.definition("file:///work/space/x.rs", 0, 1).unwrap();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].target_uri.as_str(), "file:///a/b.rs");
    assert_eq!(d[0].target_selection_range.start.line, 3);

    server.shutdown();
}

mod lsp_types_reexport {
    pub use kalem_lsp::lsp_types::CompletionResponse;
}
