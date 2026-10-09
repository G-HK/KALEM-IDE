//! Side-by-side diff of one file, shown in an editor tab.

use crate::theming::rgba;
use gpui_kit::*;
use kalem_theme::Palette;
use kalem_vcs::diff::{DiffLine, DiffRow, FileDiff, LineKind};

/// Rows beyond this are cut off (the header says so); a diff this long is better read elsewhere.
const MAX_ROWS: usize = 4000;

pub(crate) struct DiffDoc {
    pub path: String,
    pub staged: bool,
    pub untracked: bool,
    pub diff: FileDiff,
    pub rows: Vec<DiffRow>,
    pub error: Option<String>,
}

impl DiffDoc {
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
}

pub(crate) fn diff_view(
    doc: &DiffDoc,
    palette: &Palette,
    fg: Hsla,
    muted: Hsla,
    border: Hsla,
) -> AnyElement {
    let (add_c, del_c) = (rgba(palette.get("success")), rgba(palette.get("danger")));
    let header = div()
        .flex()
        .gap_3()
        .items_center()
        .px_3()
        .h(px(28.))
        .flex_none()
        .border_b_1()
        .border_color(border)
        .text_sm()
        .child(doc.path.clone())
        .child(
            div()
                .text_color(add_c)
                .child(format!("+{}", doc.diff.added)),
        )
        .child(
            div()
                .text_color(del_c)
                .child(format!("-{}", doc.diff.removed)),
        )
        .child(div().text_color(muted).text_xs().child(if doc.staged {
            "Staged: index compared with HEAD"
        } else if doc.untracked {
            "New file"
        } else {
            "Working tree compared with the index"
        }));

    let note = |text: String| div().p_3().text_color(muted).child(text).into_any_element();
    let body = if let Some(e) = &doc.error {
        note(e.clone())
    } else if doc.diff.binary {
        note("Binary file: nothing to show line by line.".into())
    } else if doc.rows.is_empty() {
        note("No changes.".into())
    } else {
        let cell = |line: &Option<DiffLine>, bg: Hsla| {
            let (no, text) = match line {
                Some(l) => (
                    l.old_no
                        .or(l.new_no)
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                    l.text.replace('\t', "    "),
                ),
                None => (String::new(), String::new()),
            };
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .overflow_hidden()
                .bg(bg)
                .child(
                    div()
                        .w(px(48.))
                        .flex_none()
                        .pr_2()
                        .text_right()
                        .text_color(muted)
                        .child(no),
                )
                .child(div().whitespace_nowrap().child(text))
        };
        let blank = muted.opacity(0.07);
        div()
            .id("diff-rows")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .font_family("monospace")
            .text_xs()
            .text_color(fg)
            .children(doc.rows.iter().take(MAX_ROWS).map(|r| {
                match r {
                    DiffRow::Hunk(h) => div()
                        .h(px(22.))
                        .flex()
                        .items_center()
                        .px_3()
                        .bg(muted.opacity(0.12))
                        .text_color(muted)
                        .child(h.clone()),
                    DiffRow::Pair { left, right } => {
                        let bg = |l: &Option<DiffLine>| match l {
                            Some(DiffLine {
                                kind: LineKind::Removed,
                                ..
                            }) => del_c.opacity(0.22),
                            Some(DiffLine {
                                kind: LineKind::Added,
                                ..
                            }) => add_c.opacity(0.22),
                            Some(_) => gpui_kit::transparent_black(),
                            None => blank,
                        };
                        // The right side shows new line numbers.
                        let right_shown = right.as_ref().map(|l| DiffLine {
                            old_no: l.new_no,
                            ..l.clone()
                        });
                        div()
                            .h(px(18.))
                            .flex()
                            .child(cell(left, bg(left)))
                            .child(div().w(px(1.)).h_full().bg(border))
                            .child(cell(&right_shown, bg(right)))
                    }
                }
            }))
            .into_any_element()
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .child(body)
        .into_any_element()
}
