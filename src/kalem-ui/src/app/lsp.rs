//! Language servers: completion, hover, go to definition and diagnostics in the editor.
//!
//! One server per language pack runs for the workspace, started when the first file of that
//! language opens. The protocol work is in `kalem-lsp`; its calls block, so they run on
//! background threads and report back here.
//!
//! Document text is sent to the server in whole ("full sync"): edits are queued on the main
//! thread and sent by whichever background task runs first, under a lock, so a server never
//! sees an old text after a new one.

use super::*;
use gpui_kit::component::input::{
    CompletionProvider, DefinitionProvider, HoverProvider, Rope, RopeExt,
};
use kalem_lsp::lsp_types::{
    CompletionContext, CompletionResponse, CompletionTriggerKind, Hover, LocationLink,
    ShowDocumentParams,
};
use kalem_lsp::{path_to_uri, uri_to_path, Notification, Server};
use kalem_workspace::lsp::{lsp_for, LspSpec};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, PartialEq)]
enum Status {
    Starting,
    Ready,
    Missing,
    Failed(String),
}

struct ServerEntry {
    slot: Arc<OnceLock<Arc<Server>>>,
    status: Status,
}

/// What the server has been told about one document, and what is still to be sent.
#[derive(Default)]
struct DocSync {
    language_id: String,
    version: i32,
    /// Edited since the text was last queued.
    dirty: bool,
    /// `didOpen` has been sent.
    opened: bool,
    pending_open: Option<(i32, String)>,
    pending_change: Option<(i32, String)>,
}

impl DocSync {
    /// Queue the whole text as the newest state.
    fn queue(&mut self, text: String) {
        self.dirty = false;
        if self.opened {
            self.pending_change = Some((self.version, text));
        } else {
            self.pending_open = Some((self.version, text));
        }
    }
}

/// Send what is queued for a document. Runs on background threads; the lock keeps the order.
fn drain(sync: &Mutex<DocSync>, server: &Server, uri: &str) {
    let mut s = sync.lock().unwrap();
    if let Some((v, text)) = s.pending_open.take() {
        let language = s.language_id.clone();
        let _ = server.did_open(uri, &language, v, &text);
        s.opened = true;
    }
    if s.opened {
        if let Some((v, text)) = s.pending_change.take() {
            let _ = server.did_change(uri, v, &text);
        }
    }
}

struct Doc {
    pack: String,
    uri: String,
    editor: WeakEntity<EditorState>,
    sync: Arc<Mutex<DocSync>>,
    _sub: Subscription,
}

#[derive(Default)]
pub(super) struct LspState {
    servers: HashMap<String, ServerEntry>,
    docs: HashMap<String, Doc>,
    /// (errors, warnings) per file.
    counts: HashMap<String, (usize, usize)>,
}

/// The editor's view of one document's server.
#[derive(Clone)]
struct Providers {
    slot: Arc<OnceLock<Arc<Server>>>,
    uri: String,
    sync: Arc<Mutex<DocSync>>,
}

impl Providers {
    /// Queue the editor text if it changed since the server last got it (main thread).
    fn sync_text(&self, text: &Rope) {
        let mut s = self.sync.lock().unwrap();
        if s.dirty {
            s.queue(text.to_string());
        }
    }
}

fn failed(e: kalem_lsp::LspError) -> anyhow::Error {
    anyhow::anyhow!(e.to_string())
}

impl CompletionProvider for Providers {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let Some(server) = self.slot.get().cloned() else {
            return Task::ready(Ok(CompletionResponse::Array(Vec::new())));
        };
        self.sync_text(text);
        let pos = text.offset_to_position(offset);
        let typed = (trigger.trigger_kind == CompletionTriggerKind::TRIGGER_CHARACTER)
            .then_some(trigger.trigger_character)
            .flatten();
        let (sync, uri) = (self.sync.clone(), self.uri.clone());
        cx.background_executor().spawn(async move {
            drain(&sync, &server, &uri);
            let r = server
                .completion(&uri, pos.line, pos.character, typed.as_deref())
                .map_err(failed)?;
            Ok(r.unwrap_or(CompletionResponse::Array(Vec::new())))
        })
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        let mut chars = new_text.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return false;
        };
        c.is_alphanumeric()
            || c == '_'
            || self
                .slot
                .get()
                .is_some_and(|s| s.trigger_characters().iter().any(|t| t.starts_with(c)))
            || matches!(c, '.' | ':' | '>')
    }
}

impl HoverProvider for Providers {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Option<Hover>>> {
        let Some(server) = self.slot.get().cloned() else {
            return Task::ready(Ok(None));
        };
        self.sync_text(text);
        let pos = text.offset_to_position(offset);
        let (sync, uri) = (self.sync.clone(), self.uri.clone());
        cx.background_executor().spawn(async move {
            drain(&sync, &server, &uri);
            server.hover(&uri, pos.line, pos.character).map_err(failed)
        })
    }
}

impl DefinitionProvider for Providers {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Vec<LocationLink>>> {
        let Some(server) = self.slot.get().cloned() else {
            return Task::ready(Ok(Vec::new()));
        };
        self.sync_text(text);
        let pos = text.offset_to_position(offset);
        let (sync, uri) = (self.sync.clone(), self.uri.clone());
        cx.background_executor().spawn(async move {
            drain(&sync, &server, &uri);
            server
                .definition(&uri, pos.line, pos.character)
                .map_err(failed)
        })
    }
}

impl Kalem {
    // ------------------------------------------------------------ attach

    /// A file was opened in an editor: give it a language server when its pack names one.
    pub(super) fn lsp_attach(
        &mut self,
        rel: &str,
        editor: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) {
        if self.lsp.docs.contains_key(rel) {
            return;
        }
        let Some(spec) = lsp_for(rel) else { return };
        let Some(root) = self.ws.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        if !self.lsp.servers.contains_key(&spec.pack) {
            self.lsp_launch(&spec, root.clone(), cx);
        }
        let Some(entry) = self.lsp.servers.get(&spec.pack) else {
            return;
        };
        if matches!(entry.status, Status::Missing | Status::Failed(_)) {
            return;
        }
        let uri = path_to_uri(&root.join(rel));
        let sync = Arc::new(Mutex::new(DocSync {
            language_id: spec.language_id.clone(),
            version: 1,
            ..Default::default()
        }));
        sync.lock()
            .unwrap()
            .queue(editor.read(cx).value().to_string());
        let providers = Providers {
            slot: entry.slot.clone(),
            uri: uri.clone(),
            sync: sync.clone(),
        };
        let me = cx.entity().downgrade();
        editor.update(cx, |e, _| {
            let lsp = e.lsp_mut();
            lsp.completion_provider = Some(Rc::new(providers.clone()));
            lsp.hover_provider = Some(Rc::new(providers.clone()));
            lsp.definition_provider = Some(Rc::new(providers));
            lsp.show_document = Some(Rc::new(
                move |p: &ShowDocumentParams, window: &mut Window, cx: &mut App| {
                    let Some(path) = uri_to_path(p.uri.as_str()) else {
                        return false;
                    };
                    let (line, col) = p
                        .selection
                        .map(|s| (s.start.line as usize + 1, s.start.character as usize))
                        .unwrap_or((1, 0));
                    // The editor that asked is being updated right now: act afterwards.
                    let me = me.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = me.update(cx, |k, cx| k.lsp_show(&path, line, col, window, cx));
                    });
                    true
                },
            ));
        });
        let key = rel.to_string();
        let sub = cx.subscribe(editor, move |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.lsp_changed(&key, cx);
            }
        });
        self.lsp.docs.insert(
            rel.to_string(),
            Doc {
                pack: spec.pack,
                uri,
                editor: editor.downgrade(),
                sync,
                _sub: sub,
            },
        );
        if self
            .lsp
            .servers
            .get(&self.lsp.docs[rel].pack)
            .is_some_and(|e| e.status == Status::Ready)
        {
            self.lsp_send(rel, cx);
        }
    }

    /// Start the server of a language pack (once per workspace).
    fn lsp_launch(&mut self, spec: &LspSpec, root: PathBuf, cx: &mut Context<Self>) {
        let slot: Arc<OnceLock<Arc<Server>>> = Arc::default();
        if !spec.installed() {
            self.lsp.servers.insert(
                spec.pack.clone(),
                ServerEntry {
                    slot,
                    status: Status::Missing,
                },
            );
            self.output.push(format!(
                "{}: the language server `{}` is not installed. Install it (see docs/LANGUAGES.md) for completion, hover and diagnostics.",
                spec.language, spec.command
            ));
            cx.notify();
            return;
        }
        self.lsp.servers.insert(
            spec.pack.clone(),
            ServerEntry {
                slot,
                status: Status::Starting,
            },
        );
        let (pack, language) = (spec.pack.clone(), spec.language.clone());
        let (command, args) = (spec.command.clone(), spec.args.clone());
        let command_name = command.clone();
        cx.spawn(async move |this, cx| {
            let started = cx
                .background_executor()
                .spawn(async move { Server::start(&command, &args, &root) })
                .await;
            let server = match started {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    let why = format!(
                        "`{command_name}` did not start ({e}). Check that it is installed and runs from a terminal."
                    );
                    let _ = this.update(cx, |k, cx| k.lsp_failed(&pack, &language, why, cx));
                    return;
                }
            };
            let alive = this
                .update(cx, |k, cx| k.lsp_ready(&pack, server.clone(), cx))
                .unwrap_or(false);
            if alive {
                loop {
                    let s = server.clone();
                    let note = cx
                        .background_executor()
                        .spawn(async move { s.next_notification(Duration::from_millis(300)) })
                        .await;
                    let live = this
                        .update(cx, |k, _| {
                            k.lsp
                                .servers
                                .get(&pack)
                                .is_some_and(|e| e.status == Status::Ready)
                        })
                        .unwrap_or(false);
                    if !live {
                        break;
                    }
                    match note {
                        Some(Notification::Diagnostics { uri, diagnostics }) => {
                            let _ =
                                this.update(cx, |k, cx| k.lsp_diagnostics(&uri, diagnostics, cx));
                        }
                        Some(Notification::Message { level, text }) if level <= 2 => {
                            let _ = this.update(cx, |k, cx| {
                                k.output.push(format!("{language}: {text}"));
                                cx.notify();
                            });
                        }
                        Some(Notification::Exited) => {
                            let _ = this.update(cx, |k, cx| {
                                k.lsp_failed(&pack, &language, "the server exited".into(), cx)
                            });
                            break;
                        }
                        _ => {}
                    }
                }
            }
            cx.background_executor()
                .spawn(async move { server.shutdown() })
                .detach();
        })
        .detach();
        cx.notify();
    }

    fn lsp_ready(&mut self, pack: &str, server: Arc<Server>, cx: &mut Context<Self>) -> bool {
        let Some(entry) = self.lsp.servers.get_mut(pack) else {
            return false;
        };
        let _ = entry.slot.set(server);
        entry.status = Status::Ready;
        let waiting: Vec<String> = self
            .lsp
            .docs
            .iter()
            .filter(|(_, d)| d.pack == pack)
            .map(|(r, _)| r.clone())
            .collect();
        for rel in waiting {
            self.lsp_send(&rel, cx);
        }
        cx.notify();
        true
    }

    fn lsp_failed(&mut self, pack: &str, language: &str, why: String, cx: &mut Context<Self>) {
        if let Some(e) = self.lsp.servers.get_mut(pack) {
            e.status = Status::Failed(why.clone());
        }
        self.output
            .push(format!("{language}: language server stopped: {why}"));
        cx.notify();
    }

    // -------------------------------------------------------------- sync

    /// Send what is queued for a document, in the background.
    fn lsp_send(&mut self, rel: &str, cx: &mut Context<Self>) {
        let Some(doc) = self.lsp.docs.get(rel) else {
            return;
        };
        let Some(server) = self
            .lsp
            .servers
            .get(&doc.pack)
            .and_then(|e| e.slot.get().cloned())
        else {
            return;
        };
        let (sync, uri) = (doc.sync.clone(), doc.uri.clone());
        cx.background_executor()
            .spawn(async move { drain(&sync, &server, &uri) })
            .detach();
    }

    /// The text of a document changed: send it to the server shortly after typing pauses.
    fn lsp_changed(&mut self, rel: &str, cx: &mut Context<Self>) {
        let Some(doc) = self.lsp.docs.get(rel) else {
            return;
        };
        let version = {
            let mut s = doc.sync.lock().unwrap();
            s.version += 1;
            s.dirty = true;
            s.version
        };
        let rel = rel.to_string();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let _ = this.update(cx, |k, cx| {
                let Some(doc) = k.lsp.docs.get(&rel) else {
                    return;
                };
                let Some(editor) = doc.editor.upgrade() else {
                    return;
                };
                {
                    let mut s = doc.sync.lock().unwrap();
                    // Typing went on, or a request already sent this text.
                    if s.version != version || !s.dirty {
                        return;
                    }
                    s.queue(editor.read(cx).value().to_string());
                }
                k.lsp_send(&rel, cx);
            });
        })
        .detach();
    }

    /// The file was saved (rust-analyzer runs `cargo check` on save).
    pub(crate) fn lsp_saved(&mut self, rel: &str, text: &str, cx: &mut Context<Self>) {
        let Some(doc) = self.lsp.docs.get(rel) else {
            return;
        };
        let Some(server) = self
            .lsp
            .servers
            .get(&doc.pack)
            .and_then(|e| e.slot.get().cloned())
        else {
            return;
        };
        let (sync, uri, text) = (doc.sync.clone(), doc.uri.clone(), text.to_string());
        cx.background_executor()
            .spawn(async move {
                drain(&sync, &server, &uri);
                let _ = server.did_save(&uri, &text);
            })
            .detach();
    }

    /// The tab was closed.
    pub(crate) fn lsp_forget(&mut self, rel: &str, cx: &mut Context<Self>) {
        let Some(doc) = self.lsp.docs.remove(rel) else {
            return;
        };
        self.lsp.counts.remove(rel);
        if let Some(server) = self
            .lsp
            .servers
            .get(&doc.pack)
            .and_then(|e| e.slot.get().cloned())
        {
            let (sync, uri) = (doc.sync.clone(), doc.uri.clone());
            cx.background_executor()
                .spawn(async move {
                    drain(&sync, &server, &uri);
                    let _ = server.did_close(&uri);
                })
                .detach();
        }
        cx.notify();
    }

    // ------------------------------------------------------- from server

    fn lsp_diagnostics(
        &mut self,
        uri: &str,
        list: Vec<kalem_lsp::lsp_types::Diagnostic>,
        cx: &mut Context<Self>,
    ) {
        let Some(rel) = uri_to_path(uri).and_then(|p| self.rel_of(&p.display().to_string())) else {
            return;
        };
        let Some(editor) = self.lsp.docs.get(&rel).and_then(|d| d.editor.upgrade()) else {
            return;
        };
        let count = |sev| list.iter().filter(|d| d.severity == Some(sev)).count();
        let errors = count(kalem_lsp::lsp_types::DiagnosticSeverity::ERROR);
        let warnings = count(kalem_lsp::lsp_types::DiagnosticSeverity::WARNING);
        self.lsp.counts.insert(rel, (errors, warnings));
        editor.update(cx, |e, cx| {
            if let Some(set) = e.diagnostics_mut() {
                set.clear();
                set.extend(list);
            }
            cx.notify();
        });
        cx.notify();
    }

    /// Go to definition landed in another file.
    fn lsp_show(
        &mut self,
        path: &Path,
        line: usize,
        col: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.rel_of(&path.display().to_string()) {
            Some(rel) => self.open_at(&rel, line, col, window, cx),
            None => {
                self.status_text = format!(
                    "The definition is outside the workspace: {}",
                    path.display()
                );
                cx.notify();
            }
        }
    }

    /// For the status bar: the server's state and the file's problem counts.
    pub(super) fn lsp_status(&self, rel: &str) -> String {
        let Some(spec) = lsp_for(rel) else {
            return String::new();
        };
        let Some(entry) = self.lsp.servers.get(&spec.pack) else {
            return String::new();
        };
        let state = match &entry.status {
            Status::Starting => format!("{}: starting", spec.command),
            Status::Ready => String::new(),
            Status::Missing => format!("{} not installed", spec.command),
            Status::Failed(_) => format!("{} stopped", spec.command),
        };
        let (e, w) = self.lsp.counts.get(rel).copied().unwrap_or_default();
        let problems = match (e, w) {
            (0, 0) => String::new(),
            (e, 0) => format!("{e} error{}", if e == 1 { "" } else { "s" }),
            (0, w) => format!("{w} warning{}", if w == 1 { "" } else { "s" }),
            (e, w) => format!("{e} errors, {w} warnings"),
        };
        [state, problems]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("  ")
    }
}
