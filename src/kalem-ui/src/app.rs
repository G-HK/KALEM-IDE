use crate::icons::icon;
use crate::theming::{self, rgba};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    input::{self, Editor, EditorState, Input, InputEvent, InputState},
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
use kalem_vcs::{ChangeKind, Status};
use kalem_workspace::kinds::{badge_for, is_image, project_kind, ProjectKind};
use kalem_workspace::tasks::{spawn, Config, RunningTask, TaskEvent, TaskKind};
use kalem_workspace::{language_for, quick_open, Workspace};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

actions!(
    kalem,
    [
        Save,
        OpenFolder,
        CloseTab,
        Quit,
        ToggleSync,
        Refresh,
        ShowExplorer,
        ShowGit,
        ShowOutput,
        ShowTests,
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

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-s", Save, None),
        KeyBinding::new("secondary-o", OpenFolder, None),
        KeyBinding::new("secondary-w", CloseTab, None),
        KeyBinding::new("secondary-shift-b", Build, None),
        KeyBinding::new("ctrl-f5", RunNoDebug, None),
        KeyBinding::new("secondary-p", QuickOpen, None),
        KeyBinding::new("alt-left", NavBack, None),
        KeyBinding::new("alt-right", NavForward, None),
        KeyBinding::new("f5", Refresh, None),
        KeyBinding::new("escape", CloseQuick, Some("QuickOpen")),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

struct OpenTab {
    path: String,
    editor: Entity<EditorState>,
    saved: SharedString,
    dirty: bool,
    _sub: Subscription,
}

#[derive(Clone, Copy, PartialEq)]
enum Right {
    Explorer,
    Git,
}

#[derive(Clone, Copy, PartialEq)]
enum Bottom {
    Output,
    Tests,
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
    bottom: Bottom,
    region: Region,
    git: Result<Status, String>,
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
}

const MAX_OUTPUT: usize = 5000;

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
        let mut me = Self {
            ws: None,
            tree,
            tabs: Vec::new(),
            active: None,
            sync_active: true,
            right: Right::Explorer,
            bottom: Bottom::Output,
            region: Region::Editor,
            git: Err("No folder open".into()),
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
        cx.notify();
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
        if let Some(ix) = self.tabs.iter().position(|t| t.path == rel) {
            self.activate(ix, cx);
            return;
        }
        let Some(ws) = &self.ws else { return };
        let text = match ws.read_file(rel) {
            Ok(t) => t,
            Err(e) => {
                self.message = Some(e);
                cx.notify();
                return;
            }
        };
        let lang = language_for(rel);
        let init = text.clone();
        let editor = cx.new(|cx| {
            let mut s = EditorState::new(window, cx);
            if let Some(l) = lang {
                s = s.language(l);
            }
            s.line_number(true).default_value(init)
        });
        let saved: SharedString = text.into();
        let sub = cx.subscribe(&editor, |this, editor, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                let now = editor.read(cx).value();
                if let Some(t) = this.tabs.iter_mut().find(|t| t.editor == editor) {
                    t.dirty = now != t.saved;
                    cx.notify();
                }
            }
        });
        self.tabs.push(OpenTab {
            path: rel.to_string(),
            editor,
            saved,
            dirty: false,
            _sub: sub,
        });
        self.message = None;
        self.activate(self.tabs.len() - 1, cx);
    }

    fn activate(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.active = Some(ix);
        self.region = Region::Editor;
        if !self.navigating {
            if let Some(t) = self.tabs.get(ix) {
                if self.nav.get(self.nav_pos) != Some(&t.path) {
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
        if ix >= self.tabs.len() {
            return;
        }
        self.tabs.remove(ix);
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
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let (Some(ws), Some(ix)) = (&self.ws, self.active) else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(ix) else {
            return;
        };
        let text = tab.editor.read(cx).value();
        match ws.write_file(&tab.path, &text) {
            Ok(()) => {
                tab.saved = text;
                tab.dirty = false;
                self.message = None;
                self.refresh(cx);
            }
            Err(e) => self.message = Some(e),
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
            Ok(plan) => {
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
        for _ in 0..800 {
            match state.task.rx.try_recv() {
                Ok(TaskEvent::Line(l)) => self.output.push(l),
                Ok(TaskEvent::Done(code)) => {
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
    bg_inactive: Hsla,
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
        .bg(if active { bg_active } else { bg_inactive })
        .text_color(if active { fg } else { muted })
        .child(label)
        .on_click(on_click)
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
                    .menu_with_disabled(
                        "Start Debugging (not implemented yet)",
                        Box::new(RunNoDebug),
                        true,
                    )
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
                    .tooltip("Undo")
                    .disabled(self.active.is_none())
                    .on_click(|_, w, cx| w.dispatch_action(Box::new(input::Undo), cx)),
            )
            .child(
                Button::new("redo")
                    .ghost()
                    .compact()
                    .icon(icon("redo-2"))
                    .tooltip("Redo")
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
                    .icon(icon("bug"))
                    .tooltip("Debugger: not implemented yet")
                    .disabled(true),
            )
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
            .on_action(cx.listener(|this, _: &Save, _, cx| this.save(cx)))
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
                this.right = Right::Explorer;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowGit, _, cx| {
                this.right = Right::Git;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowOutput, _, cx| {
                this.bottom = Bottom::Output;
                cx.notify();
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
        let git_panel = match &self.git {
            Err(e) => div()
                .p_2()
                .text_color(muted)
                .child(e.clone())
                .into_any_element(),
            Ok(st) => {
                let colour = |class: u8| match class {
                    0 => warning_c,
                    1 => success_c,
                    2 => danger_c,
                    _ => muted,
                };
                let v = cx.entity();
                let row = move |id: usize, path: String, k: ChangeKind| {
                    let (letter, class) = badge(k);
                    let p = path.clone();
                    let v = v.clone();
                    div()
                        .id(("chg", id))
                        .flex()
                        .gap_2()
                        .px_2()
                        .text_sm()
                        .cursor_pointer()
                        .hover(move |d| d.bg(list_active))
                        .child(div().w(px(14.)).text_color(colour(class)).child(letter))
                        .child(path)
                        .on_click(move |_, window, cx| {
                            let p = p.clone();
                            v.update(cx, |this, cx| this.open_file(&p, window, cx));
                        })
                };
                let head = |t: String| {
                    div()
                        .px_2()
                        .pt_1()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .child(t)
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
                div()
                    .id("git-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .child(div().px_2().py_1().text_sm().child(format!(
                        "{}{}  ahead {}  behind {}",
                        st.branch.clone().unwrap_or_else(|| "(detached)".into()),
                        st.upstream.as_ref().map(|u| format!(" -> {u}")).unwrap_or_default(),
                        st.ahead,
                        st.behind
                    )))
                    .child(head(format!("Staged ({})", staged.len())))
                    .children(
                        staged
                            .iter()
                            .enumerate()
                            .map(|(i, (e, k))| row(i, e.path.clone(), *k)),
                    )
                    .child(head(format!("Changes ({})", changes_l.len())))
                    .children(
                        changes_l
                            .iter()
                            .enumerate()
                            .map(|(i, (e, k))| row(10_000 + i, e.path.clone(), *k)),
                    )
                    .when(staged.is_empty() && changes_l.is_empty(), |d| {
                        d.child(div().p_2().text_color(muted).child("Working tree clean"))
                    })
                    .into_any_element()
            }
        };

        let n_changes = self.git.as_ref().map(|s| s.entries.len()).unwrap_or(0);
        let (fg, bg, panel_bg) = (theme.foreground, theme.background, theme.sidebar);
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
        let right_dock = div()
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
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .child(if self.right == Right::Explorer {
                        "Solution Explorer"
                    } else {
                        "Git Changes"
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(if self.right == Right::Explorer {
                        explorer.into_any_element()
                    } else {
                        git_panel
                    }),
            )
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
                    .child(panel_tab(
                        "tab-explorer",
                        "Solution Explorer".into(),
                        self.right == Right::Explorer,
                        right_line,
                        fg,
                        muted,
                        bg,
                        panel_bg,
                        cx.listener(|this, _, _, cx| {
                            this.right = Right::Explorer;
                            cx.notify();
                        }),
                    ))
                    .child(panel_tab(
                        "tab-git",
                        format!("Git Changes ({n_changes})"),
                        self.right == Right::Git,
                        right_line,
                        fg,
                        muted,
                        bg,
                        panel_bg,
                        cx.listener(|this, _, _, cx| {
                            this.right = Right::Git;
                            cx.notify();
                        }),
                    )),
            );

        // ----------------------------------------------------------- editor
        let tabs_row = div()
            .flex()
            .flex_wrap()
            .children(self.tabs.iter().enumerate().map(|(i, t)| {
                let name = t.path.rsplit('/').next().unwrap_or(&t.path).to_string();
                let active = self.active == Some(i);
                div()
                    .id(("tab", i))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .h(px(30.))
                    .cursor_pointer()
                    .border_r_1()
                    .border_t_2()
                    .border_color(border)
                    .when(active, |d| {
                        d.border_t_2().border_color(accent).bg(theme.list_active)
                    })
                    .child(format!("{}{}", name, if t.dirty { " *" } else { "" }))
                    .child(
                        div()
                            .id(("close", i))
                            .text_color(muted)
                            .child("x")
                            .on_click(cx.listener(move |this, _, _, cx| this.close_tab(i, cx))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.activate(i, cx)))
            }));
        let editor_area = div()
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.region = Region::Editor;
                    cx.notify();
                }),
            )
            .child(tabs_row)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(match self.active.and_then(|i| self.tabs.get(i)) {
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
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(if self.bottom == Bottom::Output {
                        output_panel
                    } else {
                        tests_panel
                    }),
            )
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
                        panel_bg,
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
                        panel_bg,
                        cx.listener(|this, _, _, cx| {
                            this.bottom = Bottom::Tests;
                            cx.notify();
                        }),
                    )),
            );

        // -------------------------------------------------------- status bar
        let branch = match &self.git {
            Ok(s) => s.branch.clone().unwrap_or_default(),
            Err(_) => String::new(),
        };
        let lang = self
            .active
            .and_then(|i| self.tabs.get(i))
            .map(|t| language_for(&t.path).unwrap_or("plain text"))
            .unwrap_or("");
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

        let _ = window;
        root.child(
            div().flex_1().min_h_0().child(
                v_resizable("main")
                    .child(
                        resizable_panel().child(
                            h_resizable("top")
                                .child(resizable_panel().child(editor_area))
                                .child(resizable_panel().size(px(320.)).child(right_dock)),
                        ),
                    )
                    .child(resizable_panel().size(px(220.)).child(bottom)),
            ),
        )
        .child(status)
        .children(quick)
    }
}
