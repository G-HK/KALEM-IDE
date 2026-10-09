//! Debugging: breakpoints, starting a session, stepping, and the Debug panel.
//!
//! The protocol work is in `kalem-dap`; its calls block, so they all run on background threads
//! and report back here.

use super::*;
use gpui_kit::component::input::{
    RangeDecoration, RangeDecorationCollection, RangeDecorationStyle,
};
use kalem_dap::{DebugEvent, LaunchConfig, Session, StackFrame, Variable};
use kalem_workspace::debug::DebugPlan;
use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::Arc;

/// Highlights of one editor: breakpoint lines and the line where execution stopped.
pub(crate) struct Marks {
    breakpoints: RangeDecorationCollection,
    current: RangeDecorationCollection,
}

#[derive(Clone, PartialEq)]
pub(super) enum Status {
    Building,
    Starting,
    Running,
    Paused(String),
    Exited(i64),
}

pub(super) struct DebugState {
    gen: u64,
    status: Status,
    session: Option<Arc<Session>>,
    thread: Option<i64>,
    frames: Vec<StackFrame>,
    frame_ix: usize,
    vars: Vec<(String, Vec<Variable>)>,
    /// Files whose breakpoints the adapter has been told about (so cleared ones are cleared).
    known_files: BTreeSet<String>,
}

#[derive(Clone, Copy)]
pub(super) enum Step {
    Over,
    Into,
    Out,
}

/// What the panel shows after the program stopped.
struct Snapshot {
    thread: i64,
    frames: Vec<StackFrame>,
    vars: Vec<(String, Vec<Variable>)>,
}

const MAX_VARS: usize = 300;

fn frame_vars(s: &Session, frame_id: i64) -> Vec<(String, Vec<Variable>)> {
    s.scopes(frame_id)
        .unwrap_or_default()
        .into_iter()
        // Hundreds of CPU registers would bury the variables that matter.
        .filter(|sc| !sc.name.eq_ignore_ascii_case("registers"))
        .map(|sc| {
            let mut v = s.variables(sc.variables_reference).unwrap_or_default();
            v.truncate(MAX_VARS);
            (sc.name, v)
        })
        .collect()
}

fn snapshot(s: &Session, thread: Option<i64>) -> Snapshot {
    let thread = thread
        .or_else(|| s.threads().ok().and_then(|t| t.first().map(|t| t.id)))
        .unwrap_or(1);
    let frames = s.stack_trace(thread).unwrap_or_default();
    let vars = frames
        .first()
        .map(|f| frame_vars(s, f.id))
        .unwrap_or_default();
    Snapshot {
        thread,
        frames,
        vars,
    }
}

/// Byte ranges of whole lines (1-based numbers), including the line break.
fn line_ranges(text: &str, lines: &[u32]) -> Vec<Range<usize>> {
    let mut starts = vec![0usize];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    lines
        .iter()
        .filter_map(|&l| {
            let i = (l as usize).checked_sub(1)?;
            let start = *starts.get(i)?;
            let end = starts.get(i + 1).copied().unwrap_or(text.len());
            (end > start).then_some(start..end)
        })
        .collect()
}

impl DebugState {
    fn new(gen: u64, status: Status) -> Self {
        DebugState {
            gen,
            status,
            session: None,
            thread: None,
            frames: Vec::new(),
            frame_ix: 0,
            vars: Vec::new(),
            known_files: BTreeSet::new(),
        }
    }
}

impl Kalem {
    pub(super) fn debug_paused_now(&self) -> bool {
        matches!(
            self.dbg.as_ref().map(|d| &d.status),
            Some(Status::Paused(_))
        )
    }

    fn dbg_live(&self, gen: u64) -> bool {
        self.dbg.as_ref().is_some_and(|d| d.gen == gen)
    }

    // ------------------------------------------------------------ paths

    fn abs_of(&self, rel: &str) -> Option<String> {
        self.ws
            .as_ref()
            .map(|ws| ws.root.join(rel).display().to_string())
    }

    /// Workspace-relative path (with `/`) of an absolute file, if it is inside the workspace.
    fn rel_of(&self, abs: &str) -> Option<String> {
        let root = self
            .ws
            .as_ref()?
            .root
            .display()
            .to_string()
            .replace('\\', "/");
        let abs = abs.replace('\\', "/");
        let root = root.trim_end_matches('/');
        let same = |a: &str, b: &str| {
            if cfg!(windows) {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        };
        if abs.len() > root.len() + 1
            && same(&abs[..root.len()], root)
            && abs.as_bytes()[root.len()] == b'/'
        {
            Some(abs[root.len() + 1..].to_string())
        } else {
            None
        }
    }

    // ------------------------------------------------------ breakpoints

    fn abs_breakpoints(&self) -> std::collections::HashMap<String, Vec<u32>> {
        self.breakpoints
            .iter()
            .filter_map(|(rel, lines)| Some((self.abs_of(rel)?, lines.iter().copied().collect())))
            .collect()
    }

    /// F9: toggle a breakpoint on the cursor line of the active file.
    pub(super) fn toggle_breakpoint(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self
            .active
            .and_then(|i| self.tabs.get(i))
            .filter(|t| t.diff.is_none())
        else {
            return;
        };
        let line = tab.editor.read(cx).cursor_position().line + 1;
        let path = tab.path.clone();
        let set = self.breakpoints.entry(path.clone()).or_default();
        if !set.remove(&line) {
            set.insert(line);
        }
        self.sync_breakpoints(&path, cx);
    }

    fn remove_breakpoint(&mut self, path: &str, line: u32, cx: &mut Context<Self>) {
        if let Some(set) = self.breakpoints.get_mut(path) {
            set.remove(&line);
        }
        self.sync_breakpoints(path, cx);
    }

    /// Redraw the markers and, while debugging, tell the adapter.
    fn sync_breakpoints(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.breakpoints.get(path).is_some_and(BTreeSet::is_empty) {
            self.breakpoints.remove(path);
        }
        self.apply_marks(cx);
        let session = self.dbg.as_ref().and_then(|d| d.session.clone());
        if let (Some(s), Some(abs)) = (session, self.abs_of(path)) {
            let lines: Vec<u32> = self
                .breakpoints
                .get(path)
                .map(|s| s.iter().copied().collect())
                .unwrap_or_default();
            cx.background_executor()
                .spawn(async move {
                    let _ = s.set_breakpoints(&abs, &lines);
                })
                .detach();
        }
        cx.notify();
    }

    /// Paint breakpoint lines and the current execution line into every open editor.
    pub(super) fn apply_marks(&mut self, cx: &mut Context<Self>) {
        let danger = rgba(self.palette.get("danger")).opacity(0.28);
        let current = rgba(self.palette.get("warning")).opacity(0.35);
        for tab in self.tabs.iter_mut().filter(|t| t.diff.is_none()) {
            let bps: Vec<u32> = self
                .breakpoints
                .get(&tab.path)
                .map(|s| s.iter().copied().collect())
                .unwrap_or_default();
            let pc: Vec<u32> = match &self.pc {
                Some((p, l)) if *p == tab.path => vec![*l],
                _ => Vec::new(),
            };
            if tab.marks.is_none() && bps.is_empty() && pc.is_empty() {
                continue;
            }
            let marks = tab.marks.get_or_insert_with(|| {
                tab.editor.update(cx, |e, cx| Marks {
                    breakpoints: e.create_range_decorations_collection(Vec::new(), cx),
                    current: e.create_range_decorations_collection(Vec::new(), cx),
                })
            });
            let text = tab.editor.read(cx).value().to_string();
            let fill = |r: Range<usize>, c: Hsla| {
                RangeDecoration::new(r)
                    .with_style(RangeDecorationStyle::Fill)
                    .with_color(c)
            };
            marks.breakpoints.set(
                line_ranges(&text, &bps)
                    .into_iter()
                    .map(|r| fill(r, danger))
                    .collect(),
                cx,
            );
            marks.current.set(
                line_ranges(&text, &pc)
                    .into_iter()
                    .map(|r| fill(r, current))
                    .collect(),
                cx,
            );
        }
    }

    // ----------------------------------------------------------- start

    /// F5: build, then start the program under the debugger; when paused, continue.
    pub(super) fn start_debugging(&mut self, cx: &mut Context<Self>) {
        if self.dbg.is_some() {
            if self.debug_paused_now() {
                self.debug_resume(cx);
            }
            return;
        }
        if self.task.is_some() {
            self.status_text = "Wait for the running task to finish first".into();
            cx.notify();
            return;
        }
        let Some(ws) = &self.ws else {
            self.message = Some("Open a folder first".into());
            cx.notify();
            return;
        };
        let startup = self.startup.clone().or_else(|| ws.default_startup());
        match ws.debug_plan(startup.as_deref(), self.config) {
            Err(e) => {
                self.bottom = Bottom::Output;
                self.output.push(format!("Cannot start debugging: {e}"));
                self.status_text = e;
            }
            Ok(plan) => {
                self.dbg_gen += 1;
                self.dbg = Some(DebugState::new(self.dbg_gen, Status::Building));
                self.pending_debug = Some(plan.clone());
                self.run_plan(plan.build, cx);
            }
        }
        cx.notify();
    }

    /// The build finished: start the adapter and the program, then follow its events.
    pub(super) fn begin_session(&mut self, plan: DebugPlan, cx: &mut Context<Self>) {
        let gen = self.dbg_gen;
        if let Some(d) = self.dbg.as_mut() {
            d.status = Status::Starting;
        }
        let known: BTreeSet<String> = self.breakpoints.keys().cloned().collect();
        if let Some(d) = self.dbg.as_mut() {
            d.known_files = known;
        }
        self.bottom = Bottom::Debug;
        self.status_text = "Starting the debugger...".into();
        let handle = self.handle;
        let breakpoints = self.abs_breakpoints();
        let launch = LaunchConfig {
            program: plan.program.display().to_string(),
            args: Vec::new(),
            cwd: Some(plan.cwd.clone()),
            stop_on_entry: false,
        };
        let (adapter, adapter_args, hint) = (plan.adapter, plan.adapter_args, plan.install_hint);
        cx.spawn(async move |this, cx| {
            let started = cx
                .background_executor()
                .spawn(
                    async move { Session::start(&adapter, &adapter_args, &launch, &breakpoints) },
                )
                .await;
            let session = match started {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    let _ =
                        this.update(cx, |k, cx| k.debug_failed(gen, format!("{e}\n{hint}"), cx));
                    return;
                }
            };
            let alive = this
                .update(cx, |k, cx| k.debug_attached(gen, session.clone(), cx))
                .unwrap_or(false);
            if !alive {
                stop_in_background(cx, session);
                return;
            }
            let mut exited_idle = 0;
            loop {
                let s = session.clone();
                let ev = cx
                    .background_executor()
                    .spawn(async move { s.next_event(Duration::from_millis(300)) })
                    .await;
                if !this.update(cx, |k, _| k.dbg_live(gen)).unwrap_or(false) {
                    break;
                }
                let Some(ev) = ev else {
                    // After the program exited some adapters never say `terminated`.
                    let exited = this
                        .update(cx, |k, _| {
                            matches!(k.dbg.as_ref().map(|d| &d.status), Some(Status::Exited(_)))
                        })
                        .unwrap_or(false);
                    exited_idle = if exited { exited_idle + 1 } else { 0 };
                    if exited_idle >= 8 {
                        let _ = this.update(cx, |k, cx| k.debug_ended(gen, cx));
                        break;
                    }
                    continue;
                };
                match ev {
                    DebugEvent::Stopped {
                        thread_id,
                        reason,
                        description,
                    } => {
                        let s = session.clone();
                        let snap = cx
                            .background_executor()
                            .spawn(async move { snapshot(&s, thread_id) })
                            .await;
                        let _ = cx.update_window(handle, |_, window, app| {
                            this.update(app, |k, cx| {
                                k.debug_paused(gen, reason, description, snap, window, cx)
                            })
                        });
                    }
                    DebugEvent::Continued => {
                        let _ = this.update(cx, |k, cx| k.debug_running(gen, cx));
                    }
                    DebugEvent::Output { category, text } => {
                        if category != "telemetry" {
                            let _ = this.update(cx, |k, cx| k.debug_output(&text, cx));
                        }
                    }
                    DebugEvent::Exited(code) => {
                        let _ = this.update(cx, |k, cx| k.debug_exited(gen, code, cx));
                    }
                    DebugEvent::Terminated | DebugEvent::AdapterExited => {
                        let _ = this.update(cx, |k, cx| k.debug_ended(gen, cx));
                        break;
                    }
                    DebugEvent::Other(_) => {}
                }
            }
            stop_in_background(cx, session);
        })
        .detach();
        cx.notify();
    }

    fn debug_failed(&mut self, gen: u64, error: String, cx: &mut Context<Self>) {
        if !self.dbg_live(gen) {
            return;
        }
        self.dbg = None;
        self.bottom = Bottom::Output;
        self.output.push("Could not start the debugger:".into());
        self.output.extend(error.lines().map(str::to_string));
        self.status_text = "Could not start the debugger".into();
        cx.notify();
    }

    fn debug_attached(&mut self, gen: u64, session: Arc<Session>, cx: &mut Context<Self>) -> bool {
        let Some(d) = self.dbg.as_mut().filter(|d| d.gen == gen) else {
            return false;
        };
        d.session = Some(session);
        if d.status == Status::Starting {
            d.status = Status::Running;
        }
        self.status_text = "Debugging: running".into();
        cx.notify();
        true
    }

    fn debug_output(&mut self, text: &str, cx: &mut Context<Self>) {
        // gdb reports breakpoints it will resolve once the program is loaded as "pending".
        let noise = |l: &str| {
            l.starts_with("No source file named")
                || (l.starts_with("Breakpoint ") && l.ends_with("pending."))
        };
        self.output.extend(
            text.trim_end()
                .lines()
                .filter(|l| !noise(l))
                .map(str::to_string),
        );
        self.output_scroll.scroll_to_bottom();
        cx.notify();
    }

    fn debug_running(&mut self, gen: u64, cx: &mut Context<Self>) {
        if !self.dbg_live(gen) {
            return;
        }
        if let Some(d) = self.dbg.as_mut() {
            if !matches!(d.status, Status::Exited(_)) {
                d.status = Status::Running;
            }
        }
        self.pc = None;
        self.apply_marks(cx);
        self.status_text = "Debugging: running".into();
        cx.notify();
    }

    fn debug_exited(&mut self, gen: u64, code: i64, cx: &mut Context<Self>) {
        if !self.dbg_live(gen) {
            return;
        }
        if let Some(d) = self.dbg.as_mut() {
            d.status = Status::Exited(code);
        }
        self.output
            .push(format!("The program exited with code {code}"));
        self.pc = None;
        self.apply_marks(cx);
        cx.notify();
    }

    fn debug_ended(&mut self, gen: u64, cx: &mut Context<Self>) {
        if !self.dbg_live(gen) {
            return;
        }
        self.dbg = None;
        self.pc = None;
        self.apply_marks(cx);
        self.status_text = "Debugging ended".into();
        self.output.push("--- Debugging ended ---".into());
        cx.notify();
    }

    fn debug_paused(
        &mut self,
        gen: u64,
        reason: String,
        description: Option<String>,
        snap: Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.dbg_live(gen) {
            return;
        }
        let text = match description.filter(|d| !d.is_empty()) {
            Some(d) => format!("{reason}: {d}"),
            None => reason,
        };
        let top = snap.frames.first().cloned();
        if let Some(d) = self.dbg.as_mut() {
            d.status = Status::Paused(text.clone());
            d.thread = Some(snap.thread);
            d.frames = snap.frames;
            d.frame_ix = 0;
            d.vars = snap.vars;
        }
        self.status_text = format!("Debugging: paused ({text})");
        self.bottom = Bottom::Debug;
        self.show_frame_source(top, window, cx);
    }

    /// Open the file of a frame at its line and mark it as the current line.
    fn show_frame_source(
        &mut self,
        frame: Option<StackFrame>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rel = frame
            .as_ref()
            .and_then(|f| f.file.as_deref())
            .and_then(|f| self.rel_of(f));
        match (rel, frame) {
            (Some(rel), Some(f)) => {
                self.pc = Some((rel.clone(), f.line));
                self.open_at(&rel, f.line as usize, 0, window, cx);
            }
            _ => self.pc = None,
        }
        self.apply_marks(cx);
        cx.notify();
    }

    // ---------------------------------------------------------- control

    fn session_and_thread(&self) -> Option<(Arc<Session>, i64)> {
        let d = self.dbg.as_ref()?;
        Some((d.session.clone()?, d.thread.unwrap_or(1)))
    }

    fn run_in_background(
        &mut self,
        cx: &mut Context<Self>,
        what: &'static str,
        f: impl FnOnce(&Session, i64) -> Result<(), kalem_dap::DapError> + Send + 'static,
    ) {
        let Some((s, tid)) = self.session_and_thread() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { f(&s, tid) })
                .await;
            if let Err(e) = r {
                let _ = this.update(cx, |k, cx| {
                    k.debug_output(&format!("{what} failed: {e}"), cx)
                });
            }
        })
        .detach();
    }

    /// Mark the program as running (the adapter's `continued` event may come later or never).
    fn mark_running(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.dbg.as_mut() {
            d.status = Status::Running;
            d.frames.clear();
            d.vars.clear();
        }
        self.pc = None;
        self.apply_marks(cx);
        self.status_text = "Debugging: running".into();
        cx.notify();
    }

    /// Every file's breakpoints, to send before the program runs on: some adapters (gdb)
    /// cannot change breakpoints while the program runs, so edits made then are applied now.
    fn breakpoint_sync(&mut self) -> Vec<(String, Vec<u32>)> {
        let mut files: BTreeSet<String> = self.breakpoints.keys().cloned().collect();
        if let Some(d) = self.dbg.as_mut() {
            files.extend(d.known_files.iter().cloned());
            d.known_files = self.breakpoints.keys().cloned().collect();
        }
        files
            .into_iter()
            .filter_map(|rel| {
                let lines = self
                    .breakpoints
                    .get(&rel)
                    .map(|s| s.iter().copied().collect())
                    .unwrap_or_default();
                Some((self.abs_of(&rel)?, lines))
            })
            .collect()
    }

    pub(super) fn debug_resume(&mut self, cx: &mut Context<Self>) {
        if !self.debug_paused_now() {
            return;
        }
        let sync = self.breakpoint_sync();
        self.mark_running(cx);
        self.run_in_background(cx, "Continue", move |s, t| {
            for (file, lines) in &sync {
                let _ = s.set_breakpoints(file, lines);
            }
            s.resume(t)
        });
    }

    pub(super) fn debug_step(&mut self, step: Step, cx: &mut Context<Self>) {
        if !self.debug_paused_now() {
            return;
        }
        let sync = self.breakpoint_sync();
        self.mark_running(cx);
        self.run_in_background(cx, "Step", move |s, t| {
            for (file, lines) in &sync {
                let _ = s.set_breakpoints(file, lines);
            }
            match step {
                Step::Over => s.next(t),
                Step::Into => s.step_in(t),
                Step::Out => s.step_out(t),
            }
        });
    }

    pub(super) fn debug_pause(&mut self, cx: &mut Context<Self>) {
        if self.dbg.is_some() && !self.debug_paused_now() {
            self.run_in_background(cx, "Pause", |s, t| s.pause(t));
        }
    }

    pub(super) fn stop_debugging(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.dbg.take() else { return };
        if let Some(t) = self.task.take() {
            t.task.cancel(); // still building
        }
        self.pending_debug = None;
        self.pc = None;
        self.apply_marks(cx);
        if let Some(s) = d.session {
            cx.background_executor()
                .spawn(async move { s.stop() })
                .detach();
        }
        self.status_text = "Debugging stopped".into();
        cx.notify();
    }

    /// Click in the Call Stack: show that frame's source and variables.
    fn select_frame(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.dbg.as_mut() else { return };
        let Some(frame) = d.frames.get(ix).cloned() else {
            return;
        };
        d.frame_ix = ix;
        let (gen, session) = (d.gen, d.session.clone());
        self.show_frame_source(Some(frame.clone()), window, cx);
        let Some(s) = session else { return };
        cx.spawn(async move |this, cx| {
            let vars = cx
                .background_executor()
                .spawn(async move { frame_vars(&s, frame.id) })
                .await;
            let _ = this.update(cx, |k, cx| {
                if let Some(d) = k.dbg.as_mut().filter(|d| d.gen == gen && d.frame_ix == ix) {
                    d.vars = vars;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    // ------------------------------------------------------------ panel

    /// The Debug tab: Call Stack | Variables | Breakpoints.
    pub(super) fn debug_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let (list_active, border) = (theme.list_active, theme.border);
        let danger = rgba(self.palette.get("danger"));
        let warning = rgba(self.palette.get("warning"));

        let status_line = match self.dbg.as_ref().map(|d| &d.status) {
            None => "Not debugging. Press F5 to build and start; F9 sets a breakpoint.".to_string(),
            Some(Status::Building) => "Building...".into(),
            Some(Status::Starting) => "Starting the debugger...".into(),
            Some(Status::Running) => "Running".into(),
            Some(Status::Paused(why)) => format!("Paused ({why})"),
            Some(Status::Exited(c)) => format!("The program exited with code {c}"),
        };
        let column = |id: &'static str, title: String| {
            div()
                .id(id)
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .border_r_1()
                .border_color(border)
                .child(
                    div()
                        .px_2()
                        .pt_1()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .child(title),
                )
        };

        // Call stack.
        let (frames, frame_ix) = self
            .dbg
            .as_ref()
            .map(|d| (d.frames.clone(), d.frame_ix))
            .unwrap_or_default();
        let mut stack = column("dbg-stack", "Call Stack".into());
        for (i, f) in frames.iter().enumerate() {
            let place = f
                .file
                .as_deref()
                .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string())
                .map(|n| format!("{n}:{}", f.line))
                .unwrap_or_default();
            stack = stack.child(
                div()
                    .id(("frame", i))
                    .flex()
                    .gap_2()
                    .px_2()
                    .text_sm()
                    .cursor_pointer()
                    .when(i == frame_ix, |d| d.bg(list_active))
                    .child(f.name.clone())
                    .child(div().text_xs().text_color(muted).child(place))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.select_frame(i, window, cx)),
                    ),
            );
        }

        // Variables of the selected frame.
        let mut vars = column("dbg-vars", "Variables".into());
        if let Some(d) = &self.dbg {
            for (scope, list) in &d.vars {
                vars = vars.child(
                    div()
                        .px_2()
                        .text_xs()
                        .text_color(muted)
                        .child(scope.clone()),
                );
                for v in list {
                    vars = vars.child(
                        div()
                            .flex()
                            .gap_2()
                            .px_3()
                            .text_sm()
                            .font_family("monospace")
                            .child(v.name.clone())
                            .child(div().text_color(muted).child("="))
                            .child(v.value.clone())
                            .when_some(v.type_name.clone(), |d, t| {
                                d.child(div().text_xs().text_color(muted).child(t))
                            }),
                    );
                }
            }
        }

        // Breakpoints, always listed.
        let total: usize = self.breakpoints.values().map(BTreeSet::len).sum();
        let mut bps = column("dbg-bps", format!("Breakpoints ({total})"));
        let mut n = 0usize;
        for (path, lines) in &self.breakpoints {
            for &line in lines {
                n += 1;
                let (p_open, p_del) = (path.clone(), path.clone());
                bps = bps.child(
                    div()
                        .id(("bp", n))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .text_sm()
                        .child(div().w(px(8.)).h(px(8.)).rounded_full().bg(danger))
                        .child(
                            div()
                                .id(("bp-open", n))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .cursor_pointer()
                                .child(format!("{path}:{line}"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_at(&p_open, line as usize, 0, window, cx)
                                })),
                        )
                        .child(
                            div()
                                .id(("bp-del", n))
                                .cursor_pointer()
                                .text_color(muted)
                                .child("x")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_breakpoint(&p_del, line, cx)
                                })),
                        ),
                );
            }
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_2()
                    .h(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(if self.debug_paused_now() {
                        warning
                    } else {
                        muted
                    })
                    .child(status_line),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(stack)
                    .child(vars)
                    .child(bps),
            )
            .into_any_element()
    }
}

fn stop_in_background(cx: &mut AsyncApp, session: Arc<Session>) {
    cx.background_executor()
        .spawn(async move { session.stop() })
        .detach();
}
