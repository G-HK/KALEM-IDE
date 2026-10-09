//! One running language server: the handshake and the requests the editor needs.
//!
//! Positions are LSP positions (0-based line, UTF-16 column), which is what the editor
//! widget uses too.

use crate::client::{Client, LspError, Notification};
use crate::uri::path_to_uri;
use lsp_types::{CompletionResponse, GotoDefinitionResponse, Hover, LocationLink};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

const INIT_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Server {
    client: Client,
    capabilities: Value,
}

fn pos_params(uri: &str, line: u32, character: u32) -> Value {
    json!({
        "textDocument": {"uri": uri},
        "position": {"line": line, "character": character},
    })
}

fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Option<T> {
    if v.is_null() {
        None
    } else {
        serde_json::from_value(v).ok()
    }
}

impl Server {
    /// Start the server program and do the `initialize` handshake for a workspace.
    pub fn start(command: &str, args: &[String], root: &Path) -> Result<Server, LspError> {
        let client = Client::spawn(command, args, Some(root))?;
        match Server::handshake(client, root) {
            Ok(s) => Ok(s),
            Err((e, client)) => {
                client.kill();
                Err(e)
            }
        }
    }

    /// The handshake over an existing connection (tests use an in-process server).
    pub fn connect(client: Client, root: &Path) -> Result<Server, LspError> {
        Server::handshake(client, root).map_err(|(e, _)| e)
    }

    fn handshake(client: Client, root: &Path) -> Result<Server, (LspError, Client)> {
        let root_uri = path_to_uri(root);
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".into());
        let init = client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": {"name": "Kalem IDE"},
                "rootUri": root_uri,
                "rootPath": root.display().to_string(),
                "workspaceFolders": [{"uri": root_uri, "name": name}],
                "capabilities": {
                    "general": {"positionEncodings": ["utf-16"]},
                    "window": {"workDoneProgress": true},
                    "workspace": {"configuration": true, "workspaceFolders": true},
                    "textDocument": {
                        "synchronization": {"didSave": true},
                        "completion": {
                            "contextSupport": true,
                            "completionItem": {
                                "snippetSupport": false,
                                "deprecatedSupport": true,
                                "documentationFormat": ["markdown", "plaintext"],
                            },
                        },
                        "hover": {"contentFormat": ["markdown", "plaintext"]},
                        "definition": {"linkSupport": true},
                        "publishDiagnostics": {"relatedInformation": true},
                    },
                },
            }),
            INIT_TIMEOUT,
        );
        let init = match init {
            Ok(v) => v,
            Err(e) => return Err((e, client)),
        };
        if let Err(e) = client.notify("initialized", json!({})) {
            return Err((e, client));
        }
        Ok(Server {
            client,
            capabilities: init["capabilities"].clone(),
        })
    }

    /// Characters that should pop up completions (`.`, `::`, ...), as the server declares.
    pub fn trigger_characters(&self) -> Vec<String> {
        self.capabilities["completionProvider"]["triggerCharacters"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn can_complete(&self) -> bool {
        !self.capabilities["completionProvider"].is_null()
    }
    pub fn can_hover(&self) -> bool {
        !self.capabilities["hoverProvider"].is_null()
            && self.capabilities["hoverProvider"] != json!(false)
    }
    pub fn can_define(&self) -> bool {
        !self.capabilities["definitionProvider"].is_null()
            && self.capabilities["definitionProvider"] != json!(false)
    }

    // ------------------------------------------------------ documents

    pub fn did_open(
        &self,
        uri: &str,
        language_id: &str,
        version: i32,
        text: &str,
    ) -> Result<(), LspError> {
        self.client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": language_id, "version": version, "text": text,
            }}),
        )
    }

    /// Tell the server the whole new text (full sync works with every server).
    pub fn did_change(&self, uri: &str, version: i32, text: &str) -> Result<(), LspError> {
        self.client.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": uri, "version": version},
                "contentChanges": [{"text": text}],
            }),
        )
    }

    pub fn did_save(&self, uri: &str, text: &str) -> Result<(), LspError> {
        self.client.notify(
            "textDocument/didSave",
            json!({"textDocument": {"uri": uri}, "text": text}),
        )
    }

    pub fn did_close(&self, uri: &str) -> Result<(), LspError> {
        self.client.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        )
    }

    // -------------------------------------------------------- queries

    /// `trigger` is the typed trigger character, if the popup was opened by one.
    pub fn completion(
        &self,
        uri: &str,
        line: u32,
        character: u32,
        trigger: Option<&str>,
    ) -> Result<Option<CompletionResponse>, LspError> {
        let mut params = pos_params(uri, line, character);
        params["context"] = match trigger {
            Some(t) => json!({"triggerKind": 2, "triggerCharacter": t}),
            None => json!({"triggerKind": 1}),
        };
        Ok(parse(self.client.request(
            "textDocument/completion",
            params,
            REQUEST_TIMEOUT,
        )?))
    }

    pub fn hover(&self, uri: &str, line: u32, character: u32) -> Result<Option<Hover>, LspError> {
        Ok(parse(self.client.request(
            "textDocument/hover",
            pos_params(uri, line, character),
            REQUEST_TIMEOUT,
        )?))
    }

    pub fn definition(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<LocationLink>, LspError> {
        let v = self.client.request(
            "textDocument/definition",
            pos_params(uri, line, character),
            REQUEST_TIMEOUT,
        )?;
        Ok(match parse::<GotoDefinitionResponse>(v) {
            None => Vec::new(),
            Some(GotoDefinitionResponse::Link(l)) => l,
            Some(GotoDefinitionResponse::Scalar(loc)) => vec![link_of(loc)],
            Some(GotoDefinitionResponse::Array(a)) => a.into_iter().map(link_of).collect(),
        })
    }

    // ---------------------------------------------------- notifications

    pub fn next_notification(&self, timeout: Duration) -> Option<Notification> {
        self.client.next_notification(timeout)
    }

    /// Ask the server to exit, then make sure it does.
    pub fn shutdown(&self) {
        let _ = self
            .client
            .request("shutdown", Value::Null, Duration::from_secs(2));
        let _ = self.client.notify("exit", Value::Null);
        self.client.kill();
    }
}

fn link_of(loc: lsp_types::Location) -> LocationLink {
    LocationLink {
        origin_selection_range: None,
        target_uri: loc.uri,
        target_range: loc.range,
        target_selection_range: loc.range,
    }
}
