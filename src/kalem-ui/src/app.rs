use crate::detached::{dest_at, mouse_up_probe, tab_strip, Detached, StripCtx, TabHost};
use crate::diffview::{diff_view, DiffDoc, Live};
use crate::icons::icon;
use crate::theming::{self, rgba};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    input::{
        self, Editor, EditorState, Input, InputEvent, InputState, Position, Textarea, TextareaState,
    },
    list::ListItem,
    menu::DropdownMenu as _,
    resizable::{h_resizable, resizable_panel, v_resizable},
    tree::{tree, TreeItem, TreeState},
    ActiveTheme as _, Disableable as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kalem_core::explorer::Node;
use kalem_test::{Outcome, TestRun};
use kalem_theme::{Mode, Palette, Settings, Theme as KalemTheme};
use kalem_vcs::diff::DiffRow;
use kalem_vcs::git::{Branch, GitCli, Stash};
use kalem_vcs::overview::ChangesOverview;
use kalem_vcs::{ChangeKind, Status, VcsError};
use kalem_workspace::kinds::{badge_for, is_image, project_index, project_kind, ProjectKind};
use kalem_workspace::search::{
    matches_file_types, replace_in_files, search, SearchOptions, SearchResult,
};
use kalem_workspace::tasks::{spawn, Config, RunningTask, TaskEvent, TaskKind, TaskPlan};
use kalem_workspace::{language_for, quick_open, Workspace};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

mod debugger;
mod lsp;

actions!(
    kalem,
    [
        FindNext,
        FindPrev,
        FindNextFile,
        FindReferences,
        RenameSymbol,
        FormatDocument,
        CloseRename,
        StartDebugging,
        StopDebugging,
        DebugPause,
        StepOver,
        StepInto,
        StepOut,
        ToggleBreakpoint,
        Save,
        OpenFolder,
        CloseTab,
        Quit,
        ToggleSync,
        Refresh,
        ShowExplorer,
        ShowGit,
        GitFetch,
        GitPull,
        GitPush,
        GitSync,
        GitLog,
        GitStashAll,
        ShowOutput,
        ShowTests,
        ShowFind,
        FindClose,
        Build,
        RunNoDebug,
        RunTests,
        CancelTask,
        NavBack,
        NavForward,
        QuickOpen,
        CloseQuick,
        ModeLight,
        ModeDark,
        ModeSystem,
        ConfigDebug,
        ConfigRelease,
        OpenThemes,
        ReloadThemes,
        About
    ]
);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = kalem, no_json)]
pub struct SetTheme(pub usize);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = kalem, no_json)]
pub struct SetStartup(pub usize);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = kalem, no_json)]
pub struct CheckoutBranch(pub String);

/// Pin a right-dock panel (index into [`Right::ALL`]) back into the dock.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = kalem, no_json)]
pub struct DockRight(pub usize);

/// Collapse a right-dock panel (index into [`Right::ALL`]) to the edge strip.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = kalem, no_json)]
pub struct AutoHideRight(pub usize);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = kalem, no_json)]
pub struct SetFindScope(pub usize);

/// Where the Find and Replace window currently lives.
#[derive(Clone, Copy, PartialEq)]
enum FindPlace {
    Hidden,
    /// A free-floating window on top of the IDE.
    Float,
    /// A tab in the bottom dock (form on the left, results on the right).
    Bottom,
    /// A tab in the right dock.
    Right,
}

#[derive(Clone, Copy, PartialEq)]
enum FindMode {
    Find,
    Replace,
}

const FIND_SCOPES: [&str; 4] = [
    "Entire solution",
    "Startup project",
    "Open documents",
    "Current document",
];

/// Docking guides shown while the Find window is dragged (like Visual Studio's pin points).
#[derive(Clone, Copy, PartialEq)]
enum Guide {
    Float,
    Bottom,
    Right,
    /// Shown greyed out: this dock does not exist yet.
    Top,
    Left,
}

impl Guide {
    fn available(self) -> bool {
        !matches!(self, Guide::Top | Guide::Left)
    }
}

const GUIDE: f32 = 34.;

/// Guide squares for a window of the given size: the centre cross plus the window edges.
fn guide_rects(size: Size<Pixels>) -> Vec<(Guide, Bounds<Pixels>)> {
    let (w, h) = (f32::from(size.width), f32::from(size.height));
    let (cx, cy) = (w / 2. - GUIDE / 2., h / 2. - GUIDE / 2.);
    let at =
        |x: f32, y: f32| Bounds::new(point(px(x), px(y)), gpui_kit::size(px(GUIDE), px(GUIDE)));
    let d = GUIDE + 6.;
    vec![
        (Guide::Float, at(cx, cy)),
        (Guide::Top, at(cx, cy - d)),
        (Guide::Left, at(cx - d, cy)),
        (Guide::Right, at(cx + d, cy)),
        (Guide::Bottom, at(cx, cy + d)),
        // Window edges.
        (Guide::Top, at(cx, 72.)),
        (Guide::Left, at(8., cy)),
        (Guide::Right, at(w - GUIDE - 8., cy)),
        (Guide::Bottom, at(cx, h - GUIDE - 30.)),
    ]
}

fn guide_at(size: Size<Pixels>, pos: Point<Pixels>) -> Option<Guide> {
    guide_rects(size)
        .into_iter()
        .find(|(_, b)| b.contains(&pos))
        .map(|(g, _)| g)
}

/// A git write operation, run off the UI thread.
enum GitOp {
    Stage(Vec<String>),
    Unstage(Vec<String>),
    StageAll,
    UnstageAll,
    Commit {
        all: bool,
        message: String,
        amend: bool,
    },
    Fetch,
    Pull,
    Push,
    /// Pull, then push (the "Sync" button).
    Sync,
    /// Print the commit graph in the Output panel.
    Log,
    Checkout(String),
    StashPush,
    StashPop(String),
    StashDrop(String),
}

fn run_git_op(root: &Path, op: &GitOp) -> Result<String, VcsError> {
    let git = GitCli;
    let none = |r: Result<(), VcsError>| r.map(|_| String::new());
    match op {
        GitOp::Stage(p) => none(git.stage(root, p)),
        GitOp::Unstage(p) => none(git.unstage(root, p)),
        GitOp::StageAll => none(git.stage_all(root)),
        GitOp::UnstageAll => none(git.unstage_all(root)),
        GitOp::Commit {
            all,
            message,
            amend,
        } => {
            if *all {
                git.stage_all(root)?;
            }
            git.commit(root, message, *amend)
        }
        GitOp::Fetch => git.fetch(root),
        GitOp::Pull => git.pull(root),
        GitOp::Push => git.push(root),
        GitOp::Sync => {
            let pulled = git.pull(root)?;
            let pushed = git.push(root)?;
            Ok(format!("{pulled}\n{pushed}").trim().to_string())
        }
        GitOp::Log => git.log(root),
        GitOp::Checkout(b) => git.checkout(root, b),
        GitOp::StashPush => git.stash_push(root, ""),
        GitOp::StashPop(n) => git.stash_pop(root, n),
        GitOp::StashDrop(n) => git.stash_drop(root, n),
    }
}

fn vcs_error_text(e: &VcsError) -> String {
    match e {
        VcsError::NotInstalled(s) | VcsError::Failed(s) | VcsError::Parse(s) => s.clone(),
    }
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-s", Save, None),
        KeyBinding::new("secondary-o", OpenFolder, None),
        KeyBinding::new("secondary-w", CloseTab, None),
        KeyBinding::new("secondary-shift-b", Build, None),
        KeyBinding::new("ctrl-f5", RunNoDebug, None),
        KeyBinding::new("secondary-p", QuickOpen, None),
        KeyBinding::new("secondary-shift-f", ShowFind, None),
        KeyBinding::new("f3", FindNext, None),
        KeyBinding::new("shift-f12", FindReferences, None),
        KeyBinding::new("f2", RenameSymbol, None),
        KeyBinding::new("shift-alt-f", FormatDocument, None),
        KeyBinding::new("escape", CloseRename, Some("Rename")),
        KeyBinding::new("shift-f3", FindPrev, None),
        KeyBinding::new("alt-left", NavBack, None),
        KeyBinding::new("alt-right", NavForward, None),
        KeyBinding::new("secondary-shift-r", Refresh, None),
        KeyBinding::new("f5", StartDebugging, None),
        KeyBinding::new("shift-f5", StopDebugging, None),
        KeyBinding::new("f9", ToggleBreakpoint, None),
        KeyBinding::new("f10", StepOver, None),
        KeyBinding::new("f11", StepInto, None),
        KeyBinding::new("shift-f11", StepOut, None),
        KeyBinding::new("escape", CloseQuick, Some("QuickOpen")),
        // Redo is Ctrl+Y in the editor already; Ctrl+Shift+Z (Cmd+Shift+Z) is the other habit.
        KeyBinding::new("secondary-shift-z", input::Redo, Some("Input")),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

pub(crate) struct OpenTab {
    pub path: String,
    pub editor: Entity<EditorState>,
    pub saved: SharedString,
    pub dirty: bool,
    pub _sub: Subscription,
    /// A diff view of `path` instead of the file's text (the editor is then unused).
    pub diff: Option<Rc<DiffDoc>>,
    /// Breakpoint / current-line highlights, created when first needed.
    pub marks: Option<debugger::Marks>,
}

impl OpenTab {
    /// The tab edits the file itself (a plain file tab, or an editable diff).
    fn edits_file(&self) -> bool {
        self.diff.as_ref().is_none_or(|d| d.live.is_some())
    }
}

/// Where a tab is moved to.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Dest {
    /// A fresh window.
    New,
    /// The main IDE window.
    Main,
    /// A torn-off window, by id.
    Window(u64),
}

/// Move the tab at this index of the current window.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = kalem, no_json)]
pub(crate) struct MoveTab(pub usize, pub Dest);

/// A torn-off editor window known to the main window.
struct WinRef {
    id: u64,
    view: WeakEntity<Detached>,
    handle: AnyWindowHandle,
}

#[derive(Clone, Copy, PartialEq)]
enum Right {
    Explorer,
    Git,
    Find,
}

impl Right {
    const ALL: [Right; 3] = [Right::Explorer, Right::Git, Right::Find];

    fn title(self) -> &'static str {
        match self {
            Right::Explorer => "Solution Explorer",
            Right::Git => "Git Changes",
            Right::Find => "Find and Replace",
        }
    }

    fn from_ix(i: usize) -> Right {
        Right::ALL[i.min(2)]
    }

    fn ix(self) -> usize {
        Right::ALL.iter().position(|r| *r == self).unwrap_or(0)
    }

    /// Size in px of the rotated label svg.
    fn label_size(self) -> (f32, f32) {
        match self {
            Right::Explorer => (17., 105.),
            Right::Git => (17., 78.),
            Right::Find => (17., 106.),
        }
    }

    /// The rotated label shown on the edge strip.
    fn label_icon(self) -> &'static str {
        match self {
            Right::Explorer => "label-explorer",
            Right::Git => "label-git",
            Right::Find => "label-find",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Bottom {
    Output,
    Tests,
    Find,
    Debug,
}

#[derive(Clone, Copy, PartialEq)]
enum Region {
    Editor,
    Right,
    Bottom,
}

struct TaskState {
    task: RunningTask,
    gen: u64,
}

struct Quick {
    input: Entity<InputState>,
    hits: Vec<String>,
    _sub: Subscription,
}

pub struct Kalem {
    ws: Option<Workspace>,
    tree: Entity<TreeState>,
    tabs: Vec<OpenTab>,
    active: Option<usize>,
    sync_active: bool,
    right: Right,
    /// Panels collapsed to the edge strip (the others are docked, as tabs of the right dock).
    right_auto: Vec<Right>,
    /// The auto-hidden panel that is currently slid out over the editor.
    right_fly: Option<Right>,
    bottom: Bottom,
    region: Region,
    git: Result<Status, String>,
    commit_msg: Entity<TextareaState>,
    amend: bool,
    branches: Vec<Branch>,
    stashes: Vec<Stash>,
    git_busy: Option<&'static str>,
    /// Last git result for the panel: (text, is_error).
    git_note: Option<(String, bool)>,
    _msg_sub: Subscription,
    tests: TestRun,
    message: Option<String>,
    focus: FocusHandle,
    // theme
    themes: Vec<KalemTheme>,
    settings: Settings,
    dark: bool,
    palette: Palette,
    // explorer decoration
    proj_kinds: Rc<HashMap<String, ProjectKind>>,
    changes: Rc<HashMap<String, ChangeKind>>,
    // build / run / test
    config: Config,
    startup: Option<String>,
    task: Option<TaskState>,
    task_gen: u64,
    output: Vec<String>,
    status_text: String,
    // navigation history
    nav: Vec<String>,
    nav_pos: usize,
    navigating: bool,
    quick: Option<Quick>,
    output_scroll: ScrollHandle,
    // Find in Files
    find_input: Entity<InputState>,
    replace_input: Entity<InputState>,
    types_input: Entity<InputState>,
    find_place: FindPlace,
    find_mode: FindMode,
    find_scope: usize,
    find_append: bool,
    find_error: Option<String>,
    find_note: Option<String>,
    /// Position of the floating window and, while it is being dragged, the grab offset.
    find_pos: Point<Pixels>,
    find_drag: Option<Point<Pixels>>,
    find_hover: Option<Guide>,
    /// Set after a Replace All click found matches: the next click does the replacing.
    replace_confirm: bool,
    pending_replace: bool,
    find_opts: SearchOptions,
    find: Option<SearchResult>,
    find_busy: bool,
    find_query: String,
    find_gen: u64,
    /// The hit F3 / Shift+F3 last jumped to.
    find_cur: Option<usize>,
    /// After a search that F3 started: jump this many hits (1 = first, -1 = last).
    find_goto: Option<isize>,
    find_scroll: ScrollHandle,
    _find_subs: Vec<Subscription>,
    /// Project folders (sorted) for colouring tabs by project.
    proj_dirs: Vec<String>,
    // torn-off windows
    handle: AnyWindowHandle,
    windows: Vec<WinRef>,
    next_win: u64,
    tab_drag: Option<usize>,
    // debugging
    dbg: Option<debugger::DebugState>,
    dbg_gen: u64,
    /// Set between "build for debugging" and the build finishing.
    pending_debug: Option<kalem_workspace::debug::DebugPlan>,
    /// Breakpoints by workspace-relative path (1-based lines).
    breakpoints: std::collections::BTreeMap<String, std::collections::BTreeSet<u32>>,
    /// Conditions of breakpoints: (workspace-relative path, line) -> expression.
    bp_conditions: std::collections::BTreeMap<(String, u32), String>,
    /// The breakpoint whose condition is being edited.
    cond_edit: Option<(String, u32)>,
    cond_input: Entity<InputState>,
    lsp: lsp::LspState,
    rename: Option<lsp::RenameState>,
    rename_input: Entity<InputState>,
    /// Watch expressions, evaluated whenever the program stops.
    watches: Vec<String>,
    watch_input: Entity<InputState>,
    _dbg_subs: Vec<Subscription>,
    /// Where execution is stopped: (workspace-relative path, 1-based line).
    pc: Option<(String, u32)>,
}

const MAX_OUTPUT: usize = 5000;
const MAX_FIND_HITS: usize = 1000;

fn tree_items(ws: &Workspace) -> Vec<TreeItem> {
    fn conv(n: &Node) -> TreeItem {
        let mut it = TreeItem::new(n.path.clone(), n.name.clone());
        if n.is_dir {
            it = it.children(n.children.iter().map(conv));
        }
        it
    }
    ws.explorer.root.children.iter().map(conv).collect()
}

impl Kalem {
    pub fn new(start: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cfg = kalem_theme::config_dir();
        let (themes, errors) =
            kalem_theme::load_all(cfg.as_ref().map(|d| d.join("themes")).as_deref());
        let settings = Settings::load(cfg.as_deref());
        let tree = cx.new(|cx| TreeState::new(cx));
        let commit_msg =
            cx.new(|cx| TextareaState::new(window, cx).placeholder("Enter a message <Required>"));
        // The Commit button's enabled state depends on the message, so redraw on every edit.
        let msg_sub = cx.subscribe_in(&commit_msg, window, |_, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        });
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
        let replace_input = cx.new(|cx| InputState::new(window, cx).placeholder("Replace"));
        let types_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("*.rs;*.toml (all files)"));
        let find_subs: Vec<Subscription> = [&find_input, &replace_input, &types_input]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(
                    input,
                    window,
                    |this, _, ev: &InputEvent, window, cx| match ev {
                        InputEvent::PressEnter { .. } => this.run_find(window, cx),
                        // Editing the query invalidates a pending "confirm replace".
                        InputEvent::Change => {
                            this.replace_confirm = false;
                            cx.notify();
                        }
                        _ => {}
                    },
                )
            })
            .collect();
        let watch_input = cx.new(|cx| InputState::new(window, cx).placeholder("Add watch (Enter)"));
        let cond_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Condition, e.g. i == 3 (Enter; empty clears)")
        });
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("New name"));
        let dbg_subs = vec![
            cx.subscribe_in(
                &rename_input,
                window,
                |this, _, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::PressEnter { .. }) {
                        this.commit_rename(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &watch_input,
                window,
                |this, _, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::PressEnter { .. }) {
                        this.add_watch(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &cond_input,
                window,
                |this, _, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::PressEnter { .. }) {
                        this.commit_condition(window, cx);
                    }
                },
            ),
        ];
        let mut me = Self {
            ws: None,
            tree,
            tabs: Vec::new(),
            active: None,
            sync_active: true,
            right: Right::Explorer,
            right_auto: Vec::new(),
            right_fly: None,
            bottom: Bottom::Output,
            region: Region::Editor,
            git: Err("No folder open".into()),
            commit_msg,
            amend: false,
            branches: Vec::new(),
            stashes: Vec::new(),
            git_busy: None,
            git_note: None,
            _msg_sub: msg_sub,
            tests: TestRun { cases: Vec::new() },
            message: errors.first().cloned(),
            focus: cx.focus_handle(),
            themes,
            settings,
            dark: true,
            palette: Palette::default(),
            proj_kinds: Rc::new(HashMap::new()),
            changes: Rc::new(HashMap::new()),
            config: Config::Debug,
            startup: None,
            task: None,
            task_gen: 0,
            output: Vec::new(),
            status_text: "Ready".into(),
            nav: Vec::new(),
            nav_pos: 0,
            navigating: false,
            quick: None,
            output_scroll: ScrollHandle::new(),
            find_input,
            replace_input,
            types_input,
            find_place: FindPlace::Hidden,
            find_mode: FindMode::Find,
            find_scope: 0,
            find_append: false,
            find_error: None,
            find_note: None,
            find_pos: point(px(320.), px(110.)),
            find_drag: None,
            find_hover: None,
            replace_confirm: false,
            pending_replace: false,
            proj_dirs: Vec::new(),
            handle: window.window_handle(),
            windows: Vec::new(),
            next_win: 2,
            tab_drag: None,
            dbg: None,
            dbg_gen: 0,
            pending_debug: None,
            breakpoints: Default::default(),
            bp_conditions: Default::default(),
            cond_edit: None,
            cond_input,
            lsp: Default::default(),
            rename: None,
            rename_input,
            watches: Vec::new(),
            watch_input,
            _dbg_subs: dbg_subs,
            pc: None,
            find_opts: SearchOptions::default(),
            find: None,
            find_busy: false,
            find_query: String::new(),
            find_gen: 0,
            find_cur: None,
            find_goto: None,
            find_scroll: ScrollHandle::new(),
            _find_subs: find_subs,
        };
        me.apply_theme(cx);
        if let Some(p) = start {
            me.open_path(&p, window, cx);
        }
        window.focus(&me.focus, cx);
        me
    }

    // ---------------------------------------------------------------- theme

    fn apply_theme(&mut self, cx: &mut Context<Self>) {
        let theme = self
            .themes
            .iter()
            .find(|t| t.name == self.settings.theme)
            .unwrap_or(&self.themes[0])
            .clone();
        self.dark = theming::is_dark(self.settings.mode, cx);
        self.palette = theming::apply(&theme, self.dark, cx);
        cx.notify();
    }

    fn save_settings(&mut self) {
        if let Err(e) = self.settings.save(kalem_theme::config_dir().as_deref()) {
            self.message = Some(format!("Could not save settings: {e}"));
        }
    }

    fn set_theme(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(t) = self.themes.get(ix) {
            self.settings.theme = t.name.clone();
            self.save_settings();
            self.apply_theme(cx);
        }
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.settings.mode = mode;
        self.save_settings();
        self.apply_theme(cx);
    }

    fn open_themes_folder(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = kalem_theme::config_dir().map(|d| d.join("themes")) else {
            self.message = Some("No config folder on this system".into());
            return;
        };
        if std::fs::create_dir_all(&dir).is_ok() {
            let template = dir.join("_template.toml.example");
            if !template.exists() {
                let text = kalem_theme::BUILTIN_TOML.replacen(
                    "name = \"Kalem\"",
                    "name = \"My Theme\"",
                    1,
                );
                let _ = std::fs::write(template, text);
            }
            cx.reveal_path(&dir);
        }
    }

    fn reload_themes(&mut self, cx: &mut Context<Self>) {
        let cfg = kalem_theme::config_dir();
        let (themes, errors) =
            kalem_theme::load_all(cfg.as_ref().map(|d| d.join("themes")).as_deref());
        self.themes = themes;
        self.message = errors.first().cloned();
        self.apply_theme(cx);
    }

    // ------------------------------------------------------------ workspace

    fn open_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        match Workspace::open(path) {
            Ok(ws) => {
                let items = tree_items(&ws);
                self.tree.update(cx, |t, cx| t.set_items(items, cx));
                let kinds: HashMap<String, ProjectKind> = ws
                    .project_dirs()
                    .into_iter()
                    .map(|d| {
                        let k = project_kind(&d, &ws.files);
                        (d, k)
                    })
                    .collect();
                self.proj_dirs = ws.project_dirs().into_iter().collect();
                self.proj_kinds = Rc::new(kinds);
                self.startup = ws.default_startup();
                self.ws = Some(ws);
                self.tabs.clear();
                self.active = None;
                self.nav.clear();
                self.nav_pos = 0;
                self.message = None;
                self.refresh(cx);
            }
            Err(e) => self.message = Some(e.to_string()),
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(ws) = &self.ws {
            self.git = ws.git_status().map_err(|e| format!("{e:?}"));
            self.branches = GitCli.branches(&ws.root).unwrap_or_default();
            self.stashes = GitCli.stashes(&ws.root).unwrap_or_default();
            self.tests = ws.test_results();
            let mut map = HashMap::new();
            if let Ok(st) = &self.git {
                for e in &st.entries {
                    if let Some(k) = e.unstaged.or(e.staged) {
                        map.insert(e.path.clone(), k);
                    }
                }
            }
            self.changes = Rc::new(map);
        }
        self.reload_diffs();
        cx.notify();
    }

    /// Run a git write operation on a background thread, then refresh the panel.
    fn git_op(
        &mut self,
        label: &'static str,
        op: GitOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_busy.is_some() {
            return;
        }
        let Some(ws) = &self.ws else { return };
        let root = ws.root.clone();
        self.git_busy = Some(label);
        self.git_note = None;
        let is_commit = matches!(op, GitOp::Commit { .. });
        let quiet = matches!(
            op,
            GitOp::Stage(_) | GitOp::Unstage(_) | GitOp::StageAll | GitOp::UnstageAll
        );
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { run_git_op(&root, &op) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.git_busy = None;
                let (text, is_err) = match &res {
                    Ok(t) => (t.clone(), false),
                    Err(e) => (vcs_error_text(e), true),
                };
                if !quiet {
                    this.output.push(format!("> git {label}"));
                    this.output.extend(text.lines().map(str::to_string));
                    if is_err {
                        this.output.push(format!("git {label} failed"));
                    }
                }
                if is_commit && !is_err {
                    this.amend = false;
                    this.commit_msg
                        .update(cx, |m, cx| m.set_value("", window, cx));
                }
                // Errors: show the line that says what went wrong; the full text is in Output.
                let first = if is_err {
                    text.lines()
                        .find(|l| {
                            let l = l.trim_start();
                            l.starts_with("error:")
                                || l.starts_with("fatal:")
                                || l.starts_with("! ")
                        })
                        .map(|l| l.trim_start().trim_start_matches("error: "))
                } else {
                    None
                }
                .or_else(|| text.lines().find(|l| !l.trim().is_empty()))
                .unwrap_or("");
                this.git_note = if is_err {
                    Some((format!("{label} failed: {first}"), true))
                } else if label == "log" {
                    Some(("Commit graph is in the Output panel".to_string(), false))
                } else if quiet {
                    None
                } else {
                    Some((
                        if first.is_empty() {
                            format!("{label}: done")
                        } else {
                            format!("{label}: {first}")
                        },
                        false,
                    ))
                };
                this.refresh(cx);
            });
        })
        .detach();
    }

    fn set_amend(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.amend = on;
        // Like Visual Studio: amending prefills the previous message.
        if on && self.commit_msg.read(cx).value().trim().is_empty() {
            if let Some(ws) = &self.ws {
                if let Ok(last) = GitCli.last_commit_message(&ws.root) {
                    self.commit_msg
                        .update(cx, |m, cx| m.set_value(last, window, cx));
                }
            }
        }
        cx.notify();
    }

    fn commit_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(st) = &self.git else { return };
        let ov = ChangesOverview::from_status(st);
        let message = self.commit_msg.read(cx).value().to_string();
        if !ov.commit_button(&message, self.amend).enabled {
            return;
        }
        let op = GitOp::Commit {
            all: ov.staged == 0,
            message,
            amend: self.amend,
        };
        self.git_op("commit", op, window, cx);
    }

    fn pick_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("Open folder or solution".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(p) = paths.first() {
                    let p = p.clone();
                    let _ = this.update_in(cx, |this, window, cx| this.open_path(&p, window, cx));
                }
            }
        })
        .detach();
    }

    // ----------------------------------------------------------------- tabs

    fn open_file(&mut self, rel: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| t.diff.is_none() && t.path == rel)
        {
            self.activate(ix, cx);
            return;
        }
        // Already open in a torn-off window: bring that one forward instead of opening a copy.
        let wins: Vec<(WeakEntity<Detached>, AnyWindowHandle)> = self
            .windows
            .iter()
            .map(|w| (w.view.clone(), w.handle))
            .collect();
        for (view, handle) in wins {
            let Some(view) = view.upgrade() else { continue };
            if view.update(cx, |d, cx| d.show_path(rel, cx)) {
                let _ = handle.update(cx, |_, w, _| w.activate_window());
                return;
            }
        }
        // The file is open in an editable diff: that editor is the file, so both show one text.
        let shared = self
            .tabs
            .iter()
            .find(|t| t.diff.as_ref().is_some_and(|d| d.live.is_some()) && t.path == rel)
            .map(|t| (t.editor.clone(), t.saved.clone(), t.dirty));
        let (editor, saved, dirty) = match shared {
            Some(s) => s,
            None => {
                let Some(ws) = &self.ws else { return };
                let text = match ws.read_file(rel) {
                    Ok(t) => t,
                    Err(e) => {
                        self.message = Some(e);
                        cx.notify();
                        return;
                    }
                };
                let editor = Self::file_editor(rel, &text, window, cx);
                (editor, text.into(), false)
            }
        };
        let sub = self.track_dirty(&editor, cx);
        self.lsp_attach(rel, &editor, cx);
        self.tabs.push(OpenTab {
            path: rel.to_string(),
            editor,
            saved,
            dirty,
            _sub: sub,
            diff: None,
            marks: None,
        });
        self.message = None;
        self.activate(self.tabs.len() - 1, cx);
    }

    /// An editor holding the text of a file, with its language and line numbers.
    fn file_editor(
        rel: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<EditorState> {
        let lang = language_for(rel);
        let init = text.to_string();
        cx.new(|cx| {
            let mut s = EditorState::new(window, cx);
            if let Some(l) = lang {
                s = s.language(l);
            }
            s.line_number(true).default_value(init)
        })
    }

    /// The text the left side of an editable diff starts from: the index version, nothing for a
    /// new file. `None` when git cannot say.
    fn diff_base(&self, path: &str, untracked: bool) -> Option<String> {
        if untracked {
            return Some(String::new());
        }
        let ws = self.ws.as_ref()?;
        GitCli
            .index_text(&ws.root, path)
            .ok()
            .map(Option::unwrap_or_default)
    }

    /// Compute the diff of one file for a diff tab.
    fn make_diff(&self, path: &str, staged: bool, untracked: bool) -> DiffDoc {
        let res = match &self.ws {
            Some(ws) => GitCli.file_diff(&ws.root, path, staged, untracked),
            None => Err(VcsError::Failed("No folder open".into())),
        };
        let (diff, error) = match res {
            Ok(d) => (d, None),
            Err(e) => (Default::default(), Some(vcs_error_text(&e))),
        };
        let rows: Vec<DiffRow> = diff.rows();
        DiffDoc::new(path.to_string(), staged, untracked, diff, rows, error)
    }

    /// Show what changed in a file (Git Changes panel click), side by side, in an editor tab.
    fn open_diff(
        &mut self,
        path: &str,
        staged: bool,
        untracked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = path.trim_end_matches('/');
        let doc = self.make_diff(path, staged, untracked);
        let existing = self.tabs.iter().position(|t| {
            t.diff
                .as_ref()
                .is_some_and(|d| d.path == path && d.staged == staged)
        });
        // The working-tree side of a text file is edited right in the diff.
        let editable = !staged && doc.error.is_none() && !doc.diff.binary && !doc.rows.is_empty();
        let mut file: Option<(Entity<EditorState>, SharedString, bool)> = None;
        let mut live = None;
        if editable {
            if let Some(ix) = existing {
                if let Some(l) = self.tabs[ix].diff.as_ref().and_then(|d| d.live.clone()) {
                    file = Some((
                        self.tabs[ix].editor.clone(),
                        self.tabs[ix].saved.clone(),
                        self.tabs[ix].dirty,
                    ));
                    live = Some(l);
                }
            }
            if live.is_none() {
                let open = self
                    .tabs
                    .iter()
                    .find(|t| t.diff.is_none() && t.path == path)
                    .map(|t| (t.editor.clone(), t.saved.clone(), t.dirty));
                let text = match &open {
                    Some((_, saved, _)) => Some(saved.to_string()),
                    None => self.ws.as_ref().and_then(|w| w.read_file(path).ok()),
                };
                let base = self.diff_base(path, untracked);
                if let (Some(text), Some(base)) = (text, base) {
                    if text.len() < 1_000_000 && base.len() < 1_000_000 {
                        let right = match &open {
                            Some((e, _, _)) => e.clone(),
                            None => Self::file_editor(path, &text, window, cx),
                        };
                        let left = Self::file_editor(path, &base, window, cx);
                        let l = Rc::new(Live::new(left, right.clone(), base, cx));
                        l.refresh(self.dark, cx);
                        file = Some(match open {
                            Some(o) => o,
                            None => (right, text.into(), false),
                        });
                        live = Some(l);
                    }
                }
            }
        }
        let doc = Rc::new(doc.with_live(live));
        if let Some(ix) = existing {
            self.tabs[ix].diff = Some(doc);
            if let Some((e, saved, dirty)) = file {
                self.tabs[ix].editor = e;
                self.tabs[ix].saved = saved;
                self.tabs[ix].dirty = dirty;
            }
            self.activate(ix, cx);
            return;
        }
        let (editor, saved, dirty) = match file {
            Some(f) => f,
            None => (
                cx.new(|cx| EditorState::new(window, cx)),
                SharedString::default(),
                false,
            ),
        };
        let sub = self.track_dirty(&editor, cx);
        self.tabs.push(OpenTab {
            path: path.to_string(),
            editor,
            saved,
            dirty,
            _sub: sub,
            diff: Some(doc),
            marks: None,
        });
        self.activate(self.tabs.len() - 1, cx);
    }

    /// After a stage/unstage/commit the open diff tabs show the new state.
    fn reload_diffs(&mut self) {
        for i in 0..self.tabs.len() {
            let Some(d) = self.tabs[i].diff.clone() else {
                continue;
            };
            let fresh = self.make_diff(&d.path, d.staged, d.untracked);
            fresh.unified.set(d.unified.get());
            // An editable diff keeps its editors; only the old side may have moved (staging).
            if let Some(live) = &d.live {
                if let Some(base) = self.diff_base(&d.path, d.untracked) {
                    if *live.base.borrow() != base {
                        *live.base.borrow_mut() = base;
                        live.base_stale.set(true);
                    }
                }
            }
            let fresh = fresh.with_live(d.live.clone());
            self.tabs[i].diff = Some(Rc::new(fresh));
        }
    }

    /// Open a file and put the cursor on a 1-based line / 0-based column.
    fn open_at(
        &mut self,
        rel: &str,
        line: usize,
        col: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file(rel, window, cx);
        let editor = self
            .tabs
            .iter()
            .find(|t| t.diff.is_none() && t.path == rel)
            .map(|t| t.editor.clone());
        if let Some(editor) = editor {
            let pos = Position::new(line.saturating_sub(1) as u32, col as u32);
            editor.update(cx, |e, cx| e.set_cursor_position(pos, window, cx));
        }
    }

    /// The panels the right dock can show right now.
    fn right_avail(&self) -> Vec<Right> {
        Right::ALL
            .into_iter()
            .filter(|r| *r != Right::Find || self.find_place == FindPlace::Right)
            .collect()
    }

    /// Panels that are docked (tabs of the dock), as opposed to collapsed to the edge strip.
    fn right_docked(&self) -> Vec<Right> {
        self.right_avail()
            .into_iter()
            .filter(|r| !self.right_auto.contains(r))
            .collect()
    }

    /// Make sure `self.right` names a docked panel (when there is one).
    fn fix_right(&mut self) {
        let docked = self.right_docked();
        if !docked.contains(&self.right) {
            if let Some(first) = docked.first() {
                self.right = *first;
            }
        }
    }

    /// Show a panel: select its tab when docked, slide it out when collapsed to the strip.
    fn reveal_right(&mut self, which: Right) {
        if self.right_auto.contains(&which) {
            self.right_fly = Some(which);
        } else {
            self.right = which;
        }
    }

    /// Pin a panel back into the dock.
    fn dock_right(&mut self, which: Right, cx: &mut Context<Self>) {
        self.right_auto.retain(|r| *r != which);
        if self.right_fly == Some(which) {
            self.right_fly = None;
        }
        self.right = which;
        cx.notify();
    }

    /// Collapse a panel to the edge strip; it stays there as a vertical tab.
    fn autohide_right(&mut self, which: Right, cx: &mut Context<Self>) {
        if !self.right_auto.contains(&which) {
            self.right_auto.push(which);
        }
        // Un-pinning keeps it slid out until the user clicks away.
        self.right_fly = Some(which);
        self.fix_right();
        cx.notify();
    }

    fn show_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_place == FindPlace::Hidden {
            self.find_place = FindPlace::Float;
        }
        match self.find_place {
            FindPlace::Bottom => self.bottom = Bottom::Find,
            FindPlace::Right => self.reveal_right(Right::Find),
            _ => {}
        }
        self.find_input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn set_find_place(&mut self, place: FindPlace, window: &mut Window, cx: &mut Context<Self>) {
        self.find_place = place;
        match place {
            FindPlace::Bottom => self.bottom = Bottom::Find,
            FindPlace::Right => self.reveal_right(Right::Find),
            _ => {
                // Leaving a dock must not leave an empty tab selected.
                self.right_auto.retain(|r| *r != Right::Find);
                if self.right_fly == Some(Right::Find) {
                    self.right_fly = None;
                }
                if self.right == Right::Find {
                    self.right = Right::Explorer;
                }
            }
        }
        if place != FindPlace::Hidden {
            self.find_input.update(cx, |s, cx| s.focus(window, cx));
        }
        cx.notify();
    }

    fn end_find_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_drag.take().is_none() {
            return;
        }
        let hover = self.find_hover.take();
        match hover {
            Some(Guide::Bottom) => self.set_find_place(FindPlace::Bottom, window, cx),
            Some(Guide::Right) => self.set_find_place(FindPlace::Right, window, cx),
            _ => self.set_find_place(FindPlace::Float, window, cx),
        }
        cx.notify();
    }

    /// The files a search covers: the "Look in" scope narrowed by the file-type filter.
    fn find_files(&self, cx: &App) -> Vec<String> {
        let Some(ws) = &self.ws else {
            return Vec::new();
        };
        let base: Vec<String> = match self.find_scope {
            1 => match &self.startup {
                Some(d) => {
                    let prefix = format!("{d}/");
                    ws.files
                        .iter()
                        .filter(|f| f.starts_with(&prefix))
                        .cloned()
                        .collect()
                }
                None => ws.files.clone(),
            },
            2 => self
                .tabs
                .iter()
                .filter(|t| t.diff.is_none())
                .map(|t| t.path.clone())
                .collect(),
            3 => self
                .active
                .and_then(|i| self.tabs.get(i))
                .map(|t| vec![t.path.clone()])
                .unwrap_or_default(),
            _ => ws.files.clone(),
        };
        let types = self.types_input.read(cx).value().to_string();
        base.into_iter()
            .filter(|f| matches_file_types(f, &types))
            .collect()
    }

    /// An option or scope changed: forget a pending confirmation and refresh the results.
    fn find_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_confirm = false;
        if !self.find_input.read(cx).value().is_empty() {
            self.run_find(window, cx);
        } else {
            cx.notify();
        }
    }

    fn run_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.find_input.read(cx).value().to_string();
        let Some(ws) = &self.ws else { return };
        self.find_error = None;
        self.find_note = None;
        if query.is_empty() {
            self.find = None;
            self.pending_replace = false;
            cx.notify();
            return;
        }
        let (root, opts) = (ws.root.clone(), self.find_opts);
        let files = self.find_files(cx);
        let append = self.find_append && self.find.is_some();
        self.find_gen += 1;
        let gen = self.find_gen;
        self.find_busy = true;
        self.find_query = query.clone();
        self.bottom = Bottom::Find;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { search(&root, &files, &query, opts, MAX_FIND_HITS) })
                .await;
            let _ = this.update(cx, |this, cx| {
                // A newer search superseded this one.
                if this.find_gen != gen {
                    return;
                }
                this.find_busy = false;
                match res {
                    Ok(mut r) => {
                        if append {
                            if let Some(prev) = this.find.take() {
                                let mut hits = prev.hits;
                                hits.extend(r.hits);
                                r.hits = hits;
                                r.files_searched += prev.files_searched;
                                r.truncated |= prev.truncated;
                            }
                        }
                        // Replace All: the first click only searches; this arms the second one.
                        this.replace_confirm = this.pending_replace && !r.hits.is_empty();
                        if this.pending_replace && r.hits.is_empty() {
                            this.find_note = Some("Nothing to replace".into());
                        }
                        this.find_cur = None;
                        this.find = Some(r);
                    }
                    Err(e) => {
                        this.find_error = Some(e);
                        this.find = None;
                    }
                }
                this.pending_replace = false;
                cx.notify();
            });
            // F3 on a fresh query: search first, then jump.
            let _ = cx.update(|window, cx| {
                this.update(cx, |this, cx| {
                    if let Some(d) = this.find_goto.take() {
                        this.find_step(d, false, window, cx);
                    }
                })
            });
        })
        .detach();
    }

    /// F3 / Shift+F3 (`delta` +1 / -1): open the next / previous hit of the last search,
    /// wrapping around. `by_file` skips the remaining hits of the current file.
    fn find_step(
        &mut self,
        delta: isize,
        by_file: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let n = self.find.as_ref().map(|r| r.hits.len()).unwrap_or(0);
        if n == 0 {
            // No results yet: run the search in the Find box, then come back here.
            if self.find.is_none() && !self.find_input.read(cx).value().is_empty() {
                self.find_goto = Some(delta);
                self.run_find(window, cx);
            }
            return;
        }
        let hits = &self.find.as_ref().unwrap().hits;
        let start = match self.find_cur {
            Some(c) => c as isize,
            None => {
                if delta > 0 {
                    -1
                } else {
                    n as isize
                }
            }
        };
        let cur_path = self.find_cur.map(|c| hits[c].path.clone());
        let mut ix = start;
        let mut next = None;
        for _ in 0..n {
            ix = (ix + delta).rem_euclid(n as isize);
            if by_file && cur_path.as_deref() == Some(hits[ix as usize].path.as_str()) {
                continue;
            }
            next = Some(ix as usize);
            break;
        }
        let Some(i) = next else { return };
        let h = hits[i].clone();
        self.find_cur = Some(i);
        self.find_scroll.scroll_to_item(i);
        self.open_at(&h.path, h.line, h.col, window, cx);
        cx.notify();
    }

    fn replace_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_busy || self.ws.is_none() {
            return;
        }
        if self.replace_confirm {
            self.execute_replace(window, cx);
        } else {
            self.pending_replace = true;
            self.run_find(window, cx);
        }
    }

    fn execute_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws) = &self.ws else { return };
        let root = ws.root.clone();
        let query = self.find_input.read(cx).value().to_string();
        let with = self.replace_input.read(cx).value().to_string();
        let opts = self.find_opts;
        let files = self.find_files(cx);
        // Files with unsaved edits are never touched: the disk copy is not what the user sees.
        let skip: Vec<String> = self
            .tabs
            .iter()
            .filter(|t| t.dirty)
            .map(|t| t.path.clone())
            .collect();
        self.replace_confirm = false;
        self.find_busy = true;
        cx.notify();
        let skipped = skip.clone();
        cx.spawn_in(window, async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { replace_in_files(&root, &files, &query, &with, opts, &skip) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.find_busy = false;
                let mut note_out: Option<String> = None;
                match res {
                    Err(e) => this.find_error = Some(e),
                    Ok(r) => {
                        // Open, unmodified tabs show the new text.
                        for rel in &r.changed {
                            let text = this.ws.as_ref().and_then(|w| w.read_file(rel).ok());
                            if let (Some(text), Some(tab)) = (
                                text,
                                this.tabs
                                    .iter_mut()
                                    .find(|t| t.edits_file() && &t.path == rel),
                            ) {
                                tab.saved = text.clone().into();
                                tab.dirty = false;
                                tab.editor.update(cx, |e, cx| e.set_value(text, window, cx));
                            }
                        }
                        let mut note = format!(
                            "Replaced {} occurrence(s) in {} file(s)",
                            r.replacements, r.files_changed
                        );
                        let left: Vec<_> = skipped
                            .iter()
                            .filter(|p| this.tabs.iter().any(|t| &t.path == *p))
                            .collect();
                        if !left.is_empty() {
                            note.push_str(&format!(
                                "; skipped {} file(s) with unsaved changes",
                                left.len()
                            ));
                        }
                        if let Some((f, e)) = r.failed.first() {
                            note.push_str(&format!("; could not write {f}: {e}"));
                        }
                        this.output.push(format!("> replace: {note}"));
                        note_out = Some(note);
                        this.refresh(cx);
                    }
                }
                this.run_find(window, cx);
                // run_find clears the note; keep the replace summary visible.
                if note_out.is_some() {
                    this.find_note = note_out;
                }
            });
        })
        .detach();
    }

    fn activate(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.active = Some(ix);
        self.region = Region::Editor;
        if !self.navigating {
            if let Some(t) = self.tabs.get(ix) {
                if t.diff.is_none() && self.nav.get(self.nav_pos) != Some(&t.path) {
                    self.nav
                        .truncate(self.nav_pos + usize::from(!self.nav.is_empty()));
                    self.nav.push(t.path.clone());
                    self.nav_pos = self.nav.len() - 1;
                }
            }
        }
        if self.sync_active {
            self.sync_tree(cx);
        }
        cx.notify();
    }

    fn navigate(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.nav_pos as isize + delta;
        if target < 0 || target as usize >= self.nav.len() {
            return;
        }
        self.nav_pos = target as usize;
        let path = self.nav[self.nav_pos].clone();
        self.navigating = true;
        self.open_file(&path, window, cx);
        self.navigating = false;
    }

    /// "Sync with active document": expand the folders above the active file and select it.
    fn sync_tree(&mut self, cx: &mut Context<Self>) {
        let (Some(ws), Some(ix)) = (&self.ws, self.active) else {
            return;
        };
        let Some(tab) = self.tabs.get(ix) else { return };
        if let Some(plan) = ws.reveal(&tab.path) {
            let id: SharedString = plan.select.into();
            self.tree.update(cx, |t, cx| {
                t.reveal_item(&id, ScrollStrategy::Center, cx);
                if let Some(i) = t.index_of(&id) {
                    t.set_selected_index(Some(i), cx);
                }
            });
        }
    }

    fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            let tab = self.remove_tab(ix, cx);
            if tab.diff.is_none() {
                self.lsp_forget(&tab.path, cx);
            }
        }
    }

    fn track_dirty(&self, editor: &Entity<EditorState>, cx: &mut Context<Self>) -> Subscription {
        cx.subscribe(editor, |this, editor, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                let now = editor.read(cx).value();
                let dark = this.dark;
                for t in this.tabs.iter_mut().filter(|t| t.editor == editor) {
                    t.dirty = now != t.saved;
                    // An editable diff compares again with every edit.
                    if let Some(live) = t.diff.as_ref().and_then(|d| d.live.as_ref()) {
                        live.refresh(dark, cx);
                    }
                }
                cx.notify();
            }
        })
    }

    fn remove_tab(&mut self, ix: usize, cx: &mut Context<Self>) -> OpenTab {
        let tab = self.tabs.remove(ix);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(a) if a > ix => Some(a - 1),
            Some(a) => Some(a.min(self.tabs.len() - 1)),
            None => None,
        };
        if self.sync_active {
            self.sync_tree(cx);
        }
        cx.notify();
        tab
    }

    // ------------------------------------------------- torn-off windows

    pub(crate) fn palette(&self) -> &Palette {
        &self.palette
    }

    pub(crate) fn proj_dirs(&self) -> &[String] {
        &self.proj_dirs
    }

    pub(crate) fn project_name_of(&self, path: &str) -> Option<String> {
        project_index(path, &self.proj_dirs).map(|ix| {
            self.proj_dirs[ix]
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string()
        })
    }

    pub(crate) fn write_doc(
        &mut self,
        rel: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let ws = self
            .ws
            .as_ref()
            .ok_or_else(|| "No folder open".to_string())?;
        ws.write_file(rel, text)?;
        self.lsp_saved(rel, text, cx);
        self.refresh(cx);
        Ok(())
    }

    /// A torn-off window went away.
    pub(crate) fn forget_window(&mut self, id: u64) {
        self.windows.retain(|w| w.id != id);
    }

    /// Every window a tab can be dropped on: the main one and the live torn-off ones.
    pub(crate) fn windows_snapshot(&self) -> Vec<(Dest, AnyWindowHandle)> {
        std::iter::once((Dest::Main, self.handle))
            .chain(
                self.windows
                    .iter()
                    .filter(|w| w.view.upgrade().is_some())
                    .map(|w| (Dest::Window(w.id), w.handle)),
            )
            .collect()
    }

    /// The "Move to ..." entries for the tab context menu of window `from` (None = main).
    pub(crate) fn window_menu(&self, from: Option<u64>) -> Vec<(String, Dest)> {
        let mut v = Vec::new();
        if from.is_some() {
            v.push(("Move to Main Window".to_string(), Dest::Main));
        }
        v.push(("Move to New Window".to_string(), Dest::New));
        for w in self.windows.iter().filter(|w| w.view.upgrade().is_some()) {
            if Some(w.id) != from {
                v.push((format!("Move to Window {}", w.id), Dest::Window(w.id)));
            }
        }
        v
    }

    /// Take ownership of a tab (re-attaches its dirty tracking to this window).
    pub(crate) fn adopt(&mut self, mut tab: OpenTab, cx: &mut Context<Self>) {
        tab._sub = self.track_dirty(&tab.editor, cx);
        self.tabs.push(tab);
        self.activate(self.tabs.len() - 1, cx);
    }

    /// Hand a tab to its new window.
    pub(crate) fn deliver(
        &mut self,
        tab: OpenTab,
        dest: Dest,
        at: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        match dest {
            Dest::Main => {
                self.adopt(tab, cx);
                let _ = self.handle.update(cx, |_, w, _| w.activate_window());
            }
            Dest::Window(id) => {
                let target = self
                    .windows
                    .iter()
                    .find(|w| w.id == id)
                    .and_then(|w| w.view.upgrade().map(|v| (v, w.handle)));
                match target {
                    Some((view, handle)) => {
                        view.update(cx, |d, cx| d.adopt(tab, cx));
                        let _ = handle.update(cx, |_, w, _| w.activate_window());
                    }
                    None => self.adopt(tab, cx),
                }
            }
            Dest::New => self.open_detached(tab, at, cx),
        }
    }

    fn open_detached(&mut self, tab: OpenTab, at: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        let id = self.next_win;
        self.next_win += 1;
        let main = cx.entity();
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("Kalem IDE".into()),
                ..Default::default()
            }),
            window_bounds: at.map(|p| {
                WindowBounds::Windowed(Bounds::new(
                    point(p.x - px(80.), p.y - px(12.)),
                    size(px(900.), px(640.)),
                ))
            }),
            ..Default::default()
        };
        // A new window draws its first frame immediately, and that frame reads this (main)
        // entity, so the window is opened once the current update has finished.
        cx.defer(move |cx| {
            let slot = Rc::new(std::cell::RefCell::new(Some(tab)));
            let slot2 = slot.clone();
            let m = main.clone();
            let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                let tab = slot2.borrow_mut().take().expect("tab is moved once");
                cx.new(|cx| Detached::new(id, m, tab, window, cx))
            });
            main.update(cx, |k, cx| match opened {
                Ok((handle, view)) => k.windows.push(WinRef {
                    id,
                    view: view.downgrade(),
                    handle,
                }),
                Err(e) => {
                    // Could not open a window: keep the tab where the user can still see it.
                    k.message = Some(format!("Could not open a new window: {e}"));
                    if let Some(tab) = slot.borrow_mut().take() {
                        k.adopt(tab, cx);
                    }
                }
            });
        });
    }

    fn move_tab(
        &mut self,
        ix: usize,
        dest: Dest,
        at: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if ix < self.tabs.len() && dest != Dest::Main {
            let tab = self.remove_tab(ix, cx);
            self.deliver(tab, dest, at, cx);
        }
    }

    /// A mouse-up anywhere (also outside the window): drop a dragged tab on whatever is there.
    fn tab_mouse_up(&mut self, screen: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tab_drag.take() else {
            return;
        };
        if window.bounds().contains(&screen) {
            return; // a click, or a drag that stayed inside
        }
        let wins: Vec<_> = self
            .windows_snapshot()
            .into_iter()
            .filter(|(d, _)| *d != Dest::Main)
            .collect();
        let dest = dest_at(&wins, screen, cx).unwrap_or(Dest::New);
        self.move_tab(ix, dest, Some(screen), cx);
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let (Some(ws), Some(ix)) = (&self.ws, self.active) else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(ix).filter(|t| t.edits_file()) else {
            return;
        };
        let text = tab.editor.read(cx).value();
        let editor = tab.editor.clone();
        let mut written = None;
        match ws.write_file(&tab.path, &text) {
            Ok(()) => {
                written = Some((tab.path.clone(), text.to_string()));
                // Every tab on this editor (the file and its diff) is saved with it.
                for t in self.tabs.iter_mut().filter(|t| t.editor == editor) {
                    t.saved = text.clone();
                    t.dirty = false;
                }
                self.message = None;
                self.refresh(cx);
            }
            Err(e) => self.message = Some(e),
        }
        if let Some((path, text)) = written {
            self.lsp_saved(&path, &text, cx);
        }
        cx.notify();
    }

    // ---------------------------------------------------------- quick open

    fn toggle_quick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.ws.is_none() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search files (Ctrl+P)"));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => {
                    let q = input.read(cx).value().to_string();
                    let hits = this
                        .ws
                        .as_ref()
                        .map(|w| quick_open(&w.files, &q, 12))
                        .unwrap_or_default();
                    if let Some(qk) = this.quick.as_mut() {
                        qk.hits = hits;
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    let first = this.quick.as_ref().and_then(|q| q.hits.first().cloned());
                    if let Some(p) = first {
                        this.close_quick(window, cx);
                        this.open_file(&p, window, cx);
                    }
                }
                _ => {}
            },
        );
        let hits = self
            .ws
            .as_ref()
            .map(|w| quick_open(&w.files, "", 12))
            .unwrap_or_default();
        input.update(cx, |s, cx| s.focus(window, cx));
        self.quick = Some(Quick {
            input,
            hits,
            _sub: sub,
        });
        cx.notify();
    }

    fn close_quick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.quick = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // ---------------------------------------------------- build / run / test

    fn start_task(&mut self, kind: TaskKind, cx: &mut Context<Self>) {
        let Some(ws) = &self.ws else {
            self.message = Some("Open a folder first".into());
            cx.notify();
            return;
        };
        self.bottom = Bottom::Output;
        let startup = self.startup.clone().or_else(|| ws.default_startup());
        match ws.plan(kind, startup.as_deref(), self.config) {
            Err(e) => {
                self.output
                    .push(format!("{} failed to start: {e}", kind.title()));
                self.status_text = e;
            }
            Ok(plan) => self.run_plan(plan, cx),
        }
        cx.notify();
    }

    /// Run a build/run/test command in the background and stream it to the Output panel.
    fn run_plan(&mut self, plan: TaskPlan, cx: &mut Context<Self>) {
        self.bottom = Bottom::Output;
        {
            {
                if let Some(old) = self.task.take() {
                    old.task.cancel();
                }
                self.output.clear();
                self.output.push(format!("> {}", plan.title));
                self.output.push(format!(
                    "$ {}   ({})",
                    plan.command_line(),
                    plan.cwd.display()
                ));
                self.status_text = format!("{}...", plan.title);
                self.task_gen += 1;
                let gen = self.task_gen;
                self.task = Some(TaskState {
                    task: spawn(&plan),
                    gen,
                });
                cx.spawn(async move |this, cx| loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(120))
                        .await;
                    let go = this
                        .update(cx, |this, cx| this.poll_task(gen, cx))
                        .unwrap_or(false);
                    if !go {
                        break;
                    }
                })
                .detach();
            }
        }
        cx.notify();
    }

    fn poll_task(&mut self, gen: u64, cx: &mut Context<Self>) -> bool {
        let Some(state) = &self.task else {
            return false;
        };
        if state.gen != gen {
            return false;
        }
        let mut finished: Option<String> = None;
        let mut succeeded = false;
        for _ in 0..800 {
            match state.task.rx.try_recv() {
                Ok(TaskEvent::Line(l)) => self.output.push(l),
                Ok(TaskEvent::Done(code)) => {
                    succeeded = code == Some(0);
                    finished = Some(match code {
                        Some(0) => "succeeded".to_string(),
                        Some(c) => format!("failed (exit code {c})"),
                        None => "cancelled".to_string(),
                    });
                    break;
                }
                Ok(TaskEvent::Failed(e)) => {
                    self.output.push(e.clone());
                    finished = Some(format!("failed: {e}"));
                    break;
                }
                Err(_) => break,
            }
        }
        if self.output.len() > MAX_OUTPUT {
            let drop = self.output.len() - MAX_OUTPUT;
            self.output.drain(..drop);
        }
        if let Some(f) = finished {
            let title = self
                .output
                .first()
                .map(|s| s.trim_start_matches("> ").to_string())
                .unwrap_or_default();
            self.status_text = format!("{title} {f}");
            self.output.push(String::new());
            self.output.push(format!("--- {f} ---"));
            self.task = None;
            self.refresh(cx);
            // A build started by "Start Debugging": launch the debugger when it worked.
            if let Some(plan) = self.pending_debug.take() {
                if succeeded {
                    self.begin_session(plan, cx);
                } else {
                    self.dbg = None;
                    self.status_text = "Debugging cancelled: the build did not succeed".into();
                }
            }
            return false;
        }
        self.output_scroll.scroll_to_bottom();
        cx.notify();
        true
    }

    fn cancel_task(&mut self, cx: &mut Context<Self>) {
        if let Some(t) = &self.task {
            t.task.cancel();
        }
        cx.notify();
    }
}

impl Kalem {
    /// The Find and Replace form (Find in Files / Replace in Files). Used floating and docked.
    fn find_form(&self, docked: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let accent = rgba(self.palette.get("accent"));
        let danger = rgba(self.palette.get("danger"));
        let muted = theme.muted_foreground;
        let v = cx.entity();
        let opts = self.find_opts;
        let replace_mode = self.find_mode == FindMode::Replace;
        let busy = self.find_busy;

        let mode_tab = |id: &'static str, label: &'static str, mode: FindMode| {
            let on = self.find_mode == mode;
            div()
                .id(id)
                .px_1()
                .pb_1()
                .cursor_pointer()
                .font_weight(if on {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if on { theme.foreground } else { muted })
                .border_b_2()
                .border_color(if on {
                    accent
                } else {
                    gpui_kit::transparent_black()
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.find_mode = mode;
                    this.replace_confirm = false;
                    cx.notify();
                }))
        };
        let check = |id: &'static str, label: &'static str, on: bool, set: fn(&mut Kalem, bool)| {
            let v = v.clone();
            Checkbox::new(id)
                .label(label)
                .checked(on)
                .on_click(move |b, w, cx| {
                    let b = *b;
                    v.update(cx, |this, cx| {
                        set(this, b);
                        this.find_changed(w, cx);
                    });
                })
        };
        let cur_scope = self.find_scope;
        let row = |label: &'static str, body: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().w(px(70.)).flex_none().text_sm().child(label))
                .child(div().flex_1().min_w_0().child(body))
        };

        let (n_hits, n_files) = self
            .find
            .as_ref()
            .map(|r| {
                let mut f: Vec<&str> = r.hits.iter().map(|h| h.path.as_str()).collect();
                f.dedup();
                (r.hits.len(), f.len())
            })
            .unwrap_or((0, 0));
        let replace_label = if self.replace_confirm {
            format!("Replace {n_hits} in {n_files} file(s): confirm")
        } else {
            "Replace All".to_string()
        };

        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(mode_tab(
                        "fm-find",
                        if docked { "Find" } else { "Find in Files" },
                        FindMode::Find,
                    ))
                    .child(mode_tab(
                        "fm-replace",
                        if docked {
                            "Replace"
                        } else {
                            "Replace in Files"
                        },
                        FindMode::Replace,
                    ))
                    .child(div().flex_1())
                    .when(docked, |d| {
                        d.child(
                            Button::new("f-float")
                                .ghost()
                                .compact()
                                .icon(icon("app-window"))
                                .tooltip("Float window")
                                .on_click(cx.listener(|this, _, w, cx| {
                                    this.set_find_place(FindPlace::Float, w, cx)
                                })),
                        )
                        .child(
                            Button::new("f-hide")
                                .ghost()
                                .compact()
                                .icon(icon("x"))
                                .tooltip("Close")
                                .on_click(cx.listener(|this, _, w, cx| {
                                    this.set_find_place(FindPlace::Hidden, w, cx)
                                })),
                        )
                    }),
            )
            .child(Input::new(&self.find_input))
            .when(replace_mode, |d| d.child(Input::new(&self.replace_input)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(check(
                        "c-case",
                        "Match case",
                        opts.case_sensitive,
                        |t, b| t.find_opts.case_sensitive = b,
                    ))
                    .child(check(
                        "c-word",
                        "Match whole word",
                        opts.whole_word,
                        |t, b| t.find_opts.whole_word = b,
                    ))
                    .child(check(
                        "c-regex",
                        "Use regular expressions",
                        opts.regex,
                        |t, b| t.find_opts.regex = b,
                    )),
            )
            .child(row(
                "Look in",
                Button::new("f-scope")
                    .compact()
                    .label(FIND_SCOPES[cur_scope.min(3)])
                    .dropdown_caret(true)
                    .dropdown_menu(move |mut m, _, _| {
                        for (i, n) in FIND_SCOPES.iter().enumerate() {
                            m = m.menu_with_check(*n, i == cur_scope, Box::new(SetFindScope(i)));
                        }
                        m
                    })
                    .into_any_element(),
            ))
            .child(row(
                "File types",
                Input::new(&self.types_input).into_any_element(),
            ))
            .child(check(
                "c-append",
                "Append results",
                self.find_append,
                |t, b| t.find_append = b,
            ))
            .when_some(self.find_error.clone(), |d, e| {
                d.child(div().text_xs().text_color(danger).child(e))
            })
            .when_some(self.find_note.clone(), |d, n| {
                d.child(div().text_xs().text_color(muted).child(n))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .when(replace_mode, |d| {
                        d.child(
                            Button::new("f-replace")
                                .compact()
                                .label(replace_label)
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, w, cx| this.replace_click(w, cx))),
                        )
                    })
                    .child(accent_button(
                        "f-findall",
                        "Find All",
                        accent,
                        theming::readable_on(accent, theme.background, theme.foreground),
                        !busy,
                        cx.listener(|this, _, w, cx| this.run_find(w, cx)),
                    )),
            )
            .into_any_element()
    }

    /// Results list of the last search (the "Find Results" window).
    fn find_results(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let list_active = theme.list_active;
        let v = cx.entity();
        let summary = match (&self.find, self.find_busy) {
            (_, true) => format!("Searching for '{}'...", self.find_query),
            (None, _) => "Press Find All to search".to_string(),
            (Some(r), _) if r.hits.is_empty() => format!(
                "No results for '{}' in {} files",
                self.find_query, r.files_searched
            ),
            (Some(r), _) => format!(
                "{}{} results for '{}' in {} files",
                r.hits.len(),
                if r.truncated { "+" } else { "" },
                self.find_query,
                r.files_searched
            ),
        };
        let hits: Vec<_> = self
            .find
            .as_ref()
            .map(|r| r.hits.clone())
            .unwrap_or_default();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .child(div().flex_1().text_xs().text_color(muted).child(summary))
                    .child(
                        Button::new("f-prev")
                            .ghost()
                            .compact()
                            .icon(icon("arrow-up"))
                            .tooltip("Find Previous (Shift+F3)")
                            .on_click(
                                cx.listener(|this, _, w, cx| this.find_step(-1, false, w, cx)),
                            ),
                    )
                    .child(
                        Button::new("f-next")
                            .ghost()
                            .compact()
                            .icon(icon("arrow-down"))
                            .tooltip("Find Next (F3)")
                            .on_click(
                                cx.listener(|this, _, w, cx| this.find_step(1, false, w, cx)),
                            ),
                    )
                    .child(
                        Button::new("f-next-file")
                            .ghost()
                            .compact()
                            .label("Next file")
                            .tooltip("Skip to the next file with a match")
                            .on_click(cx.listener(|this, _, w, cx| this.find_step(1, true, w, cx))),
                    )
                    .when(self.find_place == FindPlace::Hidden, |d| {
                        d.child(
                            Button::new("f-open")
                                .ghost()
                                .compact()
                                .icon(icon("search"))
                                .label("Find...")
                                .on_click(cx.listener(|this, _, w, cx| this.show_find(w, cx))),
                        )
                    }),
            )
            .child(
                div()
                    .id("find-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.find_scroll)
                    .children(hits.into_iter().enumerate().map(|(i, h)| {
                        let v = v.clone();
                        let (path, line, col) = (h.path.clone(), h.line, h.col);
                        div()
                            .id(("find-hit", i))
                            .flex()
                            .gap_2()
                            .px_2()
                            .text_sm()
                            .cursor_pointer()
                            .when(self.find_cur == Some(i), |d| d.bg(list_active))
                            .hover(move |d| d.bg(list_active))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(muted)
                                    .child(format!("{}:{}", h.path, h.line)),
                            )
                            .child(div().truncate().child(h.text))
                            .on_click(move |_, window, cx| {
                                let p = path.clone();
                                v.update(cx, |this, cx| {
                                    this.find_cur = Some(i);
                                    this.open_at(&p, line, col, window, cx)
                                });
                            })
                    })),
            )
            .into_any_element()
    }

    /// The floating Find and Replace window plus, while it is dragged, the docking guides.
    fn find_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let accent = rgba(self.palette.get("accent"));
        let mut out: Vec<AnyElement> = Vec::new();
        if self.find_place != FindPlace::Float {
            return out;
        }
        let dragging = self.find_drag.is_some();
        out.push(
            div()
                .absolute()
                .left(self.find_pos.x)
                .top(self.find_pos.y)
                .w(px(440.))
                .rounded(px(6.))
                .border_1()
                .border_color(accent)
                .bg(theme.popover)
                .shadow_lg()
                .when(dragging, |d| d.opacity(0.85))
                .child(
                    div()
                        .id("find-title")
                        .flex()
                        .items_center()
                        .justify_between()
                        .pl_3()
                        .pr_1()
                        .h(px(32.))
                        .cursor_move()
                        .child(div().text_sm().child("Find and Replace"))
                        .child(
                            Button::new("f-close")
                                .ghost()
                                .compact()
                                .icon(icon("x"))
                                .tooltip("Close")
                                .on_click(cx.listener(|this, _, w, cx| {
                                    this.set_find_place(FindPlace::Hidden, w, cx)
                                })),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                                this.find_drag = Some(ev.position - this.find_pos);
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        ),
                )
                .child(self.find_form(false, cx))
                .into_any_element(),
        );
        if dragging {
            let size = window.viewport_size();
            // Where the window would land if dropped on the highlighted guide.
            let preview = match self.find_hover {
                Some(Guide::Bottom) => Some(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(22.))
                        .h(px(220.)),
                ),
                Some(Guide::Right) => Some(
                    div()
                        .absolute()
                        .right_0()
                        .top(px(68.))
                        .w(px(320.))
                        .bottom(px(242.)),
                ),
                _ => None,
            };
            if let Some(p) = preview {
                out.push(
                    p.bg(accent.opacity(0.18))
                        .border_2()
                        .border_color(accent)
                        .into_any_element(),
                );
            }
            for (g, b) in guide_rects(size) {
                let hot = self.find_hover == Some(g) && g.available();
                let strip = |d: Div| match g {
                    Guide::Bottom => d.left(px(3.)).right(px(3.)).bottom(px(3.)).h(px(7.)),
                    Guide::Right => d.top(px(3.)).bottom(px(3.)).right(px(3.)).w(px(7.)),
                    Guide::Top => d.left(px(3.)).right(px(3.)).top(px(3.)).h(px(7.)),
                    Guide::Left => d.top(px(3.)).bottom(px(3.)).left(px(3.)).w(px(7.)),
                    Guide::Float => d.left(px(10.)).right(px(10.)).top(px(10.)).bottom(px(10.)),
                };
                out.push(
                    div()
                        .absolute()
                        .left(b.origin.x)
                        .top(b.origin.y)
                        .size(px(GUIDE))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(if g.available() { accent } else { theme.border })
                        .bg(theme.popover)
                        .when(!g.available(), |d| d.opacity(0.4))
                        .when(hot, |d| d.bg(accent.opacity(0.35)))
                        .child(strip(div().absolute()).bg(if g.available() {
                            accent
                        } else {
                            theme.muted_foreground
                        }))
                        .into_any_element(),
                );
            }
        }
        out
    }
}

/// A filled button in the accent colour (the library's own "custom" button renders washed out).
fn accent_button(
    id: &'static str,
    label: impl Into<SharedString>,
    accent: Hsla,
    fg: Hsla,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let b = div()
        .id(id)
        .h(px(28.))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(4.))
        .text_sm()
        .bg(accent)
        .text_color(fg)
        .child(label.into());
    if enabled {
        b.cursor_pointer()
            .hover(move |d| d.bg(accent.opacity(0.85)))
            .on_click(on_click)
            .into_any_element()
    } else {
        b.opacity(0.4).into_any_element()
    }
}

fn badge(kind: ChangeKind) -> (&'static str, u8) {
    // letter, colour class: 0 modified, 1 added, 2 deleted, 3 neutral
    match kind {
        ChangeKind::Modified => ("M", 0),
        ChangeKind::Added => ("A", 1),
        ChangeKind::Deleted => ("D", 2),
        ChangeKind::Renamed => ("R", 0),
        ChangeKind::Copied => ("C", 1),
        ChangeKind::Untracked => ("U", 1),
        ChangeKind::Conflicted => ("!", 2),
        ChangeKind::Ignored | ChangeKind::Other => ("?", 3),
    }
}

fn kind_icon(k: ProjectKind) -> &'static str {
    match k {
        ProjectKind::Web => "globe",
        ProjectKind::App => "app-window",
        ProjectKind::Library => "library",
        ProjectKind::Test => "flask-conical",
        ProjectKind::Generic => "package",
    }
}

fn file_icon(name: &str, pal: &Palette, muted: Hsla) -> AnyElement {
    if is_image(name) {
        return icon("image")
            .size(px(16.))
            .text_color(rgba(pal.get("solution")))
            .into_any_element();
    }
    let base = icon("file").size(px(18.)).text_color(muted);
    match badge_for(name) {
        None => base.into_any_element(),
        Some(b) => {
            let color = pal.badge(b.label).unwrap_or(b.color);
            div()
                .relative()
                .size(px(18.))
                .child(base)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(3.))
                        .flex()
                        .justify_center()
                        .text_size(px(7.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgba(color))
                        .child(b.label),
                )
                .into_any_element()
        }
    }
}

/// A tab in a panel's tab strip; the active one gets the accent outline.
#[allow(clippy::too_many_arguments)]
fn panel_tab(
    id: &'static str,
    label: String,
    active: bool,
    line: Hsla,
    fg: Hsla,
    muted: Hsla,
    bg_active: Hsla,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    // The active tab hangs off the panel border like in Visual Studio: it repeats the border on
    // its sides and bottom and covers the panel's bottom line under itself.
    div()
        .id(id)
        .h(px(30.))
        .px_3()
        .flex()
        .items_center()
        .text_sm()
        .cursor_pointer()
        .border_2()
        .border_t_0()
        .rounded_b(px(5.))
        .border_color(if active {
            line
        } else {
            gpui_kit::transparent_black()
        })
        .bg(if active {
            bg_active
        } else {
            // Transparent: a filled inactive tab would paint over the frame's bottom line.
            gpui_kit::transparent_black()
        })
        .text_color(if active { fg } else { muted })
        .child(label)
        .on_click(on_click)
}

impl TabHost for Kalem {
    fn tab_activate(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.activate(ix, cx);
    }

    fn tab_close(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Self>) {
        self.close_tab(ix, cx);
    }

    fn tab_grab(&mut self, ix: usize) {
        self.tab_drag = Some(ix);
    }
}

impl Render for Kalem {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let pal = self.palette.clone();
        let c = |t: &str| rgba(pal.get(t));
        let (accent, muted, border) = (c("accent"), theme.muted_foreground, theme.border);
        let view = cx.entity();
        let (list_active, popover_bg) = (theme.list_active, theme.popover);
        let ws_open = self.ws.is_some();
        let running = self.task.is_some();
        let debugging = self.dbg.is_some();
        let paused = self.debug_paused_now();

        // ---------------------------------------------------------- menu bar
        let theme_names: Vec<String> = self.themes.iter().map(|t| t.name.clone()).collect();
        let projects: Vec<String> = self
            .ws
            .as_ref()
            .map(|w| w.projects().into_iter().map(|p| p.name).collect())
            .unwrap_or_default();
        let m =
            |id: &'static str, label: &'static str| Button::new(id).ghost().compact().label(label);
        let mode = self.settings.mode;
        let cur_theme = self.settings.theme.clone();
        let menu_bar = div()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .h(px(32.))
            .child(m("m-file", "File").dropdown_menu(|m, _, _| {
                m.menu("Open Folder...", Box::new(OpenFolder))
                    .menu("Save", Box::new(Save))
                    .menu("Close Tab", Box::new(CloseTab))
                    .separator()
                    .menu("Exit", Box::new(Quit))
            }))
            .child(m("m-edit", "Edit").dropdown_menu(|m, _, _| {
                m.menu("Undo", Box::new(input::Undo))
                    .menu("Redo", Box::new(input::Redo))
                    .separator()
                    .menu("Cut", Box::new(input::Cut))
                    .menu("Copy", Box::new(input::Copy))
                    .menu("Paste", Box::new(input::Paste))
                    .menu("Select All", Box::new(input::SelectAll))
                    .separator()
                    .menu("Find in Files", Box::new(ShowFind))
                    .menu("Find Next (F3)", Box::new(FindNext))
                    .menu("Find Previous (Shift+F3)", Box::new(FindPrev))
                    .menu("Find Next File", Box::new(FindNextFile))
                    .separator()
                    .menu("Find All References (Shift+F12)", Box::new(FindReferences))
                    .menu("Rename Symbol (F2)", Box::new(RenameSymbol))
                    .menu("Format Document (Shift+Alt+F)", Box::new(FormatDocument))
            }))
            .child(m("m-view", "View").dropdown_menu(move |m, w, cx| {
                let names = theme_names.clone();
                let cur = cur_theme.clone();
                m.menu("Solution Explorer", Box::new(ShowExplorer))
                    .menu("Git Changes", Box::new(ShowGit))
                    .menu("Output", Box::new(ShowOutput))
                    .menu("Test Results", Box::new(ShowTests))
                    .separator()
                    .menu("Sync with Active Document", Box::new(ToggleSync))
                    .menu("Refresh", Box::new(Refresh))
                    .separator()
                    .submenu("Appearance", w, cx, move |m, _, _| {
                        m.menu_with_check("Light", mode == Mode::Light, Box::new(ModeLight))
                            .menu_with_check("Dark", mode == Mode::Dark, Box::new(ModeDark))
                            .menu_with_check("System", mode == Mode::System, Box::new(ModeSystem))
                    })
                    .submenu("Theme", w, cx, move |mut m, _, _| {
                        for (i, n) in names.iter().enumerate() {
                            m = m.menu_with_check(n.clone(), *n == cur, Box::new(SetTheme(i)));
                        }
                        m.separator()
                            .menu("Open Themes Folder", Box::new(OpenThemes))
                            .menu("Reload Themes", Box::new(ReloadThemes))
                    })
            }))
            .child(m("m-git", "Git").dropdown_menu(|m, _, _| {
                m.menu("Refresh Status", Box::new(Refresh))
                    .menu("Show Git Changes", Box::new(ShowGit))
                    .separator()
                    .menu("Fetch", Box::new(GitFetch))
                    .menu("Pull", Box::new(GitPull))
                    .menu("Push", Box::new(GitPush))
                    .separator()
                    .menu("Stash All Changes", Box::new(GitStashAll))
            }))
            .child(m("m-project", "Project").dropdown_menu({
                let names = projects.clone();
                move |mut m, _, _| {
                    m = m.label("Set Startup Project");
                    for (i, n) in names.iter().enumerate() {
                        m = m.menu(n.clone(), Box::new(SetStartup(i)));
                    }
                    m
                }
            }))
            .child(m("m-build", "Build").dropdown_menu(move |m, _, _| {
                m.menu_with_enable("Build Solution", Box::new(Build), ws_open)
                    .menu_with_enable("Cancel", Box::new(CancelTask), running)
            }))
            .child(m("m-debug", "Debug").dropdown_menu(move |m, _, _| {
                m.menu_with_enable("Start Without Debugging", Box::new(RunNoDebug), ws_open)
                    .separator()
                    .menu_with_enable("Start / Continue (F5)", Box::new(StartDebugging), ws_open)
                    .menu_with_enable(
                        "Stop Debugging (Shift+F5)",
                        Box::new(StopDebugging),
                        debugging,
                    )
                    .menu_with_enable("Pause", Box::new(DebugPause), debugging)
                    .separator()
                    .menu_with_enable("Step Over (F10)", Box::new(StepOver), debugging)
                    .menu_with_enable("Step Into (F11)", Box::new(StepInto), debugging)
                    .menu_with_enable("Step Out (Shift+F11)", Box::new(StepOut), debugging)
                    .separator()
                    .menu("Toggle Breakpoint (F9)", Box::new(ToggleBreakpoint))
            }))
            .child(m("m-test", "Test").dropdown_menu(move |m, _, _| {
                m.menu_with_enable("Run All Tests", Box::new(RunTests), ws_open)
                    .menu("Show Test Results", Box::new(ShowTests))
            }))
            .child(m("m-tools", "Tools").dropdown_menu(|m, _, _| m.label("Nothing here yet")))
            .child(m("m-ext", "Extensions").dropdown_menu(|m, _, _| m.label("Nothing here yet")))
            .child(
                m("m-win", "Window")
                    .dropdown_menu(|m, _, _| m.menu("Close Tab", Box::new(CloseTab))),
            )
            .child(m("m-help", "Help").dropdown_menu(|m, _, _| {
                m.menu("About Kalem IDE", Box::new(About))
                    .link("Project Page", "https://github.com/G-HK/KALEM-IDE")
            }))
            .child(
                Button::new("search")
                    .ghost()
                    .compact()
                    .icon(icon("search"))
                    .label("Search")
                    .disabled(!ws_open)
                    .on_click(cx.listener(|this, _, w, cx| this.toggle_quick(w, cx))),
            )
            .child(div().flex_1())
            .when_some(self.ws.as_ref().map(|w| w.name()), |d, name| {
                d.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(border)
                        .child(name),
                )
            });

        // ----------------------------------------------------------- toolbar
        let startup_label = {
            let dir = self.startup.clone();
            dir.and_then(|d| d.rsplit('/').next().map(str::to_string))
                .or_else(|| projects.first().cloned())
                .unwrap_or_else(|| "(no project)".into())
        };
        let toolbar = div()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .h(px(36.))
            .border_y_1()
            .border_color(border)
            .child(
                Button::new("back")
                    .ghost()
                    .compact()
                    .icon(icon("arrow-left"))
                    .tooltip("Navigate Backward (Alt+Left)")
                    .disabled(self.nav_pos == 0)
                    .on_click(cx.listener(|this, _, w, cx| this.navigate(-1, w, cx))),
            )
            .child(
                Button::new("fwd")
                    .ghost()
                    .compact()
                    .icon(icon("arrow-right"))
                    .tooltip("Navigate Forward (Alt+Right)")
                    .disabled(self.nav_pos + 1 >= self.nav.len())
                    .on_click(cx.listener(|this, _, w, cx| this.navigate(1, w, cx))),
            )
            .child(
                Button::new("open")
                    .ghost()
                    .compact()
                    .icon(icon("folder-open"))
                    .tooltip("Open Folder (Ctrl+O)")
                    .on_click(cx.listener(|this, _, w, cx| this.pick_folder(w, cx))),
            )
            .child(
                Button::new("save")
                    .ghost()
                    .compact()
                    .icon(icon("save"))
                    .tooltip("Save (Ctrl+S)")
                    .disabled(self.active.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
            )
            .child(
                Button::new("undo")
                    .ghost()
                    .compact()
                    .icon(icon("undo-2"))
                    .tooltip("Undo (Ctrl+Z)")
                    .disabled(self.active.is_none())
                    .on_click(|_, w, cx| w.dispatch_action(Box::new(input::Undo), cx)),
            )
            .child(
                Button::new("redo")
                    .ghost()
                    .compact()
                    .icon(icon("redo-2"))
                    .tooltip("Redo (Ctrl+Y)")
                    .disabled(self.active.is_none())
                    .on_click(|_, w, cx| w.dispatch_action(Box::new(input::Redo), cx)),
            )
            .child(div().w(px(1.)).h(px(18.)).mx_1().bg(border))
            .child(
                Button::new("startup")
                    .ghost()
                    .compact()
                    .label(startup_label)
                    .dropdown_caret(true)
                    .dropdown_menu({
                        let names = projects.clone();
                        move |mut m, _, _| {
                            for (i, n) in names.iter().enumerate() {
                                m = m.menu(n.clone(), Box::new(SetStartup(i)));
                            }
                            m
                        }
                    }),
            )
            .child(
                Button::new("config")
                    .ghost()
                    .compact()
                    .label(self.config.name())
                    .dropdown_caret(true)
                    .dropdown_menu(|m, _, _| {
                        m.menu("Debug", Box::new(ConfigDebug))
                            .menu("Release", Box::new(ConfigRelease))
                    }),
            )
            .child(
                Button::new("run")
                    .ghost()
                    .compact()
                    .icon(icon("play").text_color(c("success")))
                    .label("Run")
                    .tooltip("Start Without Debugging (Ctrl+F5)")
                    .disabled(!ws_open || running)
                    .on_click(cx.listener(|this, _, _, cx| this.start_task(TaskKind::Run, cx))),
            )
            .child(
                Button::new("debug")
                    .ghost()
                    .compact()
                    .icon(icon(if paused { "play" } else { "bug" }).text_color(c("accent")))
                    .tooltip(if paused {
                        "Continue (F5)"
                    } else {
                        "Start Debugging (F5)"
                    })
                    .disabled(!ws_open || (running && !debugging) || (debugging && !paused))
                    .on_click(cx.listener(|this, _, _, cx| this.start_debugging(cx))),
            )
            .when(debugging, |d| {
                d.child(
                    Button::new("dbg-pause")
                        .ghost()
                        .compact()
                        .icon(icon("pause"))
                        .tooltip("Pause")
                        .disabled(paused)
                        .on_click(cx.listener(|this, _, _, cx| this.debug_pause(cx))),
                )
                .child(
                    Button::new("dbg-over")
                        .ghost()
                        .compact()
                        .icon(icon("arrow-right"))
                        .tooltip("Step Over (F10)")
                        .disabled(!paused)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.debug_step(debugger::Step::Over, cx)),
                        ),
                )
                .child(
                    Button::new("dbg-into")
                        .ghost()
                        .compact()
                        .icon(icon("arrow-down"))
                        .tooltip("Step Into (F11)")
                        .disabled(!paused)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.debug_step(debugger::Step::Into, cx)),
                        ),
                )
                .child(
                    Button::new("dbg-out")
                        .ghost()
                        .compact()
                        .icon(icon("arrow-up"))
                        .tooltip("Step Out (Shift+F11)")
                        .disabled(!paused)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.debug_step(debugger::Step::Out, cx)),
                        ),
                )
                .child(
                    Button::new("dbg-stop")
                        .ghost()
                        .compact()
                        .icon(icon("square").text_color(c("danger")))
                        .tooltip("Stop Debugging (Shift+F5)")
                        .on_click(cx.listener(|this, _, _, cx| this.stop_debugging(cx))),
                )
            })
            .child(
                Button::new("build")
                    .ghost()
                    .compact()
                    .icon(icon("hammer"))
                    .tooltip("Build (Ctrl+Shift+B)")
                    .disabled(!ws_open || running)
                    .on_click(cx.listener(|this, _, _, cx| this.start_task(TaskKind::Build, cx))),
            )
            .child(
                Button::new("test")
                    .ghost()
                    .compact()
                    .icon(icon("flask-conical"))
                    .tooltip("Run All Tests")
                    .disabled(!ws_open || running)
                    .on_click(cx.listener(|this, _, _, cx| this.start_task(TaskKind::Test, cx))),
            )
            .when(running, |d| {
                d.child(
                    Button::new("stop")
                        .ghost()
                        .compact()
                        .icon(icon("square").text_color(c("danger")))
                        .tooltip("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_task(cx))),
                )
            })
            .child(
                Button::new("theme-toggle")
                    .ghost()
                    .compact()
                    .icon(icon(if self.dark { "sun" } else { "moon" }))
                    .tooltip(if self.dark {
                        "Switch to Light mode (Black Panda palette)"
                    } else {
                        "Switch to Dark mode"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        let to = if this.dark { Mode::Light } else { Mode::Dark };
                        this.set_mode(to, cx)
                    })),
            )
            .child(
                Button::new("refresh")
                    .ghost()
                    .compact()
                    .icon(icon("refresh-cw"))
                    .tooltip("Refresh (F5)")
                    .disabled(!ws_open)
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            .child(
                Button::new("sync")
                    .ghost()
                    .compact()
                    .label(if self.sync_active {
                        "Sync: on"
                    } else {
                        "Sync: off"
                    })
                    .tooltip("Sync with Active Document")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sync_active = !this.sync_active;
                        if this.sync_active {
                            this.sync_tree(cx);
                        }
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(self.message.clone().unwrap_or_default()),
            );

        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(theme.background)
            .text_color(theme.foreground)
            .track_focus(&self.focus)
            .key_context("Kalem")
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, window, cx| {
                if let Some(off) = this.find_drag {
                    let p = ev.position - off;
                    this.find_pos = point(p.x.max(px(0.)), p.y.max(px(0.)));
                    this.find_hover = guide_at(window.viewport_size(), ev.position);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.end_find_drag(window, cx)),
            )
            .on_action(cx.listener(|this, _: &StartDebugging, _, cx| this.start_debugging(cx)))
            .on_action(cx.listener(|this, _: &StopDebugging, _, cx| this.stop_debugging(cx)))
            .on_action(cx.listener(|this, _: &DebugPause, _, cx| this.debug_pause(cx)))
            .on_action(
                cx.listener(|this, _: &StepOver, _, cx| this.debug_step(debugger::Step::Over, cx)),
            )
            .on_action(
                cx.listener(|this, _: &StepInto, _, cx| this.debug_step(debugger::Step::Into, cx)),
            )
            .on_action(
                cx.listener(|this, _: &StepOut, _, cx| this.debug_step(debugger::Step::Out, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleBreakpoint, _, cx| this.toggle_breakpoint(cx)))
            .on_action(cx.listener(|this, _: &Save, _, cx| this.save(cx)))
            .on_action(cx.listener(|this, a: &MoveTab, _, cx| this.move_tab(a.0, a.1, None, cx)))
            .on_action(cx.listener(|this, _: &OpenFolder, w, cx| this.pick_folder(w, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, _, cx| {
                if let Some(i) = this.active {
                    this.close_tab(i, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.refresh(cx)))
            .on_action(cx.listener(|this, _: &ToggleSync, _, cx| {
                this.sync_active = !this.sync_active;
                if this.sync_active {
                    this.sync_tree(cx);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowExplorer, _, cx| {
                this.reveal_right(Right::Explorer);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowGit, _, cx| {
                this.reveal_right(Right::Git);
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, a: &DockRight, _, cx| this.dock_right(Right::from_ix(a.0), cx)),
            )
            .on_action(cx.listener(|this, a: &AutoHideRight, _, cx| {
                this.autohide_right(Right::from_ix(a.0), cx)
            }))
            .on_action(
                cx.listener(|this, _: &GitSync, w, cx| this.git_op("sync", GitOp::Sync, w, cx)),
            )
            .on_action(cx.listener(|this, _: &GitLog, w, cx| {
                this.bottom = Bottom::Output;
                this.git_op("log", GitOp::Log, w, cx)
            }))
            .on_action(
                cx.listener(|this, _: &GitFetch, w, cx| this.git_op("fetch", GitOp::Fetch, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &GitPull, w, cx| this.git_op("pull", GitOp::Pull, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &GitPush, w, cx| this.git_op("push", GitOp::Push, w, cx)),
            )
            .on_action(cx.listener(|this, _: &GitStashAll, w, cx| {
                this.git_op("stash", GitOp::StashPush, w, cx)
            }))
            .on_action(cx.listener(|this, a: &CheckoutBranch, w, cx| {
                this.git_op("checkout", GitOp::Checkout(a.0.clone()), w, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowOutput, _, cx| {
                this.bottom = Bottom::Output;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowFind, w, cx| this.show_find(w, cx)))
            .on_action(cx.listener(|this, _: &FindReferences, _, cx| this.find_references(cx)))
            .on_action(cx.listener(|this, _: &RenameSymbol, w, cx| this.start_rename(w, cx)))
            .on_action(cx.listener(|this, _: &FormatDocument, _, cx| this.format_document(cx)))
            .on_action(cx.listener(|this, _: &CloseRename, w, cx| this.close_rename(w, cx)))
            .on_action(cx.listener(|this, _: &FindNext, w, cx| this.find_step(1, false, w, cx)))
            .on_action(cx.listener(|this, _: &FindPrev, w, cx| this.find_step(-1, false, w, cx)))
            .on_action(cx.listener(|this, _: &FindNextFile, w, cx| this.find_step(1, true, w, cx)))
            .on_action(cx.listener(|this, _: &FindClose, w, cx| {
                this.set_find_place(FindPlace::Hidden, w, cx)
            }))
            .on_action(cx.listener(|this, a: &SetFindScope, w, cx| {
                this.find_scope = a.0.min(3);
                this.find_changed(w, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowTests, _, cx| {
                this.bottom = Bottom::Tests;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Build, _, cx| this.start_task(TaskKind::Build, cx)))
            .on_action(
                cx.listener(|this, _: &RunNoDebug, _, cx| this.start_task(TaskKind::Run, cx)),
            )
            .on_action(cx.listener(|this, _: &RunTests, _, cx| this.start_task(TaskKind::Test, cx)))
            .on_action(cx.listener(|this, _: &CancelTask, _, cx| this.cancel_task(cx)))
            .on_action(cx.listener(|this, _: &NavBack, w, cx| this.navigate(-1, w, cx)))
            .on_action(cx.listener(|this, _: &NavForward, w, cx| this.navigate(1, w, cx)))
            .on_action(cx.listener(|this, _: &QuickOpen, w, cx| this.toggle_quick(w, cx)))
            .on_action(cx.listener(|this, _: &CloseQuick, w, cx| this.close_quick(w, cx)))
            .on_action(cx.listener(|this, _: &ModeLight, _, cx| this.set_mode(Mode::Light, cx)))
            .on_action(cx.listener(|this, _: &ModeDark, _, cx| this.set_mode(Mode::Dark, cx)))
            .on_action(cx.listener(|this, _: &ModeSystem, _, cx| this.set_mode(Mode::System, cx)))
            .on_action(cx.listener(|this, _: &ConfigDebug, _, cx| {
                this.config = Config::Debug;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ConfigRelease, _, cx| {
                this.config = Config::Release;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenThemes, _, cx| this.open_themes_folder(cx)))
            .on_action(cx.listener(|this, _: &ReloadThemes, _, cx| this.reload_themes(cx)))
            .on_action(cx.listener(|this, a: &SetTheme, _, cx| this.set_theme(a.0, cx)))
            .on_action(cx.listener(|this, a: &SetStartup, _, cx| {
                if let Some(ws) = &this.ws {
                    this.startup = ws.projects().get(a.0).map(|p| p.dir.clone());
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &About, _, cx| {
                this.message = Some(format!(
                    "Kalem IDE {} - MIT OR Apache-2.0",
                    env!("CARGO_PKG_VERSION")
                ));
                cx.notify();
            }))
            .child(menu_bar)
            .child(toolbar);

        let Some(ws) = &self.ws else {
            return root.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(div().text_xl().child("Kalem IDE"))
                    .child(
                        div()
                            .text_color(muted)
                            .child("Open a folder or a .kalem / .slnx / .sln file (Ctrl+O)"),
                    ),
            );
        };

        // ------------------------------------------------- Solution Explorer
        let proj_kinds = self.proj_kinds.clone();
        let changes = self.changes.clone();
        let git_ok = self.git.is_ok();
        let pal2 = pal.clone();
        let (folder_c, project_c, lock_c) = (c("folder"), c("project"), c("lock"));
        let (success_c, warning_c, danger_c, solution_c) =
            (c("success"), c("warning"), c("danger"), c("solution"));
        let n_projects = ws.project_count();
        let sol_title = if ws.solution.is_some() || n_projects > 0 {
            format!(
                "Solution '{}' ({} of {} projects)",
                ws.name(),
                n_projects,
                n_projects
            )
        } else {
            format!("Folder '{}'", ws.name())
        };
        let explorer = div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .child(icon("lock").size_3().text_color(lock_c))
                    .child(icon("layers").size_4().text_color(solution_c))
                    .child(div().text_sm().truncate().child(sol_title)),
            )
            .child(
                div().flex_1().min_h_0().child(
                    tree(&self.tree, move |ix, entry, _sel, _w, _cx| {
                        let id = entry.item().id.clone();
                        let is_folder = entry.is_folder();
                        let name = entry.item().label.to_string();
                        let view = view.clone();
                        let chevron = div().w(px(16.)).when(is_folder, |d| {
                            d.child(
                                icon(if entry.is_expanded() {
                                    "chevron-down"
                                } else {
                                    "chevron-right"
                                })
                                .size_4()
                                .text_color(muted),
                            )
                        });
                        let kind = proj_kinds.get(id.as_ref()).copied();
                        let status = changes.get(id.as_ref()).copied();
                        let marker =
                            div()
                                .w(px(14.))
                                .flex()
                                .justify_center()
                                .map(|d| match status {
                                    Some(
                                        ChangeKind::Added
                                        | ChangeKind::Untracked
                                        | ChangeKind::Copied,
                                    ) => d.child(icon("plus").size_3().text_color(success_c)),
                                    Some(ChangeKind::Deleted | ChangeKind::Conflicted) => {
                                        d.child(icon("pencil").size_3().text_color(danger_c))
                                    }
                                    Some(_) => {
                                        d.child(icon("pencil").size_3().text_color(warning_c))
                                    }
                                    None if git_ok && (!is_folder || kind.is_some()) => {
                                        d.child(icon("lock").size_3().text_color(lock_c))
                                    }
                                    None => d,
                                });
                        let main_icon: AnyElement = if let Some(k) = kind {
                            icon(kind_icon(k))
                                .size_4()
                                .text_color(project_c)
                                .into_any_element()
                        } else if is_folder {
                            icon(if entry.is_expanded() {
                                "folder-open"
                            } else {
                                "folder"
                            })
                            .size_4()
                            .text_color(folder_c)
                            .into_any_element()
                        } else {
                            file_icon(&name, &pal2, muted)
                        };
                        ListItem::new(ix)
                            .pl(px(4. + 16. * entry.depth() as f32))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(chevron)
                                    .child(marker)
                                    .child(main_icon)
                                    .child(name),
                            )
                            .when(!is_folder, move |li| {
                                li.on_click(move |_, window, cx| {
                                    let id = id.clone();
                                    view.update(cx, |this, cx| this.open_file(&id, window, cx));
                                })
                            })
                    })
                    .size_full(),
                ),
            );

        // ------------------------------------------------------ Git Changes
        let busy = self.git_busy;
        let git_panel = match &self.git {
            Err(e) => div()
                .p_2()
                .text_color(muted)
                .child(e.clone())
                .into_any_element(),
            Ok(st) => {
                let ov = ChangesOverview::from_status(st);
                let message = self.commit_msg.read(cx).value().to_string();
                let commit_btn = ov.commit_button(&message, self.amend);
                let colour = |class: u8| match class {
                    0 => warning_c,
                    1 => success_c,
                    2 => danger_c,
                    _ => muted,
                };
                let v = cx.entity();

                // One changed file: [letter] name dir ............ [+/-]
                let row = |id: usize, path: String, k: ChangeKind, staged: bool| {
                    let (letter, class) = badge(k);
                    let (dir, name) = match path.trim_end_matches('/').rsplit_once('/') {
                        Some((d, n)) => (format!("{d}/"), n.to_string()),
                        None => (String::new(), path.trim_end_matches('/').to_string()),
                    };
                    let (open, act) = (v.clone(), v.clone());
                    let p_open = path.clone();
                    div()
                        .id(("chg", id))
                        .group("chg-row")
                        .flex()
                        .items_center()
                        .pr_1()
                        .text_sm()
                        .hover(move |d| d.bg(list_active))
                        .child(
                            div()
                                .id(("chg-open", id))
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_2()
                                .cursor_pointer()
                                .child(
                                    div()
                                        .w(px(14.))
                                        .flex_none()
                                        .text_color(colour(class))
                                        .child(letter),
                                )
                                .child(div().flex_none().child(if path.ends_with('/') {
                                    format!("{name}/")
                                } else {
                                    name
                                }))
                                .child(div().truncate().text_xs().text_color(muted).child(dir))
                                .on_click(move |ev, window, cx| {
                                    let p = p_open.clone();
                                    let untracked = k == ChangeKind::Untracked;
                                    open.update(cx, |this, cx| {
                                        // Click: what changed. Double click: the file itself.
                                        if ev.click_count() >= 2 || p.ends_with('/') {
                                            this.open_file(&p, window, cx)
                                        } else {
                                            this.open_diff(&p, staged, untracked, window, cx)
                                        }
                                    });
                                }),
                        )
                        .child(
                            Button::new(("chg-act", id))
                                .ghost()
                                .compact()
                                .icon(icon(if staged { "minus" } else { "plus" }))
                                .tooltip(if staged { "Unstage" } else { "Stage" })
                                .on_click(move |_, window, cx| {
                                    let paths = vec![path.clone()];
                                    act.update(cx, |this, cx| {
                                        let op = if staged {
                                            GitOp::Unstage(paths)
                                        } else {
                                            GitOp::Stage(paths)
                                        };
                                        this.git_op("stage", op, window, cx)
                                    });
                                }),
                        )
                };
                let head = |t: String, action: Option<AnyElement>| {
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .pt_2()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .child(t)
                        .children(action)
                };
                let staged: Vec<_> = st
                    .entries
                    .iter()
                    .filter_map(|e| e.staged.map(|k| (e, k)))
                    .collect();
                let changes_l: Vec<_> = st
                    .entries
                    .iter()
                    .filter_map(|e| e.unstaged.map(|k| (e, k)))
                    .collect();

                // Branch picker + sync buttons.
                let branch_name = st.branch.clone().unwrap_or_else(|| "(detached)".into());
                let branches = self.branches.clone();
                let tool = |id: &'static str, ic: &'static str, tip: &'static str| {
                    Button::new(id)
                        .ghost()
                        .compact()
                        .icon(icon(ic))
                        .tooltip(tip)
                        .disabled(busy.is_some())
                };
                let top = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_1()
                    .py_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div().flex_1().min_w_0().child(
                                    Button::new("g-branch")
                                        .outline()
                                        .compact()
                                        .w_full()
                                        .child(
                                            div()
                                                .w_full()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(div().truncate().child(branch_name))
                                                .child(
                                                    icon("chevron-down").size_4().text_color(muted),
                                                ),
                                        )
                                        .dropdown_menu(move |mut m, _, _| {
                                            for b in &branches {
                                                m = m.menu_with_check(
                                                    b.name.clone(),
                                                    b.current,
                                                    Box::new(CheckoutBranch(b.name.clone())),
                                                );
                                            }
                                            m
                                        }),
                                ),
                            )
                            .child(tool("g-fetch", "arrow-down", "Fetch").on_click(cx.listener(
                                |this, _, w, cx| this.git_op("fetch", GitOp::Fetch, w, cx),
                            )))
                            .child(tool("g-pull", "arrow-down-to-line", "Pull").on_click(
                                cx.listener(|this, _, w, cx| {
                                    this.git_op("pull", GitOp::Pull, w, cx)
                                }),
                            ))
                            .child(tool("g-push", "arrow-up-to-line", "Push").on_click(
                                cx.listener(|this, _, w, cx| {
                                    this.git_op("push", GitOp::Push, w, cx)
                                }),
                            ))
                            .child(
                                tool("g-sync", "refresh-cw", "Sync (Pull, then Push)").on_click(
                                    cx.listener(|this, _, w, cx| {
                                        this.git_op("sync", GitOp::Sync, w, cx)
                                    }),
                                ),
                            )
                            .child(
                                Button::new("g-more")
                                    .ghost()
                                    .compact()
                                    .icon(icon("ellipsis"))
                                    .dropdown_menu(|m, _, _| {
                                        m.menu("Refresh Status", Box::new(Refresh))
                                            .separator()
                                            .menu("Fetch", Box::new(GitFetch))
                                            .menu("Pull", Box::new(GitPull))
                                            .menu("Push", Box::new(GitPush))
                                            .menu("Sync", Box::new(GitSync))
                                            .separator()
                                            .menu("Stash All Changes", Box::new(GitStashAll))
                                            .menu("View All Commits", Box::new(GitLog))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_1()
                            .text_sm()
                            .child(icon("arrow-up-down").size_4().text_color(muted))
                            .child(
                                div()
                                    .flex_none()
                                    .child(format!("{} / {}", ov.ahead, ov.behind)),
                            )
                            .child(
                                div()
                                    .id("g-log")
                                    .cursor_pointer()
                                    .text_color(accent)
                                    .hover(|d| d.underline())
                                    .child("View all commits")
                                    .on_click(cx.listener(|this, _, w, cx| {
                                        this.bottom = Bottom::Output;
                                        this.git_op("log", GitOp::Log, w, cx)
                                    })),
                            ),
                    );

                // Commit box.
                let note = busy
                    .map(|l| (format!("{l}..."), false))
                    .or_else(|| self.git_note.clone());
                let commit = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_2()
                    .child(Textarea::new(&self.commit_msg).h(px(72.)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(accent_button(
                                "g-commit",
                                commit_btn.label,
                                accent,
                                theming::readable_on(accent, theme.background, theme.foreground),
                                commit_btn.enabled && busy.is_none(),
                                cx.listener(|this, _, w, cx| this.commit_click(w, cx)),
                            ))
                            .child(
                                Checkbox::new("g-amend")
                                    .label("Amend")
                                    .checked(self.amend)
                                    .on_click({
                                        let v = v.clone();
                                        move |on, w, cx| {
                                            let on = *on;
                                            v.update(cx, |this, cx| this.set_amend(on, w, cx));
                                        }
                                    }),
                            ),
                    )
                    .when_some(note, |d, (text, is_err)| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(if is_err { danger_c } else { muted })
                                .child(text),
                        )
                    });

                let section_btn = |id: &'static str, ic: &'static str, tip: &'static str| {
                    Button::new(id)
                        .ghost()
                        .compact()
                        .icon(icon(ic))
                        .tooltip(tip)
                        .disabled(busy.is_some())
                };
                let unstage_all = section_btn("g-unstage-all", "minus", "Unstage All")
                    .on_click(cx.listener(|this, _, w, cx| {
                        this.git_op("unstage", GitOp::UnstageAll, w, cx)
                    }))
                    .into_any_element();
                let stage_all = section_btn("g-stage-all", "plus", "Stage All")
                    .on_click(
                        cx.listener(|this, _, w, cx| this.git_op("stage", GitOp::StageAll, w, cx)),
                    )
                    .into_any_element();
                let stash_all = section_btn("g-stash-all", "archive", "Stash All Changes")
                    .on_click(
                        cx.listener(|this, _, w, cx| this.git_op("stash", GitOp::StashPush, w, cx)),
                    )
                    .into_any_element();

                let stash_rows: Vec<_> = self
                    .stashes
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let (pop_name, drop_name) = (s.name.clone(), s.name.clone());
                        let (pv, dv) = (v.clone(), v.clone());
                        div()
                            .id(("stash", i))
                            .flex()
                            .items_center()
                            .pl_2()
                            .pr_1()
                            .text_sm()
                            .hover(move |d| d.bg(list_active))
                            .child(div().flex_1().min_w_0().truncate().child(format!(
                                "{} {}",
                                s.name,
                                s.message.split_once(": ").map(|x| x.1).unwrap_or(&s.message)
                            )))
                            .child(
                                Button::new(("stash-pop", i))
                                    .ghost()
                                    .compact()
                                    .label("Pop")
                                    .tooltip("Apply and remove this stash")
                                    .disabled(busy.is_some())
                                    .on_click(move |_, w, cx| {
                                        let n = pop_name.clone();
                                        pv.update(cx, |this, cx| {
                                            this.git_op("stash pop", GitOp::StashPop(n), w, cx)
                                        });
                                    }),
                            )
                            .child(
                                Button::new(("stash-drop", i))
                                    .ghost()
                                    .compact()
                                    .icon(icon("trash"))
                                    .tooltip("Delete this stash")
                                    .disabled(busy.is_some())
                                    .on_click(move |_, w, cx| {
                                        let n = drop_name.clone();
                                        dv.update(cx, |this, cx| {
                                            this.git_op("stash drop", GitOp::StashDrop(n), w, cx)
                                        });
                                    }),
                            )
                    })
                    .collect();

                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(top)
                    .child(commit)
                    .child(
                        div()
                            .id("git-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(head(
                                format!("Staged Changes ({})", staged.len()),
                                (!staged.is_empty()).then_some(unstage_all),
                            ))
                            .children(
                                staged
                                    .iter()
                                    .enumerate()
                                    .map(|(i, (e, k))| row(i, e.path.clone(), *k, true)),
                            )
                            .child(head(
                                format!("Changes ({})", changes_l.len()),
                                (!changes_l.is_empty()).then_some(stage_all),
                            ))
                            .children(
                                changes_l
                                    .iter()
                                    .enumerate()
                                    .map(|(i, (e, k))| row(10_000 + i, e.path.clone(), *k, false)),
                            )
                            .when(staged.is_empty() && changes_l.is_empty(), |d| {
                                d.child(div().p_2().text_color(muted).child("Working tree clean"))
                            })
                            .child(head(
                                format!("Stashes ({})", stash_rows.len()),
                                (ov.staged + ov.unstaged + ov.untracked > 0).then_some(stash_all),
                            ))
                            .children(stash_rows),
                    )
                    .into_any_element()
            }
        };

        let n_changes = self.git.as_ref().map(|s| s.entries.len()).unwrap_or(0);
        let (fg, bg) = (theme.foreground, theme.background);
        let right_line = if self.region == Region::Right {
            accent
        } else {
            border
        };
        let bottom_line = if self.region == Region::Bottom {
            accent
        } else {
            border
        };
        // Panel contents; each is used once per frame (a docked panel and a slid-out one are
        // never the same).
        let mut explorer_el = Some(explorer.into_any_element());
        let mut git_el = Some(git_panel);
        let mut find_el = (self.find_place == FindPlace::Right).then(|| {
            div()
                .id("find-right-scroll")
                .size_full()
                .overflow_y_scroll()
                .child(self.find_form(true, cx))
                .into_any_element()
        });
        let mut content = |p: Right| -> AnyElement {
            match p {
                Right::Explorer => explorer_el.take(),
                Right::Git => git_el.take(),
                Right::Find => find_el.take(),
            }
            .unwrap_or_else(|| div().into_any_element())
        };
        let branch_title = self.git.as_ref().ok().and_then(|s| s.branch.clone());
        let title_of = |p: Right| match (p, &branch_title) {
            (Right::Git, Some(b)) => format!("Git Changes - {b}"),
            _ => p.title().to_string(),
        };

        // Title bar: name, window position menu, pin, close.
        let header = |p: Right, docked: bool| {
            let (ix, id) = (p.ix(), docked as usize);
            div()
                .flex()
                .items_center()
                .h(px(30.))
                .pl_2()
                .pr_1()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .child(title_of(p)),
                )
                .child(
                    Button::new(("r-menu", id))
                        .ghost()
                        .compact()
                        .icon(icon("chevron-down"))
                        .dropdown_menu(move |m, _, _| {
                            m.menu_with_check("Dock", docked, Box::new(DockRight(ix)))
                                .menu_with_check("Auto Hide", !docked, Box::new(AutoHideRight(ix)))
                        }),
                )
                .child(
                    Button::new(("r-pin", id))
                        .ghost()
                        .compact()
                        .icon(icon(if docked { "pin" } else { "pin-off" }))
                        .tooltip(if docked { "Auto Hide" } else { "Keep Docked" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if docked {
                                this.autohide_right(p, cx)
                            } else {
                                this.dock_right(p, cx)
                            }
                        })),
                )
                .child(
                    Button::new(("r-close", id))
                        .ghost()
                        .compact()
                        .icon(icon("x"))
                        .tooltip(if docked {
                            "Hide (it stays on the side strip)"
                        } else {
                            "Close"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if docked {
                                this.autohide_right(p, cx);
                            }
                            this.right_fly = None;
                            cx.notify();
                        })),
                )
        };

        // The docked panels: one frame with the active panel and a tab per docked panel.
        let docked = self.right_docked();
        let dock_el = (!docked.is_empty()).then(|| {
            let cur = self.right;
            div()
                .size_full()
                .flex()
                .flex_col()
                .relative()
                .p(px(4.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.region = Region::Right;
                        cx.notify();
                    }),
                )
                .child(header(cur, true))
                .child(div().flex_1().min_h_0().child(content(cur)))
                .child(
                    div()
                        .absolute()
                        .top(px(4.))
                        .left(px(4.))
                        .right(px(4.))
                        .bottom(px(32.))
                        .border_2()
                        .border_color(right_line)
                        .rounded(px(5.)),
                )
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .px_1()
                        .h(px(30.))
                        .children(docked.iter().map(|&p| {
                            let (id, label) = match p {
                                Right::Explorer => {
                                    ("tab-explorer", "Solution Explorer".to_string())
                                }
                                Right::Git => ("tab-git", format!("Git Changes ({n_changes})")),
                                Right::Find => ("tab-find-right", "Find".to_string()),
                            };
                            panel_tab(
                                id,
                                label,
                                cur == p,
                                right_line,
                                fg,
                                muted,
                                bg,
                                cx.listener(move |this, _, _, cx| {
                                    this.right = p;
                                    cx.notify();
                                }),
                            )
                        })),
                )
        });

        // An auto-hidden panel slid out over the editor.
        let fly = self
            .right_fly
            .filter(|p| self.right_auto.contains(p) && self.right_avail().contains(p));
        let fly_el = fly.map(|p| {
            div()
                .size_full()
                .flex()
                .flex_col()
                .p(px(4.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.region = Region::Right;
                        cx.notify();
                    }),
                )
                .child(header(p, false))
                .child(div().flex_1().min_h_0().child(content(p)))
        });

        // The edge strip: one vertical tab per collapsed panel.
        let strip_panels: Vec<Right> = self
            .right_auto
            .iter()
            .copied()
            .filter(|p| self.right_avail().contains(p))
            .collect();
        let strip_active = theme.list_active;
        let strip = (!strip_panels.is_empty()).then(|| {
            div()
                .w(px(28.))
                .flex_none()
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .pt_1()
                .border_l_1()
                .border_color(border)
                .children(strip_panels.iter().map(|&p| {
                    let on = fly == Some(p);
                    let (w, h) = p.label_size();
                    div()
                        .id(("strip", p.ix()))
                        .w_full()
                        .py_2()
                        .flex()
                        .justify_center()
                        .cursor_pointer()
                        .when(on, |d| d.bg(strip_active))
                        .hover(move |d| d.bg(strip_active))
                        .child(icon(p.label_icon()).w(px(w)).h(px(h)).text_color(if on {
                            fg
                        } else {
                            muted
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.right_fly = if this.right_fly == Some(p) {
                                None
                            } else {
                                Some(p)
                            };
                            cx.notify();
                        }))
                }))
        });

        // ----------------------------------------------------------- editor
        let sc = StripCtx {
            palette: self.palette.clone(),
            proj_dirs: self.proj_dirs.clone(),
            accent,
            muted,
            border,
            active_bg: theme.list_active,
            dests: self.window_menu(None),
        };
        let tabs_row = tab_strip(&self.tabs, self.active, &sc, cx);
        let editor_area = div()
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.region = Region::Editor;
                    // An auto-hidden panel slides away as soon as the user goes back to work.
                    this.right_fly = None;
                    cx.notify();
                }),
            )
            .child(tabs_row)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(match self.active.and_then(|i| self.tabs.get(i)) {
                        Some(t) if t.diff.is_some() => diff_view(
                            t.diff.as_ref().unwrap(),
                            &self.palette,
                            theme.foreground,
                            muted,
                            border,
                            true,
                        ),
                        Some(t) => Editor::new(&t.editor)
                            .appearance(false)
                            .h_full()
                            .into_any_element(),
                        None => div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(muted)
                            .child("Open a file from the Solution Explorer (Ctrl+P to search)")
                            .into_any_element(),
                    }),
            );

        // --------------------------------------------------- Output / Tests
        let out_start = self.output.len().saturating_sub(400);
        let output_panel = div()
            .id("output-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.output_scroll)
            .px_2()
            .font_family("monospace")
            .text_xs()
            .children(
                self.output[out_start..]
                    .iter()
                    .map(|l| div().child(l.clone())),
            )
            .into_any_element();

        let sum = self.tests.summary();
        let mut cases: Vec<_> = self.tests.cases.iter().collect();
        cases.sort_by_key(|c| match c.outcome {
            Outcome::Failed | Outcome::Error => 0,
            Outcome::Skipped => 1,
            Outcome::Passed => 2,
        });
        let tests_panel = div()
            .id("tests-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(div().px_2().py_1().text_sm().child(if sum.total == 0 {
                "No JUnit XML results found in this folder".to_string()
            } else {
                format!(
                    "{} tests: {} passed, {} failed, {} errors, {} skipped  ({:.2}s)",
                    sum.total, sum.passed, sum.failed, sum.errors, sum.skipped, sum.duration_secs
                )
            }))
            .children(cases.into_iter().map(|case| {
                let (word, col) = match case.outcome {
                    Outcome::Passed => ("PASS", success_c),
                    Outcome::Failed => ("FAIL", danger_c),
                    Outcome::Error => ("ERROR", danger_c),
                    Outcome::Skipped => ("SKIP", muted),
                };
                let failed = matches!(case.outcome, Outcome::Failed | Outcome::Error);
                div()
                    .px_2()
                    .text_sm()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(div().w(px(48.)).text_color(col).child(word))
                            .child(case.full_name())
                            .child(
                                div()
                                    .text_color(muted)
                                    .child(format!("{:.3}s", case.duration_secs)),
                            ),
                    )
                    .when_some(case.message.clone(), |d, m| {
                        d.child(
                            div()
                                .pl(px(56.))
                                .text_xs()
                                .text_color(if failed { danger_c } else { muted })
                                .child(m),
                        )
                    })
            }))
            .into_any_element();

        let find_panel = if self.find_place == FindPlace::Bottom {
            div()
                .size_full()
                .flex()
                .child(
                    div()
                        .id("find-form-scroll")
                        .w(px(400.))
                        .flex_none()
                        .h_full()
                        .overflow_y_scroll()
                        .border_r_1()
                        .border_color(border)
                        .child(self.find_form(true, cx)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.find_results(cx)),
                )
                .into_any_element()
        } else {
            self.find_results(cx)
        };

        let debug_panel = self.debug_panel(cx);
        let bottom = div()
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .p(px(4.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.region = Region::Bottom;
                    cx.notify();
                }),
            )
            .child(div().flex_1().min_h_0().child(match self.bottom {
                Bottom::Output => output_panel,
                Bottom::Tests => tests_panel,
                Bottom::Find => find_panel,
                Bottom::Debug => debug_panel,
            }))
            .child(
                div()
                    .absolute()
                    .top(px(4.))
                    .left(px(4.))
                    .right(px(4.))
                    .bottom(px(32.))
                    .border_2()
                    .border_color(bottom_line)
                    .rounded(px(5.)),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .px_1()
                    .h(px(30.))
                    .child(panel_tab(
                        "tab-output",
                        "Output".into(),
                        self.bottom == Bottom::Output,
                        bottom_line,
                        fg,
                        muted,
                        bg,
                        cx.listener(|this, _, _, cx| {
                            this.bottom = Bottom::Output;
                            cx.notify();
                        }),
                    ))
                    .child(panel_tab(
                        "tab-tests",
                        format!("Test Results ({})", sum.total),
                        self.bottom == Bottom::Tests,
                        bottom_line,
                        fg,
                        muted,
                        bg,
                        cx.listener(|this, _, _, cx| {
                            this.bottom = Bottom::Tests;
                            cx.notify();
                        }),
                    ))
                    .child(panel_tab(
                        "tab-find",
                        format!(
                            "{} ({})",
                            if self.find_place == FindPlace::Bottom {
                                "Find and Replace"
                            } else {
                                "Find Results"
                            },
                            self.find.as_ref().map(|r| r.hits.len()).unwrap_or(0)
                        ),
                        self.bottom == Bottom::Find,
                        bottom_line,
                        fg,
                        muted,
                        bg,
                        cx.listener(|this, _, _, cx| {
                            this.bottom = Bottom::Find;
                            cx.notify();
                        }),
                    ))
                    .child(panel_tab(
                        "tab-debug",
                        "Debug".into(),
                        self.bottom == Bottom::Debug,
                        bottom_line,
                        fg,
                        muted,
                        bg,
                        cx.listener(|this, _, _, cx| {
                            this.bottom = Bottom::Debug;
                            cx.notify();
                        }),
                    )),
            );

        // -------------------------------------------------------- status bar
        let branch = match &self.git {
            Ok(s) => s.branch.clone().unwrap_or_default(),
            Err(_) => String::new(),
        };
        // After staging, the old side of an editable diff has to follow the index.
        let stale = self
            .active
            .and_then(|i| self.tabs.get(i))
            .and_then(|t| t.diff.as_ref())
            .and_then(|d| d.live.clone())
            .filter(|l| l.base_stale.replace(false));
        if let Some(live) = stale {
            let base = live.base.borrow().clone();
            live.left.update(cx, |e, cx| e.set_value(base, window, cx));
            live.refresh(self.dark, cx);
        }
        let lang = self
            .active
            .and_then(|i| self.tabs.get(i))
            .map(|t| language_for(&t.path).unwrap_or("plain text"))
            .unwrap_or("");
        let lsp_note = self
            .active
            .and_then(|i| self.tabs.get(i))
            .filter(|t| t.diff.is_none())
            .map(|t| self.lsp_status(&t.path))
            .unwrap_or_default();
        let proj_name = self
            .active
            .and_then(|i| self.tabs.get(i))
            .and_then(|t| project_index(&t.path, &self.proj_dirs))
            .map(|ix| {
                self.proj_dirs[ix]
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_string()
            })
            .unwrap_or_default();
        let status = div()
            .flex()
            .gap_4()
            .px_2()
            .h(px(22.))
            .items_center()
            .text_xs()
            .border_t_1()
            .border_color(border)
            .text_color(muted)
            .child(self.status_text.clone())
            .child(branch)
            .child(div().flex_1())
            .child(lsp_note)
            .child(proj_name)
            .child(self.config.name())
            .child(lang.to_string())
            .child(ws.root.display().to_string());

        // ------------------------------------------------------- quick open
        let quick = self.quick.as_ref().map(|q| {
            let v = cx.entity();
            div()
                .absolute()
                .top(px(72.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .key_context("QuickOpen")
                        .w(px(560.))
                        .p_2()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(accent)
                        .bg(popover_bg)
                        .child(Input::new(&q.input))
                        .children(q.hits.iter().enumerate().map(|(i, h)| {
                            let p = h.clone();
                            let v = v.clone();
                            div()
                                .id(("hit", i))
                                .px_2()
                                .py_1()
                                .text_sm()
                                .cursor_pointer()
                                .hover(move |d| d.bg(list_active))
                                .child(h.clone())
                                .on_click(move |_, window, cx| {
                                    let p = p.clone();
                                    v.update(cx, |this, cx| {
                                        this.close_quick(window, cx);
                                        this.open_file(&p, window, cx);
                                    });
                                })
                        })),
                )
        });

        let overlay = self.find_overlay(window, cx);
        let rename = self.rename_overlay(cx);

        // The top row: the editor, the docked panels beside it, an auto-hidden panel sliding over
        // the editor, and the edge strip that lists the collapsed panels.
        let main = match dock_el {
            Some(d) => h_resizable("top")
                .child(resizable_panel().child(editor_area))
                .child(resizable_panel().size(px(320.)).child(d))
                .into_any_element(),
            None => editor_area.into_any_element(),
        };
        let top_row = div()
            .size_full()
            .flex()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .relative()
                    .child(main)
                    .when_some(fly_el, |d, el| {
                        d.child(
                            div()
                                .absolute()
                                .top_0()
                                .right_0()
                                .bottom_0()
                                .w(px(320.))
                                .occlude()
                                .bg(theme.background)
                                .border_2()
                                .border_color(accent)
                                .rounded(px(5.))
                                .shadow_lg()
                                .child(el),
                        )
                    }),
            )
            .children(strip);

        root.child(
            div().flex_1().min_h_0().child(
                v_resizable("main")
                    .child(resizable_panel().child(top_row))
                    .child(resizable_panel().size(px(220.)).child(bottom)),
            ),
        )
        .child(status)
        .child(mouse_up_probe(cx.entity(), Kalem::tab_mouse_up))
        .children(quick)
        .children(rename)
        .children(overlay)
    }
}
