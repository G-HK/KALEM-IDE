//! Data behind the **Git Changes** panel: branch and sync state, whether the commit button is
//! enabled and what it says, and source-control badges for the Project Explorer.

use crate::{ChangeKind, Status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangesOverview {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicts: usize,
}

/// The primary sync button next to the branch picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    Fetch,
    Pull,
    Push,
    /// Both ahead and behind: pull then push.
    Sync,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitButton {
    pub enabled: bool,
    pub label: &'static str,
    /// Why it is disabled (shown as a tooltip).
    pub reason: Option<&'static str>,
}

impl ChangesOverview {
    pub fn from_status(st: &Status) -> Self {
        let mut o = ChangesOverview {
            branch: st.branch.clone(),
            upstream: st.upstream.clone(),
            ahead: st.ahead,
            behind: st.behind,
            staged: 0,
            unstaged: 0,
            untracked: 0,
            conflicts: 0,
        };
        for e in &st.entries {
            if e.staged == Some(ChangeKind::Conflicted)
                || e.unstaged == Some(ChangeKind::Conflicted)
            {
                o.conflicts += 1;
                continue;
            }
            if e.unstaged == Some(ChangeKind::Untracked) {
                o.untracked += 1;
                continue;
            }
            if e.unstaged == Some(ChangeKind::Ignored) {
                continue;
            }
            if e.staged.is_some() {
                o.staged += 1;
            }
            if e.unstaged.is_some() {
                o.unstaged += 1;
            }
        }
        o
    }

    pub fn is_clean(&self) -> bool {
        self.staged + self.unstaged + self.untracked + self.conflicts == 0
    }

    pub fn sync_action(&self) -> SyncAction {
        match (self.ahead > 0, self.behind > 0) {
            (true, true) => SyncAction::Sync,
            (true, false) => SyncAction::Push,
            (false, true) => SyncAction::Pull,
            (false, false) => SyncAction::Fetch,
        }
    }

    /// The "Changes" section text, including the empty-state message.
    pub fn changes_text(&self) -> String {
        if self.is_clean() {
            return "There are no changes in the working directory.".to_string();
        }
        let mut parts = Vec::new();
        if self.conflicts > 0 {
            parts.push(format!("{} conflicts", self.conflicts));
        }
        if self.staged > 0 {
            parts.push(format!("{} staged", self.staged));
        }
        if self.unstaged > 0 {
            parts.push(format!("{} unstaged", self.unstaged));
        }
        if self.untracked > 0 {
            parts.push(format!("{} untracked", self.untracked));
        }
        parts.join(", ")
    }

    /// State of the commit button given the message box and the Amend checkbox.
    pub fn commit_button(&self, message: &str, amend: bool) -> CommitButton {
        let label = if self.staged > 0 {
            "Commit Staged"
        } else {
            "Commit All"
        };
        let disabled = |reason| CommitButton {
            enabled: false,
            label,
            reason: Some(reason),
        };
        if self.conflicts > 0 {
            return disabled("Resolve merge conflicts first");
        }
        if self.is_clean() && !amend {
            return disabled("Nothing to commit");
        }
        // Amending may reuse the previous message; a normal commit needs one.
        if message.trim().is_empty() && !amend {
            return disabled("Enter a commit message");
        }
        CommitButton {
            enabled: true,
            label,
            reason: None,
        }
    }
}

/// Source-control badge for an explorer item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
    /// A folder that contains changed items.
    ContainsChanges,
}

fn badge_of(kind: &ChangeKind) -> Option<Badge> {
    match kind {
        ChangeKind::Modified | ChangeKind::Other => Some(Badge::Modified),
        ChangeKind::Added => Some(Badge::Added),
        ChangeKind::Deleted => Some(Badge::Deleted),
        ChangeKind::Renamed | ChangeKind::Copied => Some(Badge::Renamed),
        ChangeKind::Untracked => Some(Badge::Untracked),
        ChangeKind::Conflicted => Some(Badge::Conflicted),
        ChangeKind::Ignored => None,
    }
}

/// Badge for `path` (file or folder, `/` separated, relative to the repository root).
pub fn badge(st: &Status, path: &str) -> Option<Badge> {
    let path = path.trim_matches('/');
    if let Some(e) = st.entries.iter().find(|e| e.path == path) {
        return e.unstaged.as_ref().or(e.staged.as_ref()).and_then(badge_of);
    }
    let prefix = format!("{path}/");
    let inside = st
        .entries
        .iter()
        .any(|e| e.path.starts_with(&prefix) && e.unstaged != Some(ChangeKind::Ignored));
    inside.then_some(Badge::ContainsChanges)
}

/// One row of the changed-files tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    /// A folder; a chain of folders with nothing else in them is shown as one (`src/Pages`).
    Folder {
        /// Full path, `/` separated, no trailing slash.
        path: String,
        label: String,
        depth: usize,
        /// Changed files below it.
        files: usize,
        collapsed: bool,
    },
    File {
        path: String,
        kind: ChangeKind,
        depth: usize,
    },
}

#[derive(Default)]
struct Dir {
    dirs: std::collections::BTreeMap<String, Dir>,
    files: Vec<(String, ChangeKind)>,
}

impl Dir {
    fn count(&self) -> usize {
        self.files.len() + self.dirs.values().map(Dir::count).sum::<usize>()
    }
}

/// Changed files as a folder tree: folders first, then files, both sorted. `is_collapsed` says
/// which folders (by full path) are closed; their content is left out.
pub fn change_tree(
    files: &[(String, ChangeKind)],
    is_collapsed: &dyn Fn(&str) -> bool,
) -> Vec<TreeRow> {
    let mut root = Dir::default();
    for (path, kind) in files {
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        let Some((_, dirs)) = parts.split_last() else {
            continue;
        };
        let mut cur = &mut root;
        for d in dirs {
            cur = cur.dirs.entry((*d).to_string()).or_default();
        }
        cur.files.push((path.clone(), *kind));
    }
    fn emit(
        dir: &Dir,
        prefix: &str,
        depth: usize,
        is_collapsed: &dyn Fn(&str) -> bool,
        out: &mut Vec<TreeRow>,
    ) {
        for (name, child) in &dir.dirs {
            let (mut label, mut full, mut node) = (name.clone(), join(prefix, name), child);
            while node.files.is_empty() && node.dirs.len() == 1 {
                let (n, c) = node.dirs.iter().next().expect("one child");
                label = format!("{label}/{n}");
                full = join(&full, n);
                node = c;
            }
            let collapsed = is_collapsed(&full);
            out.push(TreeRow::Folder {
                path: full.clone(),
                label,
                depth,
                files: node.count(),
                collapsed,
            });
            if !collapsed {
                emit(node, &full, depth + 1, is_collapsed, out);
            }
        }
        let mut files = dir.files.clone();
        files.sort_by_key(|f| f.0.to_lowercase());
        for (path, kind) in files {
            out.push(TreeRow::File { path, kind, depth });
        }
    }
    fn join(prefix: &str, name: &str) -> String {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        }
    }
    let mut out = Vec::new();
    emit(&root, "", 0, is_collapsed, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::parse_status;

    fn st(raw: &[u8]) -> Status {
        parse_status(raw).unwrap()
    }

    #[test]
    fn clean_repo_matches_the_empty_state() {
        let o = ChangesOverview::from_status(&st(b"## main...origin/main\0"));
        assert!(o.is_clean());
        assert_eq!(
            o.changes_text(),
            "There are no changes in the working directory."
        );
        assert_eq!(o.sync_action(), SyncAction::Fetch);
        let b = o.commit_button("msg", false);
        assert!(!b.enabled);
        assert_eq!(b.reason, Some("Nothing to commit"));
        // Amend is allowed on a clean tree (reword the last commit).
        assert!(o.commit_button("", true).enabled);
    }

    #[test]
    fn counts_and_sync_action() {
        let o = ChangesOverview::from_status(&st(
            b"## main...origin/main [ahead 2, behind 1]\0M  staged.rs\0 M edited.rs\0MM both.rs\0?? new.txt\0",
        ));
        assert_eq!(
            (o.staged, o.unstaged, o.untracked, o.conflicts),
            (2, 2, 1, 0)
        );
        assert_eq!(o.sync_action(), SyncAction::Sync);
        assert_eq!(o.changes_text(), "2 staged, 2 unstaged, 1 untracked");
        let b = o.commit_button("", false);
        assert_eq!(
            (b.enabled, b.reason),
            (false, Some("Enter a commit message"))
        );
        let b = o.commit_button("feat: x", false);
        assert_eq!((b.enabled, b.label), (true, "Commit Staged"));
    }

    #[test]
    fn label_is_commit_all_when_nothing_staged_and_conflicts_block() {
        let o = ChangesOverview::from_status(&st(b"## dev\0 M a.rs\0"));
        assert_eq!(o.commit_button("m", false).label, "Commit All");
        assert_eq!(o.sync_action(), SyncAction::Fetch);

        let c = ChangesOverview::from_status(&st(b"## dev\0UU x.rs\0"));
        assert_eq!(c.conflicts, 1);
        assert_eq!(
            c.commit_button("m", false).reason,
            Some("Resolve merge conflicts first")
        );

        let p = ChangesOverview::from_status(&st(b"## dev...o/dev [ahead 1]\0"));
        assert_eq!(p.sync_action(), SyncAction::Push);
        let l = ChangesOverview::from_status(&st(b"## dev...o/dev [behind 3]\0"));
        assert_eq!(l.sync_action(), SyncAction::Pull);
    }

    #[test]
    fn explorer_badges_for_files_and_folders() {
        let s = st(b"## main\0 M src/Veto/a.cs\0?? docs/new.md\0A  src/b.cs\0");
        assert_eq!(badge(&s, "src/Veto/a.cs"), Some(Badge::Modified));
        assert_eq!(badge(&s, "docs/new.md"), Some(Badge::Untracked));
        assert_eq!(badge(&s, "src/b.cs"), Some(Badge::Added));
        assert_eq!(badge(&s, "src"), Some(Badge::ContainsChanges));
        assert_eq!(badge(&s, "src/Veto"), Some(Badge::ContainsChanges));
        assert_eq!(badge(&s, "tests"), None);
        assert_eq!(badge(&s, "src/untouched.cs"), None);
    }

    #[test]
    fn change_tree_groups_by_folder_and_compacts_chains() {
        use ChangeKind::{Modified, Untracked};
        let files = vec![
            ("README.md".to_string(), Modified),
            ("src/Veto/Pages/A.razor".to_string(), Modified),
            ("src/Veto/Pages/B.razor".to_string(), Untracked),
            ("tests/t.cs".to_string(), Modified),
        ];
        let rows = change_tree(&files, &|_| false);
        let shape: Vec<String> = rows
            .iter()
            .map(|r| match r {
                TreeRow::Folder {
                    label,
                    depth,
                    files,
                    ..
                } => format!("{depth}:{label}/ ({files})"),
                TreeRow::File { path, depth, .. } => format!("{depth}:{path}"),
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                "0:src/Veto/Pages/ (2)",
                "1:src/Veto/Pages/A.razor",
                "1:src/Veto/Pages/B.razor",
                "0:tests/ (1)",
                "1:tests/t.cs",
                "0:README.md",
            ]
        );
        let closed = change_tree(&files, &|p| p == "src/Veto/Pages");
        assert_eq!(closed.len(), 4);
    }
}
