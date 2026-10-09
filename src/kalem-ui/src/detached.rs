//! Editor tabs that live in their own OS window.
//!
//! A tab can be torn off (right-click "Move to New Window", or drag it out of the window) and
//! then moved to any other Kalem window the same way. The `EditorState` entity is app-wide, so
//! moving a tab keeps its text, undo history and cursor; only the owner changes. The main window
//! stays the owner of the workspace (saving, project list, theme palette); detached windows ask
//! it for those.

use crate::app::{CloseTab, Dest, Kalem, MoveTab, OpenTab, Save, TabCmd};
use crate::diffview::diff_view;
use crate::icons::icon;
use crate::theming::rgba;
use gpui_kit::component::{
    input::{Editor, InputEvent},
    menu::{ContextMenuExt as _, PopupMenu},
    ActiveTheme as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kalem_theme::Palette;
use kalem_workspace::kinds::project_index;
use std::collections::HashMap;

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
    /// The window this strip belongs to.
    pub me: Dest,
    /// A closed tab can be reopened.
    pub can_restore: bool,
}

impl StripCtx {
    fn me_id(&self) -> usize {
        match self.me {
            Dest::Window(id) => id as usize + 1,
            _ => 0,
        }
    }
}

/// What a tab menu entry does.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabOp {
    Save,
    Close,
    CloseOthers,
    CloseAll,
    Restore,
    ToNewWindow,
    AllToNewWindow,
    Pin,
    Color(Option<u8>),
    Locate,
    CopyPath,
    Reveal,
}

/// The tabs a "close others" (keeping `keep`) or "close all" closes, last first. Pinned tabs stay.
pub(crate) fn closable(tabs: &[OpenTab], keep: Option<usize>) -> Vec<usize> {
    (0..tabs.len())
        .rev()
        .filter(|&i| Some(i) != keep && !tabs[i].pinned)
        .collect()
}

/// Pin or unpin a tab: pinned tabs gather at the left. `active` follows its tab.
pub(crate) fn toggle_pin(tabs: &mut Vec<OpenTab>, ix: usize, active: &mut Option<usize>) {
    let pinned = tabs.iter().filter(|t| t.pinned).count();
    let was = tabs[ix].pinned;
    tabs[ix].pinned = !was;
    let to = if was { pinned - 1 } else { pinned };
    let tab = tabs.remove(ix);
    tabs.insert(to, tab);
    *active = active.map(|a| {
        if a == ix {
            to
        } else if ix < a && to >= a {
            a - 1
        } else if ix > a && to <= a {
            a + 1
        } else {
            a
        }
    });
}

// ------------------------------------------------------------------ tab dragging

/// A tab being dragged by the mouse.
#[derive(Clone)]
pub(crate) struct TabDrag {
    pub source: Dest,
    pub ix: usize,
    pub label: String,
    /// Where the mouse went down and where it is now, in screen coordinates.
    pub grab: Point<Pixels>,
    pub screen: Point<Pixels>,
    /// The mouse has moved far enough for this to be a drag and not a click.
    pub moved: bool,
}

struct DragSlot(Option<TabDrag>);

impl Global for DragSlot {}

/// Where every window's tab strip is, in screen coordinates, as of the last frame. Drops use it
/// to find the gap between tabs the mouse is over, in any window.
#[derive(Default)]
struct Geo {
    wins: HashMap<Dest, Bounds<Pixels>>,
    strips: HashMap<Dest, Bounds<Pixels>>,
    tabs: HashMap<(Dest, usize), Bounds<Pixels>>,
}

impl Global for Geo {}

pub(crate) fn drag_state(cx: &App) -> Option<TabDrag> {
    cx.try_global::<DragSlot>().and_then(|d| d.0.clone())
}

fn set_drag(cx: &mut App, drag: Option<TabDrag>) {
    cx.set_global(DragSlot(drag));
    cx.refresh_windows();
}

/// The drag ended: forget it and say what it was.
pub(crate) fn end_drag(cx: &mut App) -> Option<TabDrag> {
    let d = drag_state(cx);
    if d.is_some() {
        set_drag(cx, None);
    }
    d
}

/// A window went away: its strip no longer takes drops.
pub(crate) fn forget_geometry(dest: Dest, cx: &mut App) {
    if cx.has_global::<Geo>() {
        let geo = cx.global_mut::<Geo>();
        geo.wins.remove(&dest);
        geo.strips.remove(&dest);
        geo.tabs.retain(|(d, _), _| *d != dest);
    }
}

fn geo(cx: &App) -> Option<&Geo> {
    cx.try_global::<Geo>()
}

/// The gap between tabs (0 = before the first) of `dest`'s strip that the point is over.
pub(crate) fn gap_at(cx: &App, dest: Dest, p: Point<Pixels>) -> Option<usize> {
    let geo = geo(cx)?;
    let strip = geo.strips.get(&dest)?;
    if !strip.contains(&p) {
        return None;
    }
    let mut tabs: Vec<(usize, Bounds<Pixels>)> = geo
        .tabs
        .iter()
        .filter(|((d, _), _)| *d == dest)
        .map(|((_, i), b)| (*i, *b))
        .collect();
    tabs.sort_by_key(|(i, _)| *i);
    if tabs.is_empty() {
        return Some(0);
    }
    // Tabs wrap onto several rows: only the row under the pointer counts.
    let row: Vec<_> = tabs
        .iter()
        .filter(|(_, b)| p.y >= b.top() && p.y < b.bottom())
        .collect();
    let Some(last) = row.last() else {
        return Some(tabs.len());
    };
    for (i, b) in &row {
        if p.x < b.center().x {
            return Some(*i);
        }
    }
    Some(last.0 + 1)
}

/// The window other than `me` that is under the point, if any.
fn other_window_at(cx: &App, me: Dest, p: Point<Pixels>) -> Option<Dest> {
    geo(cx)?
        .wins
        .iter()
        .find(|(d, b)| **d != me && b.contains(&p))
        .map(|(d, _)| *d)
}

/// Reports where a window's tab strip is (placed first inside the strip).
fn strip_geometry(me: Dest) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            if !cx.has_global::<Geo>() {
                cx.set_global(Geo::default());
            }
            let origin = window.bounds().origin;
            let win = window.bounds();
            let geo = cx.global_mut::<Geo>();
            geo.wins.insert(me, win);
            geo.strips
                .insert(me, Bounds::new(origin + bounds.origin, bounds.size));
            geo.tabs.retain(|(d, _), _| *d != me);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// Reports where one tab is (placed inside the tab).
fn tab_geometry(me: Dest, ix: usize) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            let origin = window.bounds().origin;
            if cx.has_global::<Geo>() {
                cx.global_mut::<Geo>()
                    .tabs
                    .insert((me, ix), Bounds::new(origin + bounds.origin, bounds.size));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// Ctrl + mouse wheel over its area (put inside the editor area): `on_step(+1)` for a step up,
/// `on_step(-1)` for a step down. The wheel then does not scroll the editor.
pub(crate) fn wheel_zoom_probe(
    on_step: impl Fn(i32, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_step = std::rc::Rc::new(on_step);
    let acc = std::rc::Rc::new(std::cell::Cell::new(0f32));
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let (on_step, acc) = (on_step.clone(), acc.clone());
            window.on_mouse_event(move |ev: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture
                    || !ev.modifiers.secondary()
                    || !bounds.contains(&ev.position)
                {
                    return;
                }
                cx.stop_propagation();
                // A wheel notch is one step; a touchpad sends many small movements.
                let dy = ev.delta.pixel_delta(px(20.)).y;
                let total = acc.get() + f32::from(dy);
                if total.abs() >= 20. {
                    acc.set(0.);
                    on_step(if total > 0. { 1 } else { -1 }, window, cx);
                } else {
                    acc.set(total);
                }
            });
        },
    )
    .absolute()
    .size_full()
}

/// Follows the mouse while one of this window's tabs is held, even outside the window.
pub(crate) fn drag_probe(me: Dest) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            let origin = window.bounds().origin;
            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Bubble || ev.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let Some(mut d) = drag_state(cx).filter(|d| d.source == me) else {
                    return;
                };
                d.screen = origin + ev.position;
                let (dx, dy) = (d.screen.x - d.grab.x, d.screen.y - d.grab.y);
                if !d.moved && dx.abs().max(dy.abs()) >= px(5.) {
                    d.moved = true;
                }
                set_drag(cx, Some(d));
            });
        },
    )
    .absolute()
    .size_0()
}

/// What a window shows while a tab is dragged: a floating label at the mouse, a frame around the
/// window the tab will land in, or a hint that letting go opens a new window.
pub(crate) fn drag_overlay(
    me: Dest,
    accent: Hsla,
    fg: Hsla,
    bg: Hsla,
    window: &Window,
    cx: &App,
) -> Option<AnyElement> {
    let d = drag_state(cx).filter(|d| d.moved)?;
    let bounds = window.bounds();
    let inside = bounds.contains(&d.screen);
    let chip = |text: String| {
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(accent)
            .bg(bg)
            .text_color(fg)
            .text_sm()
            .child(text)
    };
    if inside {
        let at = d.screen - bounds.origin;
        let landing = d.source != me;
        let mut layer = div().absolute().top_0().left_0().size_full().child(
            div()
                .absolute()
                .left(at.x + px(14.))
                .top(at.y + px(14.))
                .opacity(0.92)
                .child(chip(d.label.clone())),
        );
        if landing {
            // The tab will move into this window.
            layer = layer.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .border_2()
                    .border_color(accent)
                    .bg(accent.opacity(0.07))
                    .flex()
                    .justify_center()
                    .items_end()
                    .pb_6()
                    .child(chip(format!("Release to move “{}” here", d.label))),
            );
        }
        Some(layer.into_any_element())
    } else if d.source == me && other_window_at(cx, me, d.screen).is_none() {
        // Over nothing of Kalem's: letting go makes a window of its own.
        Some(
            div()
                .absolute()
                .top(px(40.))
                .left_0()
                .w_full()
                .flex()
                .justify_center()
                .child(
                    chip(format!("Release to open “{}” in a new window", d.label))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(icon("app-window").size_4().text_color(accent)),
                )
                .into_any_element(),
        )
    } else {
        None
    }
}

/// What a window with a tab strip must do when its tabs are used.
pub(crate) trait TabHost: Sized + 'static {
    fn tab_activate(&mut self, ix: usize, cx: &mut Context<Self>);
    fn tab_close(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>);
    /// The mouse went down on a tab: it may be dragged out of the window.
    fn tab_grab(&mut self, ix: usize);
    /// A tab menu command (pin, ...).
    fn tab_op(&mut self, ix: usize, op: TabOp, window: &mut Window, cx: &mut Context<Self>);
}

/// The menu of the empty part of the tab strip.
fn strip_menu(m: PopupMenu, can_restore: bool) -> PopupMenu {
    m.menu("Close All Tabs", Box::new(TabCmd(0, TabOp::CloseAll)))
        .menu_with_disabled(
            "Restore Closed Tab",
            Box::new(TabCmd(0, TabOp::Restore)),
            !can_restore,
        )
        .menu(
            "All To New Window",
            Box::new(TabCmd(0, TabOp::AllToNewWindow)),
        )
}

/// One row of tabs, coloured per project, with a "Move to window" context menu.
pub(crate) fn tab_strip<T: TabHost>(
    tabs: &[OpenTab],
    active: Option<usize>,
    sc: &StripCtx,
    cx: &mut Context<T>,
) -> impl IntoElement {
    let (accent, muted, border, active_bg) = (sc.accent, sc.muted, sc.border, sc.active_bg);
    let drag = drag_state(cx).filter(|d| d.moved);
    // The gap between tabs the dragged tab would land in, in strip-relative pixels.
    let marker = drag.as_ref().and_then(|d| {
        let gap = gap_at(cx, sc.me, d.screen)?;
        let geo = geo(cx)?;
        let strip = geo.strips.get(&sc.me)?;
        let (tab, at_end) = match gap.checked_sub(1) {
            Some(prev) if geo.tabs.contains_key(&(sc.me, prev)) => (geo.tabs[&(sc.me, prev)], true),
            _ => (*geo.tabs.get(&(sc.me, gap))?, false),
        };
        let x = if at_end { tab.right() } else { tab.left() };
        Some((
            x - strip.origin.x,
            tab.top() - strip.origin.y,
            tab.size.height,
        ))
    });
    let can_restore = sc.can_restore;
    div()
        .flex()
        .flex_wrap()
        .relative()
        .child(strip_geometry(sc.me))
        .children(tabs.iter().enumerate().map(|(i, t)| {
            let name = match &t.diff {
                Some(d) => d.title(),
                None => t.path.rsplit('/').next().unwrap_or(&t.path).to_string(),
            };
            let is_active = active == Some(i);
            // Each project gets its own colour on the tab's top edge, so you can tell at a
            // glance which project a document belongs to.
            let proj_col = match t.color {
                Some(c) => Some(rgba(sc.palette.get(&format!("tab{c}")))),
                None => project_index(&t.path, &sc.proj_dirs)
                    .map(|ix| rgba(sc.palette.get(&format!("tab{}", ix % 8 + 1)))),
            };
            let dests = sc.dests.clone();
            let dragged = drag
                .as_ref()
                .is_some_and(|d| d.source == sc.me && d.ix == i);
            let (me, label) = (sc.me, name.clone());
            let (pinned, color, can_restore) = (t.pinned, t.color, sc.can_restore);
            div()
                .id(("tab", i))
                .relative()
                .when(dragged, |d| d.opacity(0.45))
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
                .child(tab_geometry(sc.me, i))
                .child(format!("{}{}", name, if t.dirty { " *" } else { "" }))
                .child(if pinned {
                    // A pinned tab has no close button; its pin unpins it.
                    div()
                        .id(("unpin", i))
                        .child(icon("pin").size_3().text_color(accent))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.tab_op(i, TabOp::Pin, window, cx)
                        }))
                } else {
                    div()
                        .id(("close", i))
                        .text_color(muted)
                        .child("x")
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.tab_close(i, window, cx)),
                        )
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        let screen = window.bounds().origin + ev.position;
                        set_drag(
                            cx,
                            Some(TabDrag {
                                source: me,
                                ix: i,
                                label: label.clone(),
                                grab: screen,
                                screen,
                                moved: false,
                            }),
                        );
                        this.tab_grab(i);
                    }),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.tab_activate(i, cx)))
                .context_menu(move |m, w, cx| {
                    let (dests, name) = (dests.clone(), name.clone());
                    let cmd = move |op| Box::new(TabCmd(i, op));
                    m.menu(format!("Save {name}"), cmd(TabOp::Save))
                        .menu("Close", cmd(TabOp::Close))
                        .menu("Close Other Tabs", cmd(TabOp::CloseOthers))
                        .menu("Close All Tabs", cmd(TabOp::CloseAll))
                        .menu_with_disabled("Restore Closed Tab", cmd(TabOp::Restore), !can_restore)
                        .separator()
                        .menu("Move To New Window", cmd(TabOp::ToNewWindow))
                        .menu("All To New Window", cmd(TabOp::AllToNewWindow))
                        .submenu("Move", w, cx, move |mut m, _, _| {
                            for (label, dest) in &dests {
                                m = m.menu(label.clone(), Box::new(MoveTab(i, *dest)));
                            }
                            m
                        })
                        .separator()
                        .menu(
                            if pinned { "Unpin Tab" } else { "Pin Tab" },
                            cmd(TabOp::Pin),
                        )
                        .submenu("Set Tab Color", w, cx, move |mut m, _, _| {
                            m = m.menu_with_check(
                                "Automatic (project)",
                                color.is_none(),
                                cmd(TabOp::Color(None)),
                            );
                            for c in 1..=8u8 {
                                m = m.menu_with_check(
                                    format!("Color {c}"),
                                    color == Some(c),
                                    cmd(TabOp::Color(Some(c))),
                                );
                            }
                            m
                        })
                        .separator()
                        .menu("Locate in Solution Explorer", cmd(TabOp::Locate))
                        .menu("Copy Full Path", cmd(TabOp::CopyPath))
                        .menu("Open in File Explorer", cmd(TabOp::Reveal))
                })
        }))
        .child(
            // The free space after the last tab: its own menu.
            div()
                .id(("strip-free", sc.me_id()))
                .flex_1()
                .min_w(px(40.))
                .h(px(30.))
                .context_menu(move |m, _, _| strip_menu(m, can_restore)),
        )
        .when_some(marker, |d, (x, y, h)| {
            d.child(
                div()
                    .absolute()
                    .left(x - px(1.5))
                    .top(y)
                    .w(px(3.))
                    .h(h)
                    .bg(accent),
            )
            .child(
                div()
                    .absolute()
                    .left(x - px(8.))
                    .top(y - px(12.))
                    .child(icon("chevron-down").size_4().text_color(accent)),
            )
            .child(
                div()
                    .absolute()
                    .left(x - px(8.))
                    .top(y + h - px(4.))
                    .child(icon("chevron-up").size_4().text_color(accent)),
            )
        })
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
        tabs: Vec<OpenTab>,
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
                    forget_geometry(Dest::Window(id), cx);
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
        for tab in tabs {
            me.adopt(tab, cx);
        }
        window.focus(&me.focus, cx);
        me
    }

    /// Take ownership of a tab (its dirty tracking is re-attached here).
    pub fn adopt(&mut self, tab: OpenTab, cx: &mut Context<Self>) {
        self.adopt_at(tab, None, cx);
    }

    /// Like [`Self::adopt`], placing the tab in the gap `gap` between tabs (at the end if `None`).
    pub fn adopt_at(&mut self, mut tab: OpenTab, gap: Option<usize>, cx: &mut Context<Self>) {
        tab._sub = cx.subscribe(&tab.editor, |this, editor, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                let now = editor.read(cx).value();
                if let Some(t) = this.tabs.iter_mut().find(|t| t.editor == editor) {
                    t.dirty = now != t.saved;
                    cx.notify();
                }
            }
        });
        let at = gap.unwrap_or(usize::MAX).min(self.tabs.len());
        self.tabs.insert(at, tab);
        self.active = Some(at);
        cx.notify();
    }

    /// Drag-and-drop inside the strip: move tab `from` into the gap `gap` between tabs.
    fn reorder_tab(&mut self, from: usize, gap: usize, cx: &mut Context<Self>) {
        if from >= self.tabs.len() {
            return;
        }
        let to = if gap > from { gap - 1 } else { gap }.min(self.tabs.len() - 1);
        if to == from {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = Some(to);
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
        gap: Option<usize>,
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
        main.update(cx, |k, cx| k.deliver(tab, dest, at, gap, cx));
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
        let dragged = end_drag(cx).is_some_and(|d| d.moved);
        if !dragged {
            return; // a click
        }
        if window.bounds().contains(&screen) {
            // Dropped inside: only the gaps of the tab strip take a tab.
            if let Some(gap) = gap_at(cx, Dest::Window(self.id), screen) {
                self.reorder_tab(ix, gap, cx);
            }
            return;
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
        let gap = (dest != Dest::New)
            .then(|| gap_at(cx, dest, screen))
            .flatten();
        self.move_tab(ix, dest, Some(screen), gap, window, cx);
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

    fn tab_op(&mut self, ix: usize, op: TabOp, window: &mut Window, cx: &mut Context<Self>) {
        self.run_tab_op(ix, op, window, cx);
    }
}

impl Detached {
    /// Put a tab menu command into effect for a tab of this window.
    fn run_tab_op(&mut self, ix: usize, op: TabOp, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.tabs.get(ix).map(|t| t.path.clone());
        let main = self.main.upgrade();
        match op {
            TabOp::Save => {
                if ix < self.tabs.len() {
                    self.active = Some(ix);
                    self.save(cx);
                }
            }
            TabOp::Close => self.tab_close(ix, window, cx),
            TabOp::CloseOthers => {
                for i in closable(&self.tabs, Some(ix)) {
                    self.tab_close(i, window, cx);
                }
            }
            TabOp::CloseAll => {
                for i in closable(&self.tabs, None) {
                    self.tab_close(i, window, cx);
                }
            }
            TabOp::Restore => {
                if let Some(main) = main {
                    let handle = main.read(cx).window_handle();
                    let _ = handle.update(cx, |_, w, cx| {
                        main.update(cx, |k, cx| k.restore_closed_tab(w, cx))
                    });
                }
            }
            TabOp::ToNewWindow => self.move_tab(ix, Dest::New, None, None, window, cx),
            TabOp::AllToNewWindow => {
                if let Some(main) = main {
                    if !self.tabs.is_empty() {
                        let tabs = std::mem::take(&mut self.tabs);
                        self.active = None;
                        window.remove_window();
                        main.update(cx, |k, cx| k.deliver_all(tabs, cx));
                    }
                }
            }
            TabOp::Pin => {
                if ix < self.tabs.len() {
                    toggle_pin(&mut self.tabs, ix, &mut self.active);
                    cx.notify();
                }
            }
            TabOp::Color(c) => {
                if let Some(t) = self.tabs.get_mut(ix) {
                    t.color = c;
                    cx.notify();
                }
            }
            TabOp::Locate => {
                if let (Some(main), Some(p)) = (main, path) {
                    main.update(cx, |k, cx| k.locate_in_explorer(&p, cx));
                }
            }
            TabOp::CopyPath => {
                let abs = main.zip(path).and_then(|(m, p)| m.read(cx).abs_path(&p));
                if let Some(abs) = abs {
                    cx.write_to_clipboard(ClipboardItem::new_string(abs));
                }
            }
            TabOp::Reveal => {
                let abs = main.zip(path).and_then(|(m, p)| m.read(cx).abs_path(&p));
                if let Some(abs) = abs {
                    cx.reveal_path(std::path::Path::new(&abs));
                }
            }
        }
    }
}

impl Render for Detached {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let can_restore = self
            .main
            .upgrade()
            .is_some_and(|m| m.read(cx).can_restore_tab());
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
            me: Dest::Window(self.id),
            can_restore,
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
        let dragging = drag_overlay(
            Dest::Window(self.id),
            sc.accent,
            theme.foreground,
            theme.popover,
            window,
            cx,
        );
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
            .on_action(
                cx.listener(|this, a: &MoveTab, w, cx| this.move_tab(a.0, a.1, None, None, w, cx)),
            )
            .on_action(cx.listener(|this, a: &TabCmd, w, cx| this.run_tab_op(a.0, a.1, w, cx)))
            .child(mouse_up_probe(view, Self::mouse_up))
            .child(drag_probe(Dest::Window(self.id)))
            .child(tab_strip(&self.tabs, self.active, &sc, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(wheel_zoom_probe({
                        let main = self.main.clone();
                        move |dir, _, cx| {
                            if let Some(main) = main.upgrade() {
                                main.update(cx, |k, cx| {
                                    k.set_zoom(crate::zoom::step(crate::zoom::percent(cx), dir), cx)
                                });
                            }
                        }
                    }))
                    .child(match active {
                        Some(t) if t.diff.is_some() => diff_view(
                            t.diff.as_ref().unwrap(),
                            &palette2,
                            theme.foreground,
                            sc.muted,
                            sc.border,
                            false,
                            crate::zoom::text_size(cx),
                        ),
                        Some(t) => Editor::new(&t.editor)
                            .text_size(crate::zoom::text_size(cx))
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
            .children(dragging)
    }
}
