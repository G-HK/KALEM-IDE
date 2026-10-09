//! UI spike: egui + egui_dock.
//!
//! The same screen as the GPUI spike: Solution Explorer (with "sync with active document"),
//! editor tabs with tree-sitter highlighting, Git Changes panel, Test Results panel.
//! All content comes from the real Kalem crates through `kalem-spike-data`.

use eframe::egui::{self, Color32, FontId, RichText, TextEdit, Ui};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use kalem_core::explorer::Node;
use kalem_spike_data::{highlight, Demo, Span, Token};
use kalem_test::Outcome;
use kalem_vcs::ChangeKind;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

// Visual Studio dark-ish palette.
const BG: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
const PANEL: Color32 = Color32::from_rgb(0x25, 0x25, 0x26);
const BORDER: Color32 = Color32::from_rgb(0x3c, 0x3c, 0x3c);
const TEXT: Color32 = Color32::from_rgb(0xd4, 0xd4, 0xd4);
const DIM: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);
const ACCENT: Color32 = Color32::from_rgb(0x00, 0x7a, 0xcc);
const GREEN: Color32 = Color32::from_rgb(0x73, 0xc9, 0x91);
const RED: Color32 = Color32::from_rgb(0xf1, 0x4c, 0x4c);
const YELLOW: Color32 = Color32::from_rgb(0xe2, 0xc0, 0x8d);

fn token_color(t: Token) -> Color32 {
    match t {
        Token::Plain => TEXT,
        Token::Keyword => Color32::from_rgb(0x56, 0x9c, 0xd6),
        Token::String => Color32::from_rgb(0xce, 0x91, 0x78),
        Token::Comment => Color32::from_rgb(0x6a, 0x99, 0x55),
        Token::Number => Color32::from_rgb(0xb5, 0xce, 0xa8),
        Token::Type => Color32::from_rgb(0x4e, 0xc9, 0xb0),
        Token::Function => Color32::from_rgb(0xdc, 0xdc, 0xaa),
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Tab {
    Explorer,
    Git,
    Tests,
    Output,
    File(String),
}

enum Action {
    Open(String),
    SyncActive,
}

struct State {
    demo: Demo,
    texts: HashMap<String, String>,
    spans: HashMap<String, (u64, Vec<Span>)>,
    selected: String,
    active: Option<String>,
    commit_msg: String,
    amend: bool,
    pending: Vec<Action>,
    editor_node: NodeIndex,
}

fn hash_str(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

struct App {
    dock: DockState<Tab>,
    st: State,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut v = egui::Visuals::dark();
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = BG;
        v.faint_bg_color = Color32::from_rgb(0x2a, 0x2a, 0x2b);
        v.selection.bg_fill = Color32::from_rgb(0x09, 0x4a, 0x78);
        v.widgets.noninteractive.bg_stroke.color = BORDER;
        v.override_text_color = Some(TEXT);
        cc.egui_ctx.set_visuals(v);

        let demo = kalem_spike_data::demo();
        let texts: HashMap<String, String> = demo
            .files
            .iter()
            .map(|f| (f.path.to_string(), f.text.to_string()))
            .collect();
        let first = demo.files[0].path.to_string();

        let mut dock = DockState::new(vec![Tab::File(first.clone())]);
        let surface = dock.main_surface_mut();
        let [editor, _left] = surface.split_left(NodeIndex::root(), 0.22, vec![Tab::Explorer]);
        let [editor, _right] = surface.split_right(editor, 0.74, vec![Tab::Git]);
        let [editor, _bottom] = surface.split_below(
            editor,
            0.68,
            vec![Tab::Tests, Tab::Output],
        );
        Self {
            dock,
            st: State {
                demo,
                texts,
                spans: HashMap::new(),
                selected: first.clone(),
                active: Some(first),
                commit_msg: String::new(),
                amend: false,
                pending: vec![Action::SyncActive],
                editor_node: editor,
            },
        }
    }

    fn apply(&mut self) {
        let actions: Vec<Action> = self.st.pending.drain(..).collect();
        for a in actions {
            match a {
                Action::Open(path) => {
                    self.st.selected = path.clone();
                    let tab = Tab::File(path.clone());
                    if let Some(found) = self.dock.find_tab(&tab) {
                        let _ = self.dock.set_active_tab(found);
                    } else if self.st.texts.contains_key(&path) {
                        let surface = self.dock.main_surface_mut();
                        surface.set_focused_node(self.st.editor_node);
                        self.dock.push_to_focused_leaf(tab);
                    }
                }
                Action::SyncActive => {
                    if let Some(p) = self.st.active.clone() {
                        self.st.selected = p;
                    }
                    // Folder expansion happens in the explorer drawing code (see `sync_open`).
                    self.st.pending.push(Action::Open(String::new()));
                    self.st.pending.pop();
                }
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.apply();

        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                for m in ["File", "Edit", "View", "Git", "Project", "Debug", "Test", "Tools", "Extensions", "Window", "Help"] {
                    ui.menu_button(m, |ui| {
                        ui.label(RichText::new("(spike: menu not wired)").color(DIM));
                    });
                }
                ui.separator();
                ui.label(RichText::new(format!("  {}", self.st.demo.solution_name)).color(DIM));
            });
        });

        egui::Panel::bottom("status")
            .exact_size(22.0)
            .frame(egui::Frame::new().fill(ACCENT).inner_margin(egui::Margin::symmetric(8, 2)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let g = &self.st.demo.git;
                    let white = Color32::WHITE;
                    ui.label(RichText::new(format!(
                        "git: {}   {} ahead, {} behind",
                        g.branch.as_deref().unwrap_or("-"),
                        g.ahead,
                        g.behind
                    )).color(white));
                    ui.label(RichText::new(self.st.demo.solution_overview.clone()).color(white));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(self.st.active.clone().unwrap_or_default()).color(white));
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG))
            .show(ui, |ui| {
                let mut style = Style::from_egui(ui.style().as_ref());
                style.tab_bar.bg_fill = PANEL;
                style.tab.active.bg_fill = BG;
                style.tab.active.text_color = Color32::WHITE;
                style.tab.inactive.bg_fill = PANEL;
                style.tab.inactive.text_color = DIM;
                style.tab.focused.bg_fill = BG;
                style.tab.focused.text_color = Color32::WHITE;
                style.separator.color_idle = BORDER;
                style.tab.hline_below_active_tab_name = true;
                let mut viewer = Viewer { st: &mut self.st };
                DockArea::new(&mut self.dock)
                    .style(style)
                    .show_close_buttons(true)
                    .show_inside(ui, &mut viewer);
            });

        // Track which file tab is active, for "sync with active document" and the status bar.
        if let Some((_, Tab::File(p))) = self.dock.find_active_focused() {
            let p = p.clone();
            if self.st.active.as_deref() != Some(p.as_str()) {
                self.st.active = Some(p);
            }
        }
    }
}

struct Viewer<'a> {
    st: &'a mut State,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        match tab {
            Tab::File(p) => egui::Id::new(("file", p.clone())),
            Tab::Explorer => egui::Id::new("explorer"),
            Tab::Git => egui::Id::new("git"),
            Tab::Tests => egui::Id::new("tests"),
            Tab::Output => egui::Id::new("output"),
        }
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab {
            Tab::Explorer => "Solution Explorer".into(),
            Tab::Git => "Git Changes".into(),
            Tab::Tests => "Test Results".into(),
            Tab::Output => "Output".into(),
            Tab::File(p) => p.rsplit('/').next().unwrap_or(p).to_string().into(),
        }
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        matches!(tab, Tab::File(_))
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::Explorer => explorer_ui(ui, self.st),
            Tab::Git => git_ui(ui, self.st),
            Tab::Tests => tests_ui(ui, self.st),
            Tab::Output => {
                ui.label(RichText::new("Kalem IDE spike (egui + egui_dock)\nBuild succeeded.").monospace().color(DIM));
            }
            Tab::File(p) => editor_ui(ui, self.st, p),
        }
    }
}

// ---- Solution Explorer ----------------------------------------------------------------------

fn explorer_ui(ui: &mut Ui, st: &mut State) {
    let stats = st.demo.explorer.stats();
    ui.label(RichText::new(format!(
        "Solution '{}' - {} projects",
        st.demo.solution_name, st.demo.project_count
    )).strong());
    ui.label(RichText::new(format!("{} folders, {} files", stats.folders, stats.files)).color(DIM));
    ui.horizontal(|ui| {
        if ui.button("Sync with active document").clicked() {
            sync_open(ui.ctx(), st);
        }
    });
    ui.separator();
    // Reveal state is applied before drawing, so it takes effect this frame.
    let root = st.demo.explorer.root.clone();
    egui::ScrollArea::both().auto_shrink([false; 2]).show(ui, |ui| {
        for child in &root.children {
            node_ui(ui, child, st);
        }
    });
}

/// Expand every folder above the active file and select it.
fn sync_open(ctx: &egui::Context, st: &mut State) {
    let Some(active) = st.active.clone() else { return };
    if let Some(plan) = st.demo.explorer.reveal(&active) {
        for dir in plan.expand {
            let id = egui::Id::new(("dir", dir));
            let mut s = egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, false);
            s.set_open(true);
            s.store(ctx);
        }
        st.selected = plan.select;
    }
}

fn node_ui(ui: &mut Ui, node: &Node, st: &mut State) {
    if node.is_dir {
        let id = egui::Id::new(("dir", node.path.clone()));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
            .show_header(ui, |ui| {
                ui.label(RichText::new(node.name.clone()));
            })
            .body(|ui| {
                for c in &node.children {
                    node_ui(ui, c, st);
                }
            });
    } else {
        let selected = st.selected == node.path;
        let resp = ui.selectable_label(selected, format!("      {}", node.name));
        if selected && !resp.clicked() {
            resp.scroll_to_me(None);
        }
        if resp.clicked() {
            st.pending.push(Action::Open(node.path.clone()));
        }
    }
}

// ---- Git Changes ----------------------------------------------------------------------------

fn kind_letter(k: &ChangeKind) -> (&'static str, Color32) {
    match k {
        ChangeKind::Added => ("A", GREEN),
        ChangeKind::Modified => ("M", YELLOW),
        ChangeKind::Deleted => ("D", RED),
        ChangeKind::Renamed => ("R", GREEN),
        ChangeKind::Untracked => ("U", GREEN),
        ChangeKind::Conflicted => ("!", RED),
        _ => ("?", DIM),
    }
}

fn git_ui(ui: &mut Ui, st: &mut State) {
    let g = st.demo.git.clone();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("branch: {}", g.branch.as_deref().unwrap_or("-"))).strong());
        ui.label(RichText::new(format!("{} ahead, {} behind", g.ahead, g.behind)).color(DIM));
    });
    ui.horizontal(|ui| {
        for b in ["Fetch", "Pull", "Push"] {
            let _ = ui.button(b);
        }
        let _ = ui.button(format!("Sync ({:?})", g.sync_action()));
    });
    ui.add_space(4.0);
    ui.add(
        TextEdit::multiline(&mut st.commit_msg)
            .hint_text("Enter a commit message")
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
    ui.horizontal(|ui| {
        let cb = g.commit_button(&st.commit_msg, st.amend);
        ui.add_enabled(cb.enabled, egui::Button::new(cb.label)).on_disabled_hover_text(cb.reason.unwrap_or(""));
        ui.checkbox(&mut st.amend, "Amend");
    });
    ui.separator();
    let status = &st.demo.git_status;
    let staged: Vec<_> = status.entries.iter().filter(|e| e.staged.is_some()).collect();
    let changes: Vec<_> = status.entries.iter().filter(|e| e.unstaged.is_some()).collect();
    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        egui::CollapsingHeader::new(format!("Staged Changes ({})", staged.len()))
            .default_open(true)
            .show(ui, |ui| {
                for e in &staged {
                    change_row(ui, &e.path, e.staged.as_ref());
                }
            });
        egui::CollapsingHeader::new(format!("Changes ({})", changes.len()))
            .default_open(true)
            .show(ui, |ui| {
                for e in &changes {
                    change_row(ui, &e.path, e.unstaged.as_ref());
                }
            });
        egui::CollapsingHeader::new("Related Items").show(ui, |ui| {
            ui.label(RichText::new("No linked issues or pull requests").color(DIM));
        });
        egui::CollapsingHeader::new("Stashes (0)").show(ui, |_| {});
    });
}

fn change_row(ui: &mut Ui, path: &str, kind: Option<&ChangeKind>) {
    ui.horizontal(|ui| {
        let name = path.rsplit('/').next().unwrap_or(path);
        ui.label(name);
        ui.label(RichText::new(path.trim_end_matches(name)).color(DIM).small());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(k) = kind {
                let (l, c) = kind_letter(k);
                ui.label(RichText::new(l).color(c).strong());
            }
        });
    });
}

// ---- Test Results ---------------------------------------------------------------------------

fn tests_ui(ui: &mut Ui, st: &mut State) {
    let run = &st.demo.tests;
    let s = run.summary();
    ui.horizontal(|ui| {
        let (txt, col) = if s.is_green() { ("PASSED", GREEN) } else { ("FAILED", RED) };
        ui.label(RichText::new(txt).color(col).strong());
        ui.label(format!(
            "{} tests: {} passed, {} failed, {} errors, {} skipped  ({:.2}s)",
            s.total, s.passed, s.failed, s.errors, s.skipped, s.duration_secs
        ));
    });
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        for (suite, sum) in run.by_suite() {
            let col = if sum.is_green() { GREEN } else { RED };
            egui::CollapsingHeader::new(RichText::new(format!("{suite}  ({}/{})", sum.passed, sum.total)).color(col))
                .default_open(true)
                .show(ui, |ui| {
                    for c in run.cases.iter().filter(|c| c.suite == suite) {
                        ui.horizontal(|ui| {
                            let (t, col) = match c.outcome {
                                Outcome::Passed => ("PASS", GREEN),
                                Outcome::Failed => ("FAIL", RED),
                                Outcome::Error => ("ERR ", RED),
                                Outcome::Skipped => ("SKIP", DIM),
                            };
                            ui.label(RichText::new(t).monospace().color(col));
                            ui.label(c.full_name());
                            ui.label(RichText::new(format!("{:.2}s", c.duration_secs)).color(DIM));
                            if let Some(m) = &c.message {
                                ui.label(RichText::new(m).color(RED));
                            }
                        });
                    }
                });
        }
    });
}

// ---- Editor ---------------------------------------------------------------------------------

fn editor_ui(ui: &mut Ui, st: &mut State, path: &str) {
    let Some(text) = st.texts.get_mut(path) else {
        ui.label("file not available in the spike");
        return;
    };
    let h = hash_str(text);
    let entry = st.spans.entry(path.to_string()).or_insert_with(|| (0, Vec::new()));
    if entry.0 != h {
        *entry = (h, highlight::rust(text));
    }
    let spans = entry.1.clone();
    let font = FontId::monospace(13.0);
    let lines = text.lines().count().max(1);
    let gutter = (0..lines).map(|i| format!("{:>4}", i + 1)).collect::<Vec<_>>().join("\n");

    egui::ScrollArea::both().auto_shrink([false; 2]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.label(RichText::new(gutter).font(font.clone()).color(Color32::from_rgb(0x85, 0x85, 0x85)));
            let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                let s = buf.as_str();
                let mut job = egui::text::LayoutJob::default();
                let mut pos = 0usize;
                for (r, t) in &spans {
                    if r.start < pos || r.end > s.len() || !s.is_char_boundary(r.start) || !s.is_char_boundary(r.end) {
                        continue;
                    }
                    if r.start > pos {
                        job.append(&s[pos..r.start], 0.0, egui::TextFormat::simple(font.clone(), token_color(Token::Plain)));
                    }
                    job.append(&s[r.clone()], 0.0, egui::TextFormat::simple(font.clone(), token_color(*t)));
                    pos = r.end;
                }
                if pos < s.len() && s.is_char_boundary(pos) {
                    job.append(&s[pos..], 0.0, egui::TextFormat::simple(font.clone(), token_color(Token::Plain)));
                }
                ui.fonts_mut(|f| f.layout_job(job))
            };
            ui.add(
                TextEdit::multiline(text)
                    .font(FontId::monospace(13.0))
                    .code_editor()
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .layouter(&mut layouter),
            );
        });
    });
}

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1500.0, 920.0]).with_title("Kalem IDE - egui spike"),
        ..Default::default()
    };
    eframe::run_native("Kalem IDE - egui spike", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
