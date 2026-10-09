//! File diffs for the Git Changes panel: parse `git diff` output and lay it out side by side.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// 1-based line number in the old / new file.
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@ context` line.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub hunks: Vec<Hunk>,
    /// `git` reported a binary file; there is nothing to show line by line.
    pub binary: bool,
    pub added: usize,
    pub removed: usize,
}

/// One row of the side-by-side view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffRow {
    /// Separator carrying the hunk header.
    Hunk(String),
    Pair {
        left: Option<DiffLine>,
        right: Option<DiffLine>,
    },
}

/// `@@ -12,5 +14,7 @@ fn x` -> (12, 14).
fn hunk_starts(header: &str) -> Option<(u32, u32)> {
    let rest = header.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(' ')?;
    let new = rest.strip_prefix('+')?.split(' ').next()?;
    let start = |s: &str| s.split(',').next()?.parse::<u32>().ok();
    Some((start(old)?, start(new)?))
}

/// Parse the output of `git diff` for one file.
pub fn parse_unified(text: &str) -> FileDiff {
    let mut d = FileDiff::default();
    let (mut old_no, mut new_no) = (0u32, 0u32);
    for line in text.lines() {
        if line.starts_with("@@ ") {
            if let Some((o, n)) = hunk_starts(line) {
                old_no = o;
                new_no = n;
                d.hunks.push(Hunk {
                    header: line.to_string(),
                    lines: Vec::new(),
                });
                continue;
            }
        }
        let Some(h) = d.hunks.last_mut() else {
            // Before the first hunk: the extended header (index, ---, +++, binary notice).
            if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
                d.binary = true;
            }
            continue;
        };
        let (kind, body) = match line.as_bytes().first() {
            Some(b'+') => (LineKind::Added, &line[1..]),
            Some(b'-') => (LineKind::Removed, &line[1..]),
            Some(b' ') => (LineKind::Context, &line[1..]),
            Some(b'\\') => continue, // "\ No newline at end of file"
            None => (LineKind::Context, ""),
            _ => continue,
        };
        let (o, n) = match kind {
            LineKind::Context => {
                let r = (Some(old_no), Some(new_no));
                old_no += 1;
                new_no += 1;
                r
            }
            LineKind::Removed => {
                d.removed += 1;
                let r = (Some(old_no), None);
                old_no += 1;
                r
            }
            LineKind::Added => {
                d.added += 1;
                let r = (None, Some(new_no));
                new_no += 1;
                r
            }
        };
        h.lines.push(DiffLine {
            kind,
            old_no: o,
            new_no: n,
            text: body.to_string(),
        });
    }
    d
}

/// A file that git does not know yet: every line is an addition.
pub fn all_added(text: &str) -> FileDiff {
    let lines: Vec<DiffLine> = text
        .lines()
        .enumerate()
        .map(|(i, l)| DiffLine {
            kind: LineKind::Added,
            old_no: None,
            new_no: Some(i as u32 + 1),
            text: l.to_string(),
        })
        .collect();
    FileDiff {
        added: lines.len(),
        hunks: if lines.is_empty() {
            Vec::new()
        } else {
            vec![Hunk {
                header: format!("@@ -0,0 +1,{} @@ new file", lines.len()),
                lines,
            }]
        },
        ..Default::default()
    }
}

impl FileDiff {
    /// Side-by-side rows: context on both sides, and each run of removals paired with the
    /// run of additions that follows it.
    pub fn rows(&self) -> Vec<DiffRow> {
        let mut rows = Vec::new();
        for h in &self.hunks {
            rows.push(DiffRow::Hunk(h.header.clone()));
            let mut i = 0;
            while i < h.lines.len() {
                let l = &h.lines[i];
                match l.kind {
                    LineKind::Context => {
                        rows.push(DiffRow::Pair {
                            left: Some(l.clone()),
                            right: Some(l.clone()),
                        });
                        i += 1;
                    }
                    _ => {
                        let mut removed = Vec::new();
                        let mut added = Vec::new();
                        while i < h.lines.len() && h.lines[i].kind == LineKind::Removed {
                            removed.push(h.lines[i].clone());
                            i += 1;
                        }
                        while i < h.lines.len() && h.lines[i].kind == LineKind::Added {
                            added.push(h.lines[i].clone());
                            i += 1;
                        }
                        for k in 0..removed.len().max(added.len()) {
                            rows.push(DiffRow::Pair {
                                left: removed.get(k).cloned(),
                                right: added.get(k).cloned(),
                            });
                        }
                    }
                }
            }
        }
        rows
    }
}

/// One difference between two versions of a text: these old lines became these new lines (0-based,
/// end exclusive; an empty range means lines were only added or only removed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineChange {
    pub old: std::ops::Range<usize>,
    pub new: std::ops::Range<usize>,
}

/// The line-level differences between two texts, in order.
///
/// Lines are what an editor shows: the text split at `\n` (so a final newline leaves an empty
/// last line), without a trailing `\r`. A file with CRLF line endings therefore compares equal
/// to the same text with LF endings, and only the lines that really differ are reported.
pub fn line_changes(old: &str, new: &str) -> Vec<LineChange> {
    use similar::{DiffTag, TextDiff};
    let lines = |t: &str| -> Vec<String> {
        t.split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect()
    };
    let (a, b) = (lines(old), lines(new));
    let (a, b): (Vec<&str>, Vec<&str>) = (
        a.iter().map(String::as_str).collect(),
        b.iter().map(String::as_str).collect(),
    );
    TextDiff::from_slices(&a, &b)
        .ops()
        .iter()
        .filter_map(|op| {
            let (tag, o, n) = op.as_tag_tuple();
            (tag != DiffTag::Equal).then_some(LineChange { old: o, new: n })
        })
        .collect()
}

/// Char ranges of the changed words in a line.
pub type Ranges = Vec<(usize, usize)>;

/// Split a line into words, runs of blanks and single punctuation characters.
fn tokens(chars: &[char]) -> Vec<(usize, usize)> {
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let k = class(chars[i]);
        let mut j = i + 1;
        if k != 2 {
            while j < chars.len() && class(chars[j]) == k {
                j += 1;
            }
        }
        out.push((i, j));
        i = j;
    }
    out
}

/// The words that differ between two versions of a line (longest common subsequence of tokens),
/// as char ranges in each. Empty when nothing is shared: then the whole line is the edit and the
/// row tint already says so.
pub fn changed_words(a: &str, b: &str) -> (Ranges, Ranges) {
    let (ca, cb): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (ta, tb) = (tokens(&ca), tokens(&cb));
    if ta.len() > 300 || tb.len() > 300 {
        return (Vec::new(), Vec::new());
    }
    let same = |i: usize, j: usize| ca[ta[i].0..ta[i].1] == cb[tb[j].0..tb[j].1];
    let (n, m) = (ta.len(), tb.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if same(i, j) {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    if lcs[0][0] == 0 {
        return (Vec::new(), Vec::new());
    }
    let (mut keep_a, mut keep_b) = (vec![false; n], vec![false; m]);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if same(i, j) {
            keep_a[i] = true;
            keep_b[j] = true;
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let ranges = |t: &[(usize, usize)], keep: &[bool]| {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for (k, &(s, e)) in t.iter().enumerate() {
            if keep[k] {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.1 == s => last.1 = e,
                _ => out.push((s, e)),
            }
        }
        out
    };
    (ranges(&ta, &keep_a), ranges(&tb, &keep_b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,4 +1,5 @@ fn x\n keep\n-old one\n-old two\n+new one\n+new two\n+new three\n tail\n\\ No newline at end of file\n@@ -20,2 +21,2 @@\n ctx\n-- dashed\n";

    #[test]
    fn parses_hunks_counts_and_numbers() {
        let d = parse_unified(SAMPLE);
        assert_eq!(d.hunks.len(), 2);
        assert_eq!((d.added, d.removed), (3, 3));
        assert!(!d.binary);
        let h = &d.hunks[0];
        assert_eq!(h.header, "@@ -1,4 +1,5 @@ fn x");
        assert_eq!((h.lines[0].old_no, h.lines[0].new_no), (Some(1), Some(1)));
        assert_eq!((h.lines[1].old_no, h.lines[1].new_no), (Some(2), None));
        assert_eq!((h.lines[3].old_no, h.lines[3].new_no), (None, Some(2)));
        assert_eq!(h.lines.last().unwrap().text, "tail");
        // A removed line that itself starts with "-" is still a removal.
        let last = d.hunks[1].lines.last().unwrap();
        assert_eq!(last.kind, LineKind::Removed);
        assert_eq!(last.text, "- dashed");
        assert_eq!(last.old_no, Some(21));
    }

    #[test]
    fn side_by_side_pairs_runs() {
        let rows = parse_unified(SAMPLE).rows();
        // hunk, keep, 3 paired change rows, tail, hunk, ctx, dashed
        assert_eq!(rows.len(), 9);
        let DiffRow::Pair { left, right } = &rows[2] else {
            panic!()
        };
        assert_eq!(left.as_ref().unwrap().text, "old one");
        assert_eq!(right.as_ref().unwrap().text, "new one");
        let DiffRow::Pair { left, right } = &rows[4] else {
            panic!()
        };
        assert!(left.is_none());
        assert_eq!(right.as_ref().unwrap().text, "new three");
    }

    #[test]
    fn binary_and_new_files() {
        let d =
            parse_unified("diff --git a/x.png b/x.png\nBinary files a/x.png and b/x.png differ\n");
        assert!(d.binary);
        assert!(d.hunks.is_empty());
        let n = all_added("a\nb\n");
        assert_eq!((n.added, n.removed), (2, 0));
        assert_eq!(n.hunks[0].lines[1].new_no, Some(2));
        assert!(all_added("").hunks.is_empty());
    }
}

#[cfg(test)]
mod changed_words_tests {
    use super::*;

    #[test]
    fn changed_words_are_picked_out() {
        let (a, b) = changed_words("let x = compute(1);", "let x = compute_all(1, true);");
        // old: `compute` is not `compute_all`; new: `compute_all` and `, true` are new.
        assert_eq!(a, vec![(8, 15)]);
        assert_eq!(b, vec![(8, 19), (21, 27)]);
        // Nothing in common: the row tint says it all.
        assert_eq!(changed_words("abc", "xyz"), (Vec::new(), Vec::new()));
        assert_eq!(changed_words("same", "same"), (Vec::new(), Vec::new()));
    }
}

#[cfg(test)]
mod line_changes_tests {
    use super::*;

    #[test]
    fn line_endings_do_not_count_as_changes() {
        assert!(line_changes("a\r\nb\r\n", "a\nb\n").is_empty());
        // Only the edited line differs, though one text is CRLF and the other LF.
        assert_eq!(
            line_changes("a\r\nb\r\nc\r\n", "a\nbx\nc\n"),
            vec![LineChange {
                old: 1..2,
                new: 1..2
            }]
        );
    }

    #[test]
    fn a_missing_final_newline_is_only_the_empty_last_line() {
        // Like the editor: "a\nb\n" has an empty line 3, "a\nb" has none.
        assert_eq!(
            line_changes("a\nb\n", "a\nb"),
            vec![LineChange {
                old: 2..3,
                new: 2..2
            }]
        );
    }

    #[test]
    fn five_lines_become_one() {
        let old = "a\nb\nc\nd\ne\nf\ng\n";
        let new = "a\nX\ng\n";
        assert_eq!(
            line_changes(old, new),
            vec![LineChange {
                old: 1..6,
                new: 1..2
            }]
        );
    }

    #[test]
    fn insertions_deletions_and_equal_texts() {
        assert!(line_changes("a\nb\n", "a\nb\n").is_empty());
        assert_eq!(
            line_changes("a\nc\n", "a\nb\nc\n"),
            vec![LineChange {
                old: 1..1,
                new: 1..2
            }]
        );
        assert_eq!(
            line_changes("a\nb\nc\n", "a\nc\n"),
            vec![LineChange {
                old: 1..2,
                new: 1..1
            }]
        );
    }
}
