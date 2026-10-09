//! Diff of one file, shown in an editor tab: two panes with the line numbers in a gutter between
//! them, changed blocks tinted across both panes and the gutter (blue changed, green added, grey
//! removed), the changed words inside a modified line picked out, and previous / next difference.

use crate::icons::icon;
use crate::theming::rgba;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{
    Editor, EditorState, RangeDecoration, RangeDecorationCollection, RangeDecorationStyle,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kalem_theme::Palette;
use kalem_vcs::diff::{
    changed_words as intra, line_changes, DiffLine, DiffRow, FileDiff, LineChange, LineKind,
};
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

/// Rows beyond this are cut off; a diff this long is better read elsewhere.
const MAX_ROWS: usize = 4000;
const ROW_H: f32 = 20.;
const GUTTER_W: f32 = 92.;

pub(crate) struct DiffDoc {
    pub path: String,
    pub staged: bool,
    pub untracked: bool,
    pub diff: FileDiff,
    pub rows: Vec<DiffRow>,
    pub error: Option<String>,
    /// View state: scroll position, current difference, unified or side by side.
    pub scroll: ScrollHandle,
    pub cur: Cell<usize>,
    pub unified: Cell<bool>,
    placed: Cell<bool>,
    /// Working-tree diffs are two real editors: the right one is the file and can be edited.
    pub live: Option<Rc<Live>>,
}

impl DiffDoc {
    pub fn new(
        path: String,
        staged: bool,
        untracked: bool,
        diff: FileDiff,
        rows: Vec<DiffRow>,
        error: Option<String>,
    ) -> Self {
        Self {
            path,
            staged,
            untracked,
            diff,
            rows,
            error,
            scroll: ScrollHandle::new(),
            cur: Cell::new(0),
            unified: Cell::new(false),
            placed: Cell::new(false),
            live: None,
        }
    }

    /// Attach the two live editors of an editable diff.
    pub fn with_live(mut self, live: Option<Rc<Live>>) -> Self {
        self.live = live;
        self
    }

    /// Tab title: `name (Staged)` / `name (Working Tree)`.
    pub fn title(&self) -> String {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        format!(
            "{name} ({})",
            if self.staged {
                "Staged"
            } else {
                "Working Tree"
            }
        )
    }

    fn scroll_to(&self, row: usize) {
        let y = (row as f32 * ROW_H - 4. * ROW_H).max(0.);
        self.scroll.set_offset(point(px(0.), px(-y)));
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Change {
    Same,
    Added,
    Removed,
    Modified,
}

/// One line of code as shown: its text and the char ranges to pick out (the changed words).
struct Code {
    text: String,
    emph: Vec<(usize, usize)>,
}

enum Item {
    /// Collapsed unchanged lines between two hunks.
    Sep(String),
    /// An unchanged line, in both panes.
    Row {
        no_old: Option<u32>,
        no_new: Option<u32>,
        code: Code,
    },
    /// One difference in the side-by-side view: `left` lines became `right` lines (5 become 1,
    /// 0 become 3, ...). The panes are not padded; a ribbon in the gutter joins the two spans.
    Block {
        left: Vec<(Option<u32>, Code)>,
        right: Vec<(Option<u32>, Code)>,
    },
    Uni {
        old: Option<u32>,
        new: Option<u32>,
        code: Code,
        change: Change,
    },
}

fn change_of(left: &Option<DiffLine>, right: &Option<DiffLine>) -> Change {
    let removed = matches!(left, Some(l) if l.kind == LineKind::Removed);
    let added = matches!(right, Some(l) if l.kind == LineKind::Added);
    match (removed, added) {
        (true, true) => Change::Modified,
        (true, false) => Change::Removed,
        (false, true) => Change::Added,
        _ => Change::Same,
    }
}

fn text_of(l: &DiffLine) -> String {
    l.text.replace('\t', "    ")
}

fn flush_unified(out: &mut Vec<Item>, block: &mut Vec<(Option<DiffLine>, Option<DiffLine>)>) {
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for (l, r) in block.drain(..) {
        let (el, er) = match (&l, &r) {
            (Some(a), Some(b)) => intra(&text_of(a), &text_of(b)),
            _ => (Vec::new(), Vec::new()),
        };
        if let Some(l) = l {
            removed.push(Item::Uni {
                old: l.old_no,
                new: None,
                code: Code {
                    text: text_of(&l),
                    emph: el,
                },
                change: Change::Removed,
            });
        }
        if let Some(r) = r {
            added.push(Item::Uni {
                old: None,
                new: r.new_no,
                code: Code {
                    text: text_of(&r),
                    emph: er,
                },
                change: Change::Added,
            });
        }
    }
    out.append(&mut removed);
    out.append(&mut added);
}

fn flush_side(out: &mut Vec<Item>, side: &mut (Vec<DiffLine>, Vec<DiffLine>)) {
    let (l, r) = std::mem::take(side);
    if l.is_empty() && r.is_empty() {
        return;
    }
    // Lines are matched top-down; the changed words are picked out where both sides have a line.
    let mut left = Vec::new();
    let mut right = Vec::new();
    for i in 0..l.len().max(r.len()) {
        let (el, er) = match (l.get(i), r.get(i)) {
            (Some(a), Some(b)) => intra(&text_of(a), &text_of(b)),
            _ => (Vec::new(), Vec::new()),
        };
        if let Some(a) = l.get(i) {
            left.push((
                a.old_no,
                Code {
                    text: text_of(a),
                    emph: el,
                },
            ));
        }
        if let Some(b) = r.get(i) {
            right.push((
                b.new_no,
                Code {
                    text: text_of(b),
                    emph: er,
                },
            ));
        }
    }
    out.push(Item::Block { left, right });
}

fn items(rows: &[DiffRow], unified: bool) -> Vec<Item> {
    let mut out = Vec::new();
    let mut block = Vec::new();
    let mut side: (Vec<DiffLine>, Vec<DiffLine>) = (Vec::new(), Vec::new());
    let (mut prev_old, mut prev_new) = (0u32, 0u32);
    let mut pending_sep = false;
    for row in rows.iter().take(MAX_ROWS) {
        match row {
            DiffRow::Hunk(_) => {
                flush_unified(&mut out, &mut block);
                flush_side(&mut out, &mut side);
                pending_sep = true;
            }
            DiffRow::Pair { left, right } => {
                let no = |pick: fn(&DiffLine) -> Option<u32>| {
                    right
                        .as_ref()
                        .and_then(pick)
                        .or_else(|| left.as_ref().and_then(pick))
                };
                if pending_sep {
                    pending_sep = false;
                    let gap = match no(|l| l.new_no) {
                        Some(n) => n.saturating_sub(prev_new + 1),
                        None => no(|l| l.old_no)
                            .map(|o| o.saturating_sub(prev_old + 1))
                            .unwrap_or(0),
                    };
                    if gap > 0 {
                        out.push(Item::Sep(format!(
                            "{gap} unchanged line{}",
                            if gap == 1 { "" } else { "s" }
                        )));
                    }
                }
                for l in [left, right].into_iter().flatten() {
                    prev_old = l.old_no.unwrap_or(prev_old);
                    prev_new = l.new_no.unwrap_or(prev_new);
                }
                let change = change_of(left, right);
                if unified {
                    if change == Change::Same {
                        flush_unified(&mut out, &mut block);
                        if let Some(l) = left.as_ref().or(right.as_ref()) {
                            out.push(Item::Uni {
                                old: l.old_no,
                                new: l.new_no,
                                code: Code {
                                    text: text_of(l),
                                    emph: Vec::new(),
                                },
                                change,
                            });
                        }
                    } else {
                        block.push((left.clone(), right.clone()));
                    }
                } else if change == Change::Same {
                    flush_side(&mut out, &mut side);
                    if let Some(l) = left.as_ref().or(right.as_ref()) {
                        out.push(Item::Row {
                            no_old: l.old_no,
                            no_new: l.new_no,
                            code: Code {
                                text: text_of(l),
                                emph: Vec::new(),
                            },
                        });
                    }
                } else {
                    if let Some(l) = left {
                        side.0.push(l.clone());
                    }
                    if let Some(r) = right {
                        side.1.push(r.clone());
                    }
                }
            }
        }
    }
    flush_side(&mut out, &mut side);
    flush_unified(&mut out, &mut block);
    out
}

/// How many rows tall an item is.
fn height(i: &Item) -> usize {
    match i {
        Item::Block { left, right } => left.len().max(right.len()),
        _ => 1,
    }
}

fn is_change(i: &Item) -> bool {
    match i {
        Item::Block { .. } => true,
        Item::Uni { change, .. } => *change != Change::Same,
        _ => false,
    }
}

/// Row offset where each difference (a block, or a run of changed lines) starts.
fn blocks(items: &[Item]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut at = 0;
    for it in items {
        let changed = is_change(it);
        if changed && !inside {
            out.push(at);
        }
        inside = changed;
        at += height(it);
    }
    out
}

/// Fixed tints (background, changed words) so a change always looks the same: blue where lines
/// changed, grey for lines that went away, green for lines that are new.
fn tint(c: Change, dark: bool) -> (Hsla, Hsla) {
    let (bg, em) = match (c, dark) {
        (Change::Same, _) => return (gpui_kit::transparent_black(), gpui_kit::transparent_black()),
        (Change::Modified, true) => (0x25323E, 0x385570),
        (Change::Removed, true) => (0x484A4A, 0x666868),
        (Change::Added, true) => (0x24402B, 0x356040),
        (Change::Modified, false) => (0xDCE8F8, 0xB3CDF0),
        (Change::Removed, false) => (0xE4E4E4, 0xCCCCCC),
        (Change::Added, false) => (0xDDF2DD, 0xB2E0B2),
    };
    (rgba(bg), rgba(em))
}

/// The same tints as a see-through layer, for painting over an editor (which draws its own
/// opaque background): on the usual editor background it comes out as `tint`.
fn wash(c: Change, dark: bool) -> Hsla {
    let (rgb, a) = match (c, dark) {
        (Change::Same, _) => return gpui_kit::transparent_black(),
        (Change::Modified, true) => (0x468CD2, 0.18),
        (Change::Removed, true) => (0xC8C8C8, 0.25),
        (Change::Added, true) => (0x3CE66E, 0.17),
        (Change::Modified, false) => (0x4682DC, 0.20),
        (Change::Removed, false) => (0x787878, 0.20),
        (Change::Added, false) => (0x3CBE3C, 0.17),
    };
    rgba(rgb).opacity(a)
}

const RIBBON_W: f32 = 48.;
/// Space between the top of an editor and its first line.
const EDITOR_PAD: f32 = 8.;

/// The two editors of an editable diff, and what is known about how they differ.
pub(crate) struct Live {
    pub left: Entity<EditorState>,
    pub right: Entity<EditorState>,
    /// The old version (the index) that the left editor shows.
    pub base: RefCell<String>,
    /// The left editor's text has to be replaced by `base` (checked while rendering).
    pub base_stale: Cell<bool>,
    pub changes: RefCell<Vec<LineChange>>,
    deco_left: RangeDecorationCollection,
    deco_right: RangeDecorationCollection,
    last_sync: Cell<f32>,
    pub cur: Cell<usize>,
    placed: Cell<bool>,
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut v = vec![0];
    v.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    v
}

/// Byte range of the chars `a..b` of the line that starts at `start`.
fn char_span(text: &str, start: usize, a: usize, b: usize) -> Range<usize> {
    let line = text[start..].split('\n').next().unwrap_or("");
    let at = |n: usize| {
        line.char_indices()
            .nth(n)
            .map(|(i, _)| i)
            .unwrap_or(line.len())
    };
    start + at(a)..start + at(b)
}

impl Live {
    pub fn new(
        left: Entity<EditorState>,
        right: Entity<EditorState>,
        base: String,
        cx: &mut App,
    ) -> Self {
        let deco_left = left.update(cx, |e, cx| {
            e.create_range_decorations_collection(Vec::new(), cx)
        });
        let deco_right = right.update(cx, |e, cx| {
            e.create_range_decorations_collection(Vec::new(), cx)
        });
        Self {
            left,
            right,
            base: RefCell::new(base),
            base_stale: Cell::new(false),
            changes: RefCell::new(Vec::new()),
            deco_left,
            deco_right,
            last_sync: Cell::new(f32::NAN),
            cur: Cell::new(0),
            placed: Cell::new(false),
        }
    }

    /// Compare the two texts again and repaint the highlights (after every edit).
    pub fn refresh(&self, dark: bool, cx: &mut App) {
        let old = self.base.borrow().clone();
        let new = self.right.read(cx).value().to_string();
        let changes = line_changes(&old, &new);
        let (so, sn) = (line_starts(&old), line_starts(&new));
        let fill = |r: Range<usize>, c: Hsla| {
            RangeDecoration::new(r)
                .with_style(RangeDecorationStyle::Fill)
                .with_color(c)
        };
        let (mut dl, mut dr) = (Vec::new(), Vec::new());
        for c in &changes {
            let paired = c.old.len().min(c.new.len());
            // The changed words of lines that have a partner on the other side.
            for i in 0..paired {
                let (lo, ln) = (c.old.start + i, c.new.start + i);
                let (Some(&a), Some(&b)) = (so.get(lo), sn.get(ln)) else {
                    continue;
                };
                let line = |t: &str, s: usize| {
                    let l = t[s..].split('\n').next().unwrap_or("");
                    l.strip_suffix('\r').unwrap_or(l).to_string()
                };
                let (ta, tb) = (line(&old, a), line(&new, b));
                let (ea, eb) = intra(&ta, &tb);
                for (x, y) in ea {
                    dl.push(fill(
                        char_span(&old, a, x, y),
                        tint(Change::Modified, dark).1,
                    ));
                }
                for (x, y) in eb {
                    dr.push(fill(
                        char_span(&new, b, x, y),
                        tint(Change::Modified, dark).1,
                    ));
                }
            }
        }
        self.deco_left.set(dl, cx);
        self.deco_right.set(dr, cx);
        *self.changes.borrow_mut() = changes;
        self.last_sync.set(f32::NAN);
    }

    /// Left editor line shown level with a line of the right editor (an unchanged stretch keeps
    /// its offset; inside a difference the lines are scaled to fit the old span).
    fn map_to_old(&self, line: f32) -> f32 {
        let mut shift = 0.0f32; // old - new
        for c in self.changes.borrow().iter() {
            let (ns, ne) = (c.new.start as f32, c.new.end as f32);
            if line < ns {
                break;
            }
            if line < ne {
                let span = (ne - ns).max(1.0);
                return c.old.start as f32 + (line - ns) / span * c.old.len() as f32;
            }
            shift = c.old.end as f32 - ne;
        }
        (line + shift).max(0.0)
    }

    /// Vertical scroll offset of the left editor when the right one is at `ro_y`.
    fn left_offset(&self, ro_y: f32, lh: f32) -> f32 {
        -(self.map_to_old((-ro_y / lh).max(0.0)) * lh)
    }

    /// Keep the left editor level with the right one. Called every frame.
    pub fn sync(&self, cx: &mut App) {
        let (lh, ro, lo) = {
            let (l, r) = (self.left.read(cx), self.right.read(cx));
            let Some(lh) = r.line_height().or(l.line_height()) else {
                return;
            };
            (f32::from(lh), r.scroll_offset(), l.scroll_offset())
        };
        // The first time round, show the first difference.
        if !self.placed.replace(true) {
            self.goto(0, cx);
            return;
        }
        let want = self.left_offset(f32::from(ro.y), lh);
        let last = self.last_sync.get();
        if last.is_nan() || (want - last).abs() > 0.5 {
            self.last_sync.set(want);
            self.left.update(cx, |e, cx| {
                e.set_scroll_offset(point(lo.x, px(want)), cx);
            });
        }
    }

    /// Scroll the right editor to difference `ix`.
    pub fn goto(&self, ix: usize, cx: &mut App) {
        let Some(c) = self.changes.borrow().get(ix).cloned() else {
            return;
        };
        self.cur.set(ix);
        self.right.update(cx, |e, cx| {
            let Some(lh) = e.line_height() else { return };
            let y = (c.new.start as f32 - 3.0).max(0.0) * f32::from(lh);
            let o = e.scroll_offset();
            e.set_scroll_offset(point(o.x, px(-y)), cx);
        });
    }
}

/// The editable view of a working-tree diff: old version on the left (read-only), the file on the
/// right, ribbons between them.
fn live_view(
    doc: &Rc<DiffDoc>,
    live: &Rc<Live>,
    palette: &Palette,
    fg: Hsla,
    muted: Hsla,
    border: Hsla,
    text_size: Pixels,
) -> AnyElement {
    let (add_c, del_c) = (rgba(palette.get("success")), rgba(palette.get("danger")));
    let dark = fg.l > 0.5;
    let changes = live.changes.borrow().clone();
    let n = changes.len();
    let (added, removed) = changes
        .iter()
        .fold((0, 0), |(a, r), c| (a + c.new.len(), r + c.old.len()));
    let cur = live.cur.get().min(n.saturating_sub(1));

    let step = |id: &'static str, ic: &'static str, tip: &'static str, dir: i32| {
        let live = live.clone();
        Button::new(id)
            .ghost()
            .compact()
            .icon(icon(ic))
            .tooltip(tip)
            .on_click(move |_, w, cx| {
                let n = live.changes.borrow().len();
                if n == 0 {
                    return;
                }
                let at = (live.cur.get() as i32 + dir).clamp(0, n as i32 - 1) as usize;
                live.goto(at, cx);
                w.refresh();
            })
    };
    let toolbar = div()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .h(px(32.))
        .flex_none()
        .border_b_1()
        .border_color(border)
        .text_sm()
        .child(step("diff-prev", "arrow-up", "Previous Difference", -1))
        .child(step("diff-next", "arrow-down", "Next Difference", 1))
        .child(div().px_2().text_color(muted).child(match n {
            0 => "No differences".to_string(),
            1 => "1 difference".to_string(),
            n => format!("{} of {n} differences", cur + 1),
        }))
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .gap_2()
                .px_2()
                .text_color(add_c)
                .child(format!("+{added}"))
                .child(div().text_color(del_c).child(format!("-{removed}"))),
        )
        .child(
            div()
                .text_color(muted)
                .text_xs()
                .child("Edit the right side, Ctrl+S saves"),
        );
    let title = |text: String| {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .px_2()
            .text_color(muted)
            .child(text)
    };
    let titles = div()
        .flex()
        .items_center()
        .h(px(24.))
        .flex_none()
        .border_b_1()
        .border_color(border)
        .text_xs()
        .child(title(format!(
            "{} · {}",
            if doc.untracked { "(nothing)" } else { "Index" },
            doc.path
        )))
        .child(div().w(px(RIBBON_W)).flex_none())
        .child(title("Current version".to_string()));

    // Ribbons: left edge spans the old lines of a difference, right edge the new ones.
    let (left, right) = (live.left.clone(), live.right.clone());
    let ribbons = canvas(|_, _, _| (), {
        let live = live.clone();
        move |b, _, w, cx| {
            let (l, r) = (left.read(cx), right.read(cx));
            let Some(lh) = r.line_height().or(l.line_height()) else {
                return;
            };
            let lh = f32::from(lh);
            let ro = f32::from(r.scroll_offset().y);
            let lo = f32::from(l.scroll_offset().y);
            let top = f32::from(b.top()) + EDITOR_PAD;
            let (x0, x1) = (b.left(), b.right());
            let y = |line: usize, off: f32| px(top + line as f32 * lh + off);
            w.with_content_mask(Some(ContentMask { bounds: b }), |w| {
                for c in live.changes.borrow().iter() {
                    let kind = if c.old.is_empty() {
                        Change::Added
                    } else if c.new.is_empty() {
                        Change::Removed
                    } else {
                        Change::Modified
                    };
                    let mut p = PathBuilder::fill();
                    p.move_to(point(x0, y(c.old.start, lo)));
                    p.line_to(point(x0, y(c.old.end, lo)));
                    p.line_to(point(x1, y(c.new.end, ro)));
                    p.line_to(point(x1, y(c.new.start, ro)));
                    p.close();
                    if let Ok(path) = p.build() {
                        w.paint_path(path, tint(kind, dark).0);
                    }
                }
            });
        }
    })
    .size_full();

    // Tinted lines behind the text, across the whole width of a pane.
    let bands = |is_left: bool| {
        let live = live.clone();
        canvas(
            |_, _, _| (),
            move |b, _, w, cx| {
                let ed = if is_left { &live.left } else { &live.right }.read(cx);
                let Some(lh) = ed.line_height() else { return };
                let lh = f32::from(lh);
                let off = f32::from(ed.scroll_offset().y);
                let top = f32::from(b.top()) + EDITOR_PAD + off;
                w.with_content_mask(Some(ContentMask { bounds: b }), |w| {
                    for c in live.changes.borrow().iter() {
                        let (r, rest) = if is_left {
                            (&c.old, Change::Removed)
                        } else {
                            (&c.new, Change::Added)
                        };
                        let paired = c.old.len().min(c.new.len());
                        for (lines, kind) in [
                            (r.start..r.start + paired, Change::Modified),
                            (r.start + paired..r.end, rest),
                        ] {
                            if lines.is_empty() {
                                continue;
                            }
                            let area = Bounds::new(
                                point(b.left(), px(top + lines.start as f32 * lh)),
                                size(b.size.width, px(lines.len() as f32 * lh)),
                            );
                            w.paint_quad(fill(area, wash(kind, dark)));
                        }
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    };

    // The old side follows the right editor: the wheel over it scrolls the right one.
    let wheel = {
        let right = live.right.clone();
        move |ev: &ScrollWheelEvent, _: &mut Window, cx: &mut App| {
            let d = ev.delta.pixel_delta(px(20.));
            right.update(cx, |e, cx| {
                let o = e.scroll_offset();
                e.set_scroll_offset(point(o.x, o.y + d.y), cx);
            });
        }
    };
    // Drawn before the left editor, so that the left editor takes the new scroll position in the
    // same frame as the right one moves.
    let level = {
        let live = live.clone();
        canvas(move |_, _, cx| live.sync(cx), |_, _, _, _| ())
            .absolute()
            .size_0()
    };
    let panes = div()
        .flex_1()
        .min_h_0()
        .flex()
        .child(level)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .relative()
                .child(
                    Editor::new(&live.left)
                        .text_size(text_size)
                        .appearance(false)
                        .readonly(true)
                        .h_full(),
                )
                .child(bands(true))
                .child(
                    div()
                        .id("diff-left-wheel")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .on_scroll_wheel(wheel),
                ),
        )
        .child(
            div()
                .w(px(RIBBON_W))
                .flex_none()
                .h_full()
                .border_l_1()
                .border_r_1()
                .border_color(border)
                .child(ribbons),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .relative()
                .child(
                    Editor::new(&live.right)
                        .text_size(text_size)
                        .appearance(false)
                        .h_full(),
                )
                .child(bands(false)),
        );
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(toolbar)
        .child(titles)
        .child(panes)
        .into_any_element()
}

pub(crate) fn diff_view(
    doc: &Rc<DiffDoc>,
    palette: &Palette,
    fg: Hsla,
    muted: Hsla,
    border: Hsla,
    live_ok: bool,
    text_size: Pixels,
) -> AnyElement {
    if let Some(live) = doc.live.as_ref().filter(|_| live_ok) {
        return live_view(doc, live, palette, fg, muted, border, text_size);
    }
    let (add_c, del_c) = (rgba(palette.get("success")), rgba(palette.get("danger")));
    let unified = doc.unified.get();

    let note = |text: String| div().p_3().text_color(muted).child(text).into_any_element();
    let broken = doc.error.is_some() || doc.diff.binary || doc.rows.is_empty();
    let list = if broken {
        Vec::new()
    } else {
        items(&doc.rows, unified)
    };
    let starts = blocks(&list);
    let n = starts.len();
    if !doc.placed.replace(true) {
        if let Some(&r) = starts.first() {
            doc.scroll_to(r);
        }
    }
    let cur = doc.cur.get().min(n.saturating_sub(1));

    // ------------------------------------------------------------- toolbar
    let step = |id: &'static str, ic: &'static str, tip: &'static str, dir: i32| {
        let (d, starts) = (doc.clone(), starts.clone());
        Button::new(id)
            .ghost()
            .compact()
            .icon(icon(ic))
            .tooltip(tip)
            .on_click(move |_, w, _| {
                if starts.is_empty() {
                    return;
                }
                let at = (d.cur.get() as i32 + dir).clamp(0, starts.len() as i32 - 1) as usize;
                d.cur.set(at);
                d.scroll_to(starts[at]);
                w.refresh();
            })
    };
    let mode = |id: &'static str, label: &'static str, want_unified: bool| {
        let d = doc.clone();
        div()
            .id(id)
            .px_2()
            .py_1()
            .rounded(px(4.))
            .cursor_pointer()
            .text_sm()
            .when(unified == want_unified, |b| {
                b.bg(muted.opacity(0.25)).text_color(fg)
            })
            .when(unified != want_unified, |b| {
                b.text_color(muted)
                    .hover(move |h| h.bg(muted.opacity(0.12)))
            })
            .child(label)
            .on_click(move |_, w, _| {
                d.unified.set(want_unified);
                d.cur.set(0);
                d.placed.set(false);
                w.refresh();
            })
    };
    let toolbar = div()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .h(px(32.))
        .flex_none()
        .border_b_1()
        .border_color(border)
        .text_sm()
        .child(step("diff-prev", "arrow-up", "Previous Difference", -1))
        .child(step("diff-next", "arrow-down", "Next Difference", 1))
        .child(div().px_2().text_color(muted).child(match n {
            0 => "No differences".to_string(),
            1 => "1 difference".to_string(),
            n => format!("{} of {n} differences", cur + 1),
        }))
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .gap_2()
                .px_2()
                .text_color(add_c)
                .child(format!("+{}", doc.diff.added))
                .child(
                    div()
                        .text_color(del_c)
                        .child(format!("-{}", doc.diff.removed)),
                ),
        )
        .child(mode("diff-side", "Side-by-side", false))
        .child(mode("diff-uni", "Unified", true));

    // ---------------------------------------------------------- pane titles
    let (old_label, new_label) = if doc.staged {
        ("HEAD", "Staged version")
    } else if doc.untracked {
        ("(nothing)", "New file")
    } else {
        ("Index", "Current version")
    };
    let title = |text: String| {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .px_2()
            .text_color(muted)
            .child(text)
    };
    let titles = div()
        .flex()
        .items_center()
        .h(px(24.))
        .flex_none()
        .border_b_1()
        .border_color(border)
        .text_xs()
        .when(unified, |d| {
            d.child(div().w(px(GUTTER_W)).flex_none())
                .child(title(format!("{} (against {old_label})", doc.path)))
        })
        .when(!unified, |d| {
            d.child(title(format!("{old_label} · {}", doc.path)))
                .child(div().w(px(GUTTER_W)).flex_none())
                .child(title(new_label.to_string()))
        });

    // ----------------------------------------------------------------- body
    // Fixed tints (background, changed words) so a change always looks the same: blue where lines
    // changed, grey for lines that went away, green for lines that are new.
    let dark = fg.l > 0.5;
    let col = move |c: Change| tint(c, dark);
    let num = |n: Option<u32>| {
        div()
            .h(px(ROW_H))
            .flex_none()
            .w(px(GUTTER_W / 2.))
            .pr_2()
            .text_right()
            .text_color(muted)
            .child(n.map(|n| n.to_string()).unwrap_or_default())
    };
    let gutter = |old: Option<u32>, new: Option<u32>, bg: Hsla| {
        div()
            .w(px(GUTTER_W))
            .h_full()
            .flex_none()
            .flex()
            .border_l_1()
            .border_r_1()
            .border_color(border)
            .bg(bg)
            .child(num(old))
            .child(num(new))
    };
    let code = |c: &Code, em: Hsla| {
        let chars: Vec<char> = c.text.chars().collect();
        let cut = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
        let mut line = div().flex().whitespace_nowrap();
        let mut at = 0;
        for &(a, b) in &c.emph {
            line = line.child(cut(at, a)).child(div().bg(em).child(cut(a, b)));
            at = b;
        }
        line.child(cut(at, chars.len()))
    };
    let pane = |content: Option<&Code>, bg: Hsla, em: Hsla| {
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .px_2()
            .flex()
            .items_center()
            .bg(bg)
            .children(content.map(|c| code(c, em)))
    };

    let body = if let Some(e) = &doc.error {
        note(e.clone())
    } else if doc.diff.binary {
        note("Binary file: nothing to show line by line.".into())
    } else if doc.rows.is_empty() {
        note("No changes.".into())
    } else {
        div()
            .id("diff-rows")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&doc.scroll)
            .font_family("monospace")
            .text_xs()
            .text_color(fg)
            .children(list.iter().map(|it| {
                let row = div().h(px(ROW_H)).flex_none().flex();
                match it {
                    Item::Sep(t) => row
                        .items_center()
                        .justify_center()
                        .bg(muted.opacity(0.1))
                        .text_color(muted)
                        .child(format!("… {t}")),
                    Item::Row {
                        no_old,
                        no_new,
                        code: c,
                    } => {
                        let none = gpui_kit::transparent_black();
                        row.child(pane(Some(c), none, none))
                            .child(gutter(*no_old, *no_new, none))
                            .child(pane(Some(c), none, none))
                    }
                    Item::Block { left, right } => {
                        let (r, a) = (left.len(), right.len());
                        let paired = r.min(a);
                        // Lines with a partner on the other side changed; the rest went away
                        // (left) or are new (right).
                        let kind = |is_left: bool, i: usize| {
                            if i < paired {
                                Change::Modified
                            } else if is_left {
                                Change::Removed
                            } else {
                                Change::Added
                            }
                        };
                        let stack = |lines: &Vec<(Option<u32>, Code)>, is_left: bool| {
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .children(lines.iter().enumerate().map(|(i, (_, c))| {
                                    let (bg, em) = col(kind(is_left, i));
                                    div()
                                        .h(px(ROW_H))
                                        .flex_none()
                                        .px_2()
                                        .flex()
                                        .items_center()
                                        .bg(bg)
                                        .child(code(c, em))
                                }))
                        };
                        let nums = |lines: &Vec<(Option<u32>, Code)>| {
                            div()
                                .absolute()
                                .top_0()
                                .flex()
                                .flex_col()
                                .children(lines.iter().map(|(n, _)| num(*n)))
                        };
                        let ribbon = col(if r > 0 && a > 0 {
                            Change::Modified
                        } else if a == 0 {
                            Change::Removed
                        } else {
                            Change::Added
                        })
                        .0;
                        let (lh, rh) = (r as f32 * ROW_H, a as f32 * ROW_H);
                        let gut = div()
                            .w(px(GUTTER_W))
                            .h_full()
                            .flex_none()
                            .relative()
                            .border_l_1()
                            .border_r_1()
                            .border_color(border)
                            .child(
                                canvas(
                                    |_, _, _| (),
                                    move |b, _, w, _| {
                                        // Left edge spans the old lines, right edge the new ones.
                                        let mut p = PathBuilder::fill();
                                        p.move_to(point(b.left(), b.top()));
                                        p.line_to(point(b.left(), b.top() + px(lh)));
                                        p.line_to(point(b.right(), b.top() + px(rh)));
                                        p.line_to(point(b.right(), b.top()));
                                        p.close();
                                        if let Ok(path) = p.build() {
                                            w.paint_path(path, ribbon);
                                        }
                                    },
                                )
                                .absolute()
                                .size_full(),
                            )
                            .child(nums(left).left_0())
                            .child(nums(right).right_0());
                        div()
                            .h(px(r.max(a) as f32 * ROW_H))
                            .flex_none()
                            .flex()
                            .child(stack(left, true))
                            .child(gut)
                            .child(stack(right, false))
                    }
                    Item::Uni {
                        old,
                        new,
                        code: c,
                        change,
                    } => {
                        let (bg, em) = col(*change);
                        row.child(gutter(*old, *new, bg))
                            .child(pane(Some(c), bg, em))
                    }
                }
            }))
            .into_any_element()
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(toolbar)
        .when(!broken, |d| d.child(titles))
        .child(body)
        .into_any_element()
}
