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

// ------------------------------------------------ references, rename, format

/// The identifier around a column (a character index) of a line.
fn word_at(line: &str, col: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut a = col.min(chars.len());
    // The cursor often sits just after the word.
    if a > 0 && (a == chars.len() || !is_word(chars[a])) && is_word(chars[a - 1]) {
        a -= 1;
    }
    if a >= chars.len() || !is_word(chars[a]) {
        return String::new();
    }
    let mut s = a;
    while s > 0 && is_word(chars[s - 1]) {
        s -= 1;
    }
    let mut e = a;
    while e < chars.len() && is_word(chars[e]) {
        e += 1;
    }
    chars[s..e].iter().collect()
}

/// What an editor command needs: the active file, its server and the cursor.
struct Target {
    rel: String,
    editor: Entity<EditorState>,
    server: Arc<Server>,
    sync: Arc<Mutex<DocSync>>,
    uri: String,
    pos: kalem_lsp::lsp_types::Position,
    word: String,
    version: i32,
}

/// State of the Rename Symbol prompt.
pub(super) struct RenameState {
    rel: String,
    pos: kalem_lsp::lsp_types::Position,
    old: String,
}

impl Kalem {
    /// The active file's server and cursor, or a status message saying why not.
    fn lsp_target(&mut self, cx: &mut Context<Self>) -> Option<Target> {
        let tab = self
            .active
            .and_then(|i| self.tabs.get(i))
            .filter(|t| t.diff.is_none())?;
        let rel = tab.path.clone();
        let editor = tab.editor.clone();
        let doc = self.lsp.docs.get(&rel);
        let server = doc.and_then(|d| {
            self.lsp
                .servers
                .get(&d.pack)
                .and_then(|e| e.slot.get().cloned())
        });
        let (Some(doc), Some(server)) = (doc, server) else {
            self.status_text = "No language server is running for this file".into();
            cx.notify();
            return None;
        };
        let (sync, uri) = (doc.sync.clone(), doc.uri.clone());
        let text = editor.read(cx).value();
        let p = editor.read(cx).cursor_position();
        let line = text.lines().nth(p.line as usize).unwrap_or("");
        let word = word_at(line, p.character as usize);
        // Make sure the server is sent the text the cursor refers to.
        let version = {
            let mut s = sync.lock().unwrap();
            if s.dirty {
                s.queue(text.to_string());
            }
            s.version
        };
        Some(Target {
            rel,
            editor,
            server,
            sync,
            uri,
            pos: kalem_lsp::lsp_types::Position::new(p.line, p.character),
            word,
            version,
        })
    }

    /// Shift+F12: every reference of the symbol under the cursor, in the Find Results.
    pub(super) fn find_references(&mut self, cx: &mut Context<Self>) {
        let Some(t) = self.lsp_target(cx) else { return };
        if !t.server.can_find_references() {
            self.status_text = "This language server cannot find references".into();
            cx.notify();
            return;
        }
        self.status_text = "Finding references...".into();
        let word = t.word.clone();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move {
                    drain(&t.sync, &t.server, &t.uri);
                    t.server.references(&t.uri, t.pos.line, t.pos.character)
                })
                .await;
            let _ = this.update(cx, |k, cx| k.show_references(&word, res, cx));
        })
        .detach();
        cx.notify();
    }

    fn show_references(
        &mut self,
        word: &str,
        res: Result<Vec<kalem_lsp::lsp_types::Location>, kalem_lsp::LspError>,
        cx: &mut Context<Self>,
    ) {
        let locations = match res {
            Ok(l) => l,
            Err(e) => {
                self.status_text = format!("Find references failed: {e}");
                cx.notify();
                return;
            }
        };
        let mut texts: HashMap<String, Vec<String>> = HashMap::new();
        let mut hits = Vec::new();
        for loc in &locations {
            let Some(rel) =
                uri_to_path(loc.uri.as_str()).and_then(|p| self.rel_of(&p.display().to_string()))
            else {
                continue;
            };
            if !texts.contains_key(&rel) {
                let open = self
                    .lsp
                    .docs
                    .get(&rel)
                    .and_then(|d| d.editor.upgrade())
                    .map(|e| e.read(cx).value().to_string());
                let text = open
                    .or_else(|| self.ws.as_ref().and_then(|w| w.read_file(&rel).ok()))
                    .unwrap_or_default();
                texts.insert(rel.clone(), text.lines().map(str::to_string).collect());
            }
            let line = loc.range.start.line as usize;
            hits.push(kalem_workspace::search::Hit {
                text: texts[&rel]
                    .get(line)
                    .map(|l| l.trim().to_string())
                    .unwrap_or_default(),
                path: rel,
                line: line + 1,
                col: loc.range.start.character as usize,
            });
        }
        hits.sort_by(|a, b| (&a.path, a.line, a.col).cmp(&(&b.path, b.line, b.col)));
        let files = hits
            .iter()
            .map(|h| h.path.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        self.find_query = format!("references of {word}");
        self.find = Some(kalem_workspace::search::SearchResult {
            hits,
            files_searched: files,
            truncated: false,
        });
        self.find_gen += 1;
        self.find_busy = false;
        self.find_cur = None;
        self.bottom = Bottom::Find;
        self.status_text = "Ready".into();
        cx.notify();
    }

    // ------------------------------------------------------------ rename

    /// F2: ask for the new name of the symbol under the cursor.
    pub(super) fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.lsp_target(cx) else { return };
        if !t.server.can_rename() {
            self.status_text = "This language server cannot rename symbols".into();
            cx.notify();
            return;
        }
        if t.word.is_empty() {
            self.status_text = "Put the cursor on a symbol to rename it".into();
            cx.notify();
            return;
        }
        self.rename = Some(RenameState {
            rel: t.rel,
            pos: t.pos,
            old: t.word.clone(),
        });
        let word = t.word;
        self.rename_input.update(cx, |i, cx| {
            i.set_value(word, window, cx);
            i.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn close_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rename.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    pub(super) fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.rename.take() else { return };
        let new_name = self.rename_input.read(cx).value().trim().to_string();
        window.focus(&self.focus, cx);
        if new_name.is_empty() || new_name == r.old {
            cx.notify();
            return;
        }
        let Some(t) = self.lsp_target(cx).filter(|t| t.rel == r.rel) else {
            return;
        };
        self.status_text = format!("Renaming {} to {new_name}...", r.old);
        let (old, pos) = (r.old, r.pos);
        cx.spawn_in(window, async move |this, cx| {
            let name = new_name.clone();
            let res = cx
                .background_executor()
                .spawn(async move {
                    drain(&t.sync, &t.server, &t.uri);
                    t.server.rename(&t.uri, pos.line, pos.character, &name)
                })
                .await;
            let _ = this.update_in(cx, |k, window, cx| match res {
                Ok(Some(edit)) => k.apply_workspace_edit(edit, &old, &new_name, window, cx),
                Ok(None) => {
                    k.status_text = format!("{old} cannot be renamed here");
                    cx.notify();
                }
                Err(e) => {
                    k.status_text = format!("Rename failed: {e}");
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Apply a server's edits: open files change in their editors (so they can be undone
    /// by closing without saving), the rest are rewritten on disk.
    fn apply_workspace_edit(
        &mut self,
        edit: kalem_lsp::lsp_types::WorkspaceEdit,
        old: &str,
        new: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut files = 0;
        let mut places = 0;
        let mut errors = Vec::new();
        for (uri, edits) in kalem_lsp::flatten(edit) {
            let Some(rel) = uri_to_path(&uri).and_then(|p| self.rel_of(&p.display().to_string()))
            else {
                continue;
            };
            places += edits.len();
            files += 1;
            if let Some(editor) = self.lsp.docs.get(&rel).and_then(|d| d.editor.upgrade()) {
                let text = editor.read(cx).value().to_string();
                let changed = kalem_lsp::apply_edits(&text, &edits);
                self.set_editor_text(&editor, changed, window, cx);
            } else if let Some(ws) = &self.ws {
                match ws.read_file(&rel) {
                    Ok(text) => {
                        if let Err(e) = ws.write_file(&rel, &kalem_lsp::apply_edits(&text, &edits))
                        {
                            errors.push(e);
                        }
                    }
                    Err(e) => errors.push(e),
                }
            }
        }
        self.refresh(cx);
        self.status_text = if errors.is_empty() {
            format!("Renamed {old} to {new}: {places} places in {files} files (save to keep the open ones)")
        } else {
            format!("Rename incomplete: {}", errors.join("; "))
        };
        cx.notify();
    }

    /// Replace an editor's text, keeping the cursor line.
    fn set_editor_text(
        &mut self,
        editor: &Entity<EditorState>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        editor.update(cx, |e, cx| {
            let pos = e.cursor_position();
            e.set_value(text, window, cx);
            e.set_cursor_position(pos, window, cx);
        });
        // `set_value` raises no change event: mark the tab and tell the server ourselves.
        let now = editor.read(cx).value();
        let mut changed = None;
        for t in self.tabs.iter_mut().filter(|t| &t.editor == editor) {
            t.dirty = now != t.saved;
            changed = Some(t.path.clone());
        }
        let changed = changed.or_else(|| {
            self.lsp
                .docs
                .iter()
                .find(|(_, d)| d.editor.upgrade().as_ref() == Some(editor))
                .map(|(r, _)| r.clone())
        });
        if let Some(rel) = changed {
            self.lsp_changed(&rel, cx);
        }
        cx.notify();
    }

    pub(super) fn rename_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let r = self.rename.as_ref()?;
        let theme = cx.theme().clone();
        let accent = rgba(self.palette.get("accent"));
        Some(
            div()
                .absolute()
                .top(px(72.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .key_context("Rename")
                        .w(px(420.))
                        .p_2()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(accent)
                        .bg(theme.popover)
                        .child(
                            div()
                                .pb_1()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!(
                                    "Rename '{}' (Enter to apply, Esc to cancel)",
                                    r.old
                                )),
                        )
                        .child(Input::new(&self.rename_input)),
                )
                .into_any_element(),
        )
    }

    // ------------------------------------------------------------ format

    /// Shift+Alt+F: let the language server format the whole document.
    pub(super) fn format_document(&mut self, cx: &mut Context<Self>) {
        let Some(t) = self.lsp_target(cx) else { return };
        if !t.server.can_format() {
            self.status_text = "This language server cannot format documents".into();
            cx.notify();
            return;
        }
        self.status_text = "Formatting...".into();
        let handle = self.handle;
        let (editor, server, sync, uri, version) = (t.editor, t.server, t.sync, t.uri, t.version);
        cx.spawn(async move |this, cx| {
            let (s2, u2, sync2) = (server.clone(), uri.clone(), sync.clone());
            let res = cx
                .background_executor()
                .spawn(async move {
                    drain(&sync2, &s2, &u2);
                    s2.format(&u2, 4, true)
                })
                .await;
            // Setting the text needs the window.
            let _ = cx.update_window(handle, |_, window, app| {
                this.update(app, |k, cx| match res {
                    Ok(edits) => {
                        // Typing went on while the server worked: do not clobber it.
                        if sync.lock().unwrap().version != version {
                            k.status_text = "Formatting skipped: the file changed".into();
                        } else if edits.is_empty() {
                            k.status_text = "Already formatted".into();
                        } else {
                            let text = editor.read(cx).value().to_string();
                            let out = kalem_lsp::apply_edits(&text, &edits);
                            k.set_editor_text(&editor, out, window, cx);
                            k.status_text = "Formatted".into();
                        }
                        cx.notify();
                    }
                    Err(e) => {
                        k.status_text = format!("Format failed: {e}");
                        cx.notify();
                    }
                })
            });
        })
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::word_at;

    #[test]
    fn finds_the_word_around_or_before_the_cursor() {
        assert_eq!(word_at("let total = add(x);", 5), "total");
        assert_eq!(word_at("let total = add(x);", 9), "total"); // just after it
        assert_eq!(word_at("let total = add(x);", 4), "total"); // at its start
        assert_eq!(word_at("a + b", 2), ""); // on the operator
        assert_eq!(word_at("   ", 1), "");
    }
}
