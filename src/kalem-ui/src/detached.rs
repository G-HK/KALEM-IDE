//! Editor tabs that live in their own OS window.
//!
//! A tab can be torn off (right-click "Move to New Window", or drag it out of the window) and
//! then moved to any other Kalem window the same way. The `EditorState` entity is app-wide, so
//! moving a tab keeps its text, undo history and cursor; only the owner changes. The main window
//! stays the owner of the workspace (saving, project list, theme palette); detached windows ask
//! it for those.

use crate::app::{CloseTab, Dest, Kalem, MoveTab, OpenTab, Save};
use crate::diffview::diff_view;
use crate::theming::rgba;
use gpui_kit::component::{
    input::{Editor, InputEvent},
    menu::ContextMenuExt as _,
    ActiveTheme as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kalem_theme::Palette;
use kalem_workspace::kinds::project_index;

/// Colours and window list a tab strip needs. Built once per render by the owning window.
pub(crate) struct StripCtx {
    pub palette: Palette,
    pub proj_dirs: Vec<String>,
    pub accent: Hsla,
    pub muted: Hsla,
    pub border: Hsla,
    pub active_bg: Hsla,
    /// Where a tab can be moved to from this window: (menu label, destination).
    pub dests: Vec<(String, Dest)>,
}

/// What a window with a tab strip must do when its tabs are used.
pub(crate) trait TabHost: Sized + 'static {
    fn tab_activate(&mut self, ix: usize, cx: &mut Context<Self>);
    fn tab_close(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>);
    /// The mouse went down on a tab: it may be dragged out of the window.
    fn tab_grab(&mut self, ix: usize);
}

/// One row of tabs, coloured per project, with a "Move to window" context menu.
pub(crate) fn tab_strip<T: TabHost>(
    tabs: &[OpenTab],
    active: Option<usize>,
    sc: &StripCtx,
    cx: &mut Context<T>,
) -> Div {
    let (accent, muted, border, active_bg) = (sc.accent, sc.muted, sc.border, sc.active_bg);
    div()
        .flex()
        .flex_wrap()
        .children(tabs.iter().enumerate().map(|(i, t)| {
            let name = match &t.diff {
                Some(d) => d.title(),
                None => t.path.rsplit('/').next().unwrap_or(&t.path).to_string(),
            };
            let is_active = active == Some(i);
            // Each project gets its own colour on the tab's top edge, so you can tell at a
            // glance which project a document belongs to.
            let proj_col = project_index(&t.path, &sc.proj_dirs)
                .map(|ix| rgba(sc.palette.get(&format!("tab{}", ix % 8 + 1))));
            let dests = sc.dests.clone();
            div()
                .id(("tab", i))
                .relative()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .h(px(30.))
                .cursor_pointer()
                .border_r_1()
                .border_color(border)
                .when(is_active, |d| d.bg(active_bg))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(if is_active { 4. } else { 3. }))
                        .bg(match (proj_col, is_active) {
                            (Some(c), true) => c,
                            (Some(c), false) => c.opacity(0.55),
                            (None, true) => accent,
                            (None, false) => border,
                        }),
                )
                .child(format!("{}{}", name, if t.dirty { " *" } else { "" }))
                .child(
                    div()
                        .id(("close", i))
                        .text_color(muted)
                        .child("x")
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.tab_close(i, window, cx)),
                        ),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, _| this.tab_grab(i)),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.tab_activate(i, cx)))
                .context_menu(move |mut m, _, _| {
                    for (label, dest) in &dests {
                        m = m.menu(label.clone(), Box::new(MoveTab(i, *dest)));
                    }
                    m
                })
        }))
}

/// Invisible element that reports every mouse-up of its window, even one that happens outside
/// the window while a tab is being dragged (normal element listeners only see hovered events).
pub(crate) fn mouse_up_probe<T: 'static>(
    view: Entity<T>,
    on_up: fn(&mut T, Point<Pixels>, &mut Window, &mut Context<T>),
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            let view = view.clone();
            window.on_mouse_event(move |ev: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || ev.button != MouseButton::Left {
                    return;
                }
                // Window-relative to screen coordinates, so windows can be compared.
                let screen = window.bounds().origin + ev.position;
                view.update(cx, |this, cx| on_up(this, screen, window, cx));
            });
        },
    )
    .absolute()
    .size_0()
}

/// Which window (if any) is under a screen point. `wins` is every window except the source.
pub(crate) fn dest_at(
    wins: &[(Dest, AnyWindowHandle)],
    screen: Point<Pixels>,
    cx: &mut App,
) -> Option<Dest> {
    wins.iter().find_map(|(dest, handle)| {
        let b = handle.update(cx, |_, w, _| w.bounds()).ok()?;
        b.contains(&screen).then_some(*dest)
    })
}

/// A window holding torn-off tabs.
pub(crate) struct Detached {
    pub id: u64,
    main: WeakEntity<Kalem>,
    tabs: Vec<OpenTab>,
    active: Option<usize>,
    focus: FocusHandle,
    tab_drag: Option<usize>,
    message: Option<String>,
    _subs: Vec<Subscription>,
}

impl Detached {
    pub fn new(
        id: u64,
        main: Entity<Kalem>,
        tab: OpenTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Follow the main window's redraws (theme, project list, window list).
        let observe = cx.observe(&main, |_, _, cx| cx.notify());
        // Closing this window hands its tabs back to the main window instead of losing edits.
        let release = cx.on_release(move |this, cx| {
            let tabs = std::mem::take(&mut this.tabs);
            let id = this.id;
            if let Some(main) = this.main.upgrade() {
                main.update(cx, |k, cx| {
                    k.forget_window(id);
                    for t in tabs {
                        k.adopt(t, cx);
                    }
                });
            }
        });
        let mut me = Self {
            id,
            main: main.downgrade(),
            tabs: Vec::new(),
            active: None,
            focus: cx.focus_handle(),
            tab_drag: None,
            message: None,
            _subs: vec![observe, release],
        };
        me.adopt(tab, cx);
        window.focus(&me.focus, cx);
        me
    }

    /// Take ownership of a tab (its dirty tracking is re-attached here).
    pub fn adopt(&mut self, mut tab: OpenTab, cx: &mut Context<Self>) {
        tab._sub = cx.subscribe(&tab.editor, |this, editor, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                let now = editor.read(cx).value();
                if let Some(t) = this.tabs.iter_mut().find(|t| t.editor == editor) {
                    t.dirty = now != t.saved;
                    cx.notify();
                }
            }
        });
        self.tabs.push(tab);
        self.active = Some(self.tabs.len() - 1);
        cx.notify();
    }

    /// If this window has `path` open, show that tab and say so.
    pub fn show_path(&mut self, path: &str, cx: &mut Context<Self>) -> bool {
        match self
            .tabs
            .iter()
            .position(|t| t.diff.is_none() && t.path == path)
        {
            Some(ix) => {
                self.active = Some(ix);
                cx.notify();
                true
            }
            None => false,
        }
    }

    fn remove_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> OpenTab {
        let tab = self.tabs.remove(ix);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(a) if a > ix => Some(a - 1),
            Some(a) => Some(a.min(self.tabs.len() - 1)),
            None => None,
        };
        if self.tabs.is_empty() {
            // Nothing left to show; the (now empty) window goes away.
            window.remove_window();
        }
        cx.notify();
        tab
    }

    fn move_tab(
        &mut self,
        ix: usize,
        dest: Dest,
        at: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ix >= self.tabs.len() {
            return;
        }
        let Some(main) = self.main.upgrade() else {
            return;
        };
        let tab = self.remove_tab(ix, window, cx);
        main.update(cx, |k, cx| k.deliver(tab, dest, at, cx));
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let (Some(main), Some(ix)) = (self.main.upgrade(), self.active) else {
            return;
        };
        let Some(tab) = self.tabs.get(ix) else { return };
        let (path, text) = (tab.path.clone(), tab.editor.read(cx).value());
        match main.update(cx, |k, cx| k.write_doc(&path, &text, cx)) {
            Ok(()) => {
                if let Some(t) = self.tabs.get_mut(ix) {
                    t.saved = text;
                    t.dirty = false;
                }
                self.message = None;
            }
            Err(e) => self.message = Some(e),
        }
        cx.notify();
    }

    /// A mouse-up anywhere (also outside the window): drop a dragged tab on whatever is there.
    fn mouse_up(&mut self, screen: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tab_drag.take() else {
            return;
        };
        if window.bounds().contains(&screen) {
            return; // a click, or a drag that stayed inside
        }
        let Some(main) = self.main.upgrade() else {
            return;
        };
        let wins: Vec<_> = main
            .read(cx)
            .windows_snapshot()
            .into_iter()
            .filter(|(d, _)| *d != Dest::Window(self.id))
            .collect();
        let dest = dest_at(&wins, screen, cx).unwrap_or(Dest::New);
        self.move_tab(ix, dest, Some(screen), window, cx);
    }
}

impl TabHost for Detached {
    fn tab_activate(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.active = Some(ix);
        cx.notify();
    }

    fn tab_close(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            let tab = self.remove_tab(ix, window, cx);
            if tab.diff.is_none() {
                if let Some(main) = self.main.upgrade() {
                    main.update(cx, |k, cx| k.lsp_forget(&tab.path, cx));
                }
            }
        }
    }

    fn tab_grab(&mut self, ix: usize) {
        self.tab_drag = Some(ix);
    }
}

impl Render for Detached {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (palette, proj_dirs, dests, project) = match self.main.upgrade() {
            Some(m) => {
                let k = m.read(cx);
                let project = self
                    .active
                    .and_then(|i| self.tabs.get(i))
                    .and_then(|t| k.project_name_of(&t.path));
                (
                    k.palette().clone(),
                    k.proj_dirs().to_vec(),
                    k.window_menu(Some(self.id)),
                    project,
                )
            }
            None => (Palette::default(), Vec::new(), Vec::new(), None),
        };
        let palette2 = palette.clone();
        let sc = StripCtx {
            accent: rgba(palette.get("accent")),
            palette,
            proj_dirs,
            muted: theme.muted_foreground,
            border: theme.border,
            active_bg: theme.list_active,
            dests,
        };
        let active = self.active.and_then(|i| self.tabs.get(i));
        window.set_window_title(&match active {
            Some(t) => format!(
                "{} - Kalem IDE",
                t.path.rsplit('/').next().unwrap_or(&t.path)
            ),
            None => "Kalem IDE".to_string(),
        });
        let view = cx.entity();
        div()
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(theme.background)
            .text_color(theme.foreground)
            .track_focus(&self.focus)
            .key_context("Kalem")
            .on_action(cx.listener(|this, _: &Save, _, cx| this.save(cx)))
            .on_action(cx.listener(|this, _: &CloseTab, w, cx| {
                if let Some(ix) = this.active {
                    this.tab_close(ix, w, cx);
                }
            }))
            .on_action(cx.listener(|this, a: &MoveTab, w, cx| this.move_tab(a.0, a.1, None, w, cx)))
            .child(mouse_up_probe(view, Self::mouse_up))
            .child(tab_strip(&self.tabs, self.active, &sc, cx))
            .child(
                div().flex_1().min_h_0().child(match active {
                    Some(t) if t.diff.is_some() => diff_view(
                        t.diff.as_ref().unwrap(),
                        &palette2,
                        theme.foreground,
                        sc.muted,
                        sc.border,
                    ),
                    Some(t) => Editor::new(&t.editor)
                        .appearance(false)
                        .h_full()
                        .into_any_element(),
                    None => div().size_full().into_any_element(),
                }),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .px_2()
                    .h(px(22.))
                    .items_center()
                    .text_xs()
                    .border_t_1()
                    .border_color(sc.border)
                    .text_color(sc.muted)
                    .child(active.map(|t| t.path.clone()).unwrap_or_default())
                    .child(div().flex_1())
                    .child(project.unwrap_or_default())
                    .child(self.message.clone().unwrap_or_default()),
            )
    }
}
