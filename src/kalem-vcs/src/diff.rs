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
