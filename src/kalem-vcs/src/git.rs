use crate::{Capabilities, ChangeKind, Status, StatusEntry, VcsBackend, VcsError};
use std::path::Path;
use std::process::{Command, Stdio};

/// Git through the `git` command line.
pub struct GitCli;

impl GitCli {
    /// Run git and return (stdout, stderr) on success.
    fn run_full(&self, repo: &Path, args: &[&str]) -> Result<(Vec<u8>, String), VcsError> {
        // --no-optional-locks: an IDE polling status must not fight the user's terminal for index.lock.
        // GIT_TERMINAL_PROMPT=0 + null stdin: a missing credential must fail, never hang the IDE.
        // GIT_EDITOR=true: merges/amends never open an editor nobody can see.
        let out = Command::new("git")
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(repo)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", "true")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| VcsError::NotInstalled(format!("git: {e}")))?;
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if out.status.success() {
            Ok((out.stdout, stderr))
        } else {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // `git commit` explains "nothing to commit" on stdout.
            Err(VcsError::Failed(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }))
        }
    }

    fn run(&self, repo: &Path, args: &[&str]) -> Result<Vec<u8>, VcsError> {
        self.run_full(repo, args).map(|(o, _)| o)
    }

    /// Run a write command and return a short human-readable result (stdout + stderr).
    fn run_text(&self, repo: &Path, args: &[&str]) -> Result<String, VcsError> {
        let (out, err) = self.run_full(repo, args)?;
        let out = String::from_utf8_lossy(&out).trim().to_string();
        Ok(match (out.is_empty(), err.is_empty()) {
            (true, true) => String::new(),
            (false, true) => out,
            (true, false) => err,
            (false, false) => format!("{out}\n{err}"),
        })
    }

    fn with_paths(base: &[&str], paths: &[String]) -> Vec<String> {
        let mut v: Vec<String> = base.iter().map(|s| s.to_string()).collect();
        v.push("--".into());
        v.extend(paths.iter().cloned());
        v
    }

    fn run_owned(&self, repo: &Path, args: &[String]) -> Result<String, VcsError> {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run_text(repo, &refs)
    }

    /// Stage paths (new, modified and deleted files alike).
    pub fn stage(&self, repo: &Path, paths: &[String]) -> Result<(), VcsError> {
        if paths.is_empty() {
            return Ok(());
        }
        self.run_owned(repo, &Self::with_paths(&["add", "-A"], paths))
            .map(|_| ())
    }

    pub fn stage_all(&self, repo: &Path) -> Result<(), VcsError> {
        self.run_text(repo, &["add", "-A"]).map(|_| ())
    }

    /// Remove paths from the index, keeping the working tree (works before the first commit too).
    pub fn unstage(&self, repo: &Path, paths: &[String]) -> Result<(), VcsError> {
        if paths.is_empty() {
            return Ok(());
        }
        self.run_owned(repo, &Self::with_paths(&["reset", "-q"], paths))
            .map(|_| ())
    }

    pub fn unstage_all(&self, repo: &Path) -> Result<(), VcsError> {
        self.run_text(repo, &["reset", "-q"]).map(|_| ())
    }

    /// Commit what is staged. With `amend` an empty message keeps the previous one.
    pub fn commit(&self, repo: &Path, message: &str, amend: bool) -> Result<String, VcsError> {
        let message = message.trim();
        let mut args = vec!["commit"];
        if amend {
            args.push("--amend");
        }
        if message.is_empty() {
            if !amend {
                return Err(VcsError::Failed("a commit message is required".into()));
            }
            args.push("--no-edit");
        } else {
            args.push("-m");
            args.push(message);
        }
        self.run_text(repo, &args)
    }

    /// First line of the last commit's message (to prefill the box when amending).
    pub fn last_commit_message(&self, repo: &Path) -> Result<String, VcsError> {
        let out = self.run(repo, &["log", "-1", "--format=%B"])?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }

    pub fn fetch(&self, repo: &Path) -> Result<String, VcsError> {
        self.run_text(repo, &["fetch", "--all", "--prune"])
    }

    /// Pull. Honors the user's `pull.rebase` / `pull.ff` config; when none is set, diverged
    /// branches are merged (git would otherwise refuse and ask the user to pick a strategy).
    pub fn pull(&self, repo: &Path) -> Result<String, VcsError> {
        let configured = |key: &str| {
            self.run(repo, &["config", "--get", key])
                .map(|o| !o.is_empty())
                .unwrap_or(false)
        };
        if configured("pull.rebase") || configured("pull.ff") {
            self.run_text(repo, &["pull", "--no-edit"])
        } else {
            self.run_text(repo, &["pull", "--no-rebase", "--no-edit"])
        }
    }

    /// Push the current branch; publishes it to `origin` when it has no upstream yet.
    pub fn push(&self, repo: &Path) -> Result<String, VcsError> {
        let status = self.status(repo)?;
        match (&status.upstream, &status.branch) {
            (None, Some(branch)) => {
                self.run_text(repo, &["push", "--set-upstream", "origin", branch])
            }
            (None, None) => Err(VcsError::Failed("no branch is checked out".into())),
            _ => self.run_text(repo, &["push"]),
        }
    }

    /// Local branches, current one flagged.
    pub fn branches(&self, repo: &Path) -> Result<Vec<Branch>, VcsError> {
        let out = self.run(
            repo,
            &[
                "for-each-ref",
                "--sort=refname",
                "--format=%(HEAD)%09%(refname:short)",
                "refs/heads",
            ],
        )?;
        Ok(String::from_utf8_lossy(&out)
            .lines()
            .filter_map(|l| {
                let (head, name) = l.split_once('\t')?;
                Some(Branch {
                    name: name.to_string(),
                    current: head == "*",
                })
            })
            .collect())
    }

    pub fn checkout(&self, repo: &Path, branch: &str) -> Result<String, VcsError> {
        self.run_text(repo, &["switch", branch])
    }

    pub fn create_branch(&self, repo: &Path, branch: &str) -> Result<String, VcsError> {
        self.run_text(repo, &["switch", "-c", branch])
    }

    pub fn stashes(&self, repo: &Path) -> Result<Vec<Stash>, VcsError> {
        let out = self.run(repo, &["stash", "list", "--format=%gd%x09%gs"])?;
        Ok(String::from_utf8_lossy(&out)
            .lines()
            .filter_map(|l| {
                let (name, msg) = l.split_once('\t')?;
                Some(Stash {
                    name: name.to_string(),
                    message: msg.to_string(),
                })
            })
            .collect())
    }

    /// Stash tracked and untracked changes.
    pub fn stash_push(&self, repo: &Path, message: &str) -> Result<String, VcsError> {
        let message = message.trim();
        if message.is_empty() {
            self.run_text(repo, &["stash", "push", "--include-untracked"])
        } else {
            self.run_text(
                repo,
                &["stash", "push", "--include-untracked", "-m", message],
            )
        }
    }

    pub fn stash_pop(&self, repo: &Path, name: &str) -> Result<String, VcsError> {
        self.run_text(repo, &["stash", "pop", name])
    }

    pub fn stash_drop(&self, repo: &Path, name: &str) -> Result<String, VcsError> {
        self.run_text(repo, &["stash", "drop", name])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// `stash@{0}`.
    pub name: String,
    pub message: String,
}

impl VcsBackend for GitCli {
    fn name(&self) -> &'static str {
        "git"
    }

    fn capabilities(&self) -> Capabilities {
        // Only what is implemented today; the rest is on the roadmap.
        Capabilities {
            status: true,
            commit: true,
            branches: true,
            push_pull: true,
            stash: true,
            ..Capabilities::default()
        }
    }

    fn status(&self, repo: &Path) -> Result<Status, VcsError> {
        let out = self.run(repo, &["status", "--porcelain=v1", "-b", "-z"])?;
        parse_status(&out)
    }

    fn remote_url(&self, repo: &Path, remote: &str) -> Result<Option<String>, VcsError> {
        match self.run(repo, &["remote", "get-url", remote]) {
            Ok(out) => Ok(Some(String::from_utf8_lossy(&out).trim().to_string())),
            Err(VcsError::Failed(_)) => Ok(None), // no such remote
            Err(e) => Err(e),
        }
    }
}

fn kind(c: u8) -> Option<ChangeKind> {
    match c {
        b' ' => None,
        b'A' => Some(ChangeKind::Added),
        b'M' | b'T' => Some(ChangeKind::Modified),
        b'D' => Some(ChangeKind::Deleted),
        b'R' => Some(ChangeKind::Renamed),
        b'C' => Some(ChangeKind::Copied),
        b'U' => Some(ChangeKind::Conflicted),
        b'?' => Some(ChangeKind::Untracked),
        b'!' => Some(ChangeKind::Ignored),
        _ => Some(ChangeKind::Other),
    }
}

/// Parse `git status --porcelain=v1 -b -z` output.
pub fn parse_status(out: &[u8]) -> Result<Status, VcsError> {
    let mut status = Status::default();
    let mut records = out.split(|b| *b == 0).filter(|r| !r.is_empty());
    while let Some(rec) = records.next() {
        let text = String::from_utf8_lossy(rec).into_owned();
        if let Some(branch_line) = text.strip_prefix("## ") {
            parse_branch_line(branch_line, &mut status);
            continue;
        }
        if rec.len() < 4 {
            return Err(VcsError::Parse(text));
        }
        let (x, y) = (rec[0], rec[1]);
        let path = text[3..].to_string();
        // Renames/copies carry the original path as the next NUL-separated record.
        let orig_path = if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
            records
                .next()
                .map(|r| String::from_utf8_lossy(r).into_owned())
        } else {
            None
        };
        // Both sides 'U', or A/A, D/D combos, are merge conflicts.
        let conflicted =
            x == b'U' || y == b'U' || (x == b'A' && y == b'A') || (x == b'D' && y == b'D');
        let (staged, unstaged) = if conflicted {
            (Some(ChangeKind::Conflicted), Some(ChangeKind::Conflicted))
        } else if x == b'?' && y == b'?' {
            (None, Some(ChangeKind::Untracked))
        } else if x == b'!' && y == b'!' {
            (None, Some(ChangeKind::Ignored))
        } else {
            (kind(x), kind(y))
        };
        status.entries.push(StatusEntry {
            path,
            orig_path,
            staged,
            unstaged,
        });
    }
    Ok(status)
}

fn parse_branch_line(line: &str, status: &mut Status) {
    if let Some(rest) = line.strip_prefix("No commits yet on ") {
        status.branch = Some(rest.to_string());
        return;
    }
    if line.starts_with("HEAD (no branch)") {
        status.branch = None;
        return;
    }
    // "main...origin/main [ahead 1, behind 2]" or just "main"
    let (names, tracking) = match line.split_once(" [") {
        Some((n, t)) => (n, Some(t.trim_end_matches(']'))),
        None => (line, None),
    };
    match names.split_once("...") {
        Some((b, u)) => {
            status.branch = Some(b.to_string());
            status.upstream = Some(u.to_string());
        }
        None => status.branch = Some(names.to_string()),
    }
    if let Some(t) = tracking {
        for part in t.split(", ") {
            if let Some(n) = part.strip_prefix("ahead ") {
                status.ahead = n.parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                status.behind = n.parse().unwrap_or(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn parses_branch_and_tracking() {
        let s = parse_status(b"## main...origin/main [ahead 2, behind 1]\0").unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind), (2, 1));

        let s = parse_status(b"## No commits yet on dev\0").unwrap();
        assert_eq!(s.branch.as_deref(), Some("dev"));

        let s = parse_status(b"## HEAD (no branch)\0").unwrap();
        assert_eq!(s.branch, None);
    }

    #[test]
    fn parses_entries_including_rename_and_conflict() {
        let raw = b"## main\0 M src/a.rs\0A  b.rs\0R  new.rs\0old.rs\0?? c.txt\0UU conflict.rs\0";
        let s = parse_status(raw).unwrap();
        assert_eq!(s.entries.len(), 5);
        assert_eq!(s.entries[0].unstaged, Some(ChangeKind::Modified));
        assert_eq!(s.entries[0].staged, None);
        assert_eq!(s.entries[1].staged, Some(ChangeKind::Added));
        assert_eq!(s.entries[2].orig_path.as_deref(), Some("old.rs"));
        assert_eq!(s.entries[3].unstaged, Some(ChangeKind::Untracked));
        assert_eq!(s.entries[4].staged, Some(ChangeKind::Conflicted));
    }

    fn temp_repo() -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kalem-vcs-test-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?} failed");
    }

    #[test]
    fn status_of_a_real_repository() {
        let dir = temp_repo();
        git(&dir, &["init", "-q", "-b", "main"]);
        fs::write(dir.join("a.txt"), "one").unwrap();
        git(&dir, &["add", "a.txt"]);
        git(&dir, &["commit", "-q", "-m", "first"]);
        fs::write(dir.join("a.txt"), "two").unwrap();
        fs::write(dir.join("new.txt"), "x").unwrap();

        let s = GitCli.status(&dir).unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        let a = s.entries.iter().find(|e| e.path == "a.txt").unwrap();
        assert_eq!(a.unstaged, Some(ChangeKind::Modified));
        let n = s.entries.iter().find(|e| e.path == "new.txt").unwrap();
        assert_eq!(n.unstaged, Some(ChangeKind::Untracked));

        assert_eq!(GitCli.remote_url(&dir, "origin").unwrap(), None);
        git(
            &dir,
            &[
                "remote",
                "add",
                "origin",
                "https://git.example.com/me/kalem-ide.git",
            ],
        );
        assert_eq!(
            GitCli.remote_url(&dir, "origin").unwrap().as_deref(),
            Some("https://git.example.com/me/kalem-ide.git")
        );
        fs::remove_dir_all(&dir).ok();
    }

    fn repo_with_identity() -> PathBuf {
        let dir = temp_repo();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.name", "Test"]);
        git(&dir, &["config", "user.email", "t@example.com"]);
        git(&dir, &["config", "commit.gpgsign", "false"]);
        dir
    }

    #[test]
    fn stage_unstage_and_commit() {
        let dir = repo_with_identity();
        fs::write(dir.join("a.txt"), "one").unwrap();
        fs::write(dir.join("b.txt"), "two").unwrap();

        // Before the first commit: stage, unstage, stage again.
        GitCli.stage(&dir, &["a.txt".into()]).unwrap();
        let s = GitCli.status(&dir).unwrap();
        let a = s.entries.iter().find(|e| e.path == "a.txt").unwrap();
        assert_eq!(a.staged, Some(ChangeKind::Added));
        GitCli.unstage(&dir, &["a.txt".into()]).unwrap();
        let s = GitCli.status(&dir).unwrap();
        let a = s.entries.iter().find(|e| e.path == "a.txt").unwrap();
        assert_eq!(a.staged, None);

        // An empty message is refused; nothing staged is refused by git.
        assert!(GitCli.commit(&dir, "  ", false).is_err());
        assert!(GitCli.commit(&dir, "nothing", false).is_err());

        GitCli.stage_all(&dir).unwrap();
        GitCli.commit(&dir, "first", false).unwrap();
        assert_eq!(GitCli.last_commit_message(&dir).unwrap(), "first");
        assert!(GitCli.status(&dir).unwrap().entries.is_empty());

        // Deleting a file stages as a deletion.
        fs::remove_file(dir.join("b.txt")).unwrap();
        GitCli.stage(&dir, &["b.txt".into()]).unwrap();
        let s = GitCli.status(&dir).unwrap();
        assert_eq!(s.entries[0].staged, Some(ChangeKind::Deleted));

        // Amend with an empty message keeps the old one; with a message it replaces it.
        GitCli.commit(&dir, "", true).unwrap();
        assert_eq!(GitCli.last_commit_message(&dir).unwrap(), "first");
        GitCli.commit(&dir, "better", true).unwrap();
        assert_eq!(GitCli.last_commit_message(&dir).unwrap(), "better");

        fs::write(dir.join("a.txt"), "changed").unwrap();
        GitCli.stage_all(&dir).unwrap();
        GitCli.unstage_all(&dir).unwrap();
        let s = GitCli.status(&dir).unwrap();
        assert_eq!(s.entries[0].staged, None);
        assert_eq!(s.entries[0].unstaged, Some(ChangeKind::Modified));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn paths_with_spaces_and_dashes_are_safe() {
        let dir = repo_with_identity();
        fs::write(dir.join("-weird name.txt"), "x").unwrap();
        GitCli.stage(&dir, &["-weird name.txt".into()]).unwrap();
        GitCli.commit(&dir, "m", false).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn branches_and_checkout() {
        let dir = repo_with_identity();
        fs::write(dir.join("a.txt"), "1").unwrap();
        GitCli.stage_all(&dir).unwrap();
        GitCli.commit(&dir, "init", false).unwrap();

        GitCli.create_branch(&dir, "feature").unwrap();
        let b = GitCli.branches(&dir).unwrap();
        assert_eq!(b.len(), 2);
        assert!(b.iter().any(|b| b.name == "feature" && b.current));
        GitCli.checkout(&dir, "main").unwrap();
        let b = GitCli.branches(&dir).unwrap();
        assert!(b.iter().any(|b| b.name == "main" && b.current));
        assert!(GitCli.checkout(&dir, "does-not-exist").is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stash_round_trip_includes_untracked() {
        let dir = repo_with_identity();
        fs::write(dir.join("a.txt"), "1").unwrap();
        GitCli.stage_all(&dir).unwrap();
        GitCli.commit(&dir, "init", false).unwrap();

        fs::write(dir.join("a.txt"), "2").unwrap();
        fs::write(dir.join("new.txt"), "n").unwrap();
        GitCli.stash_push(&dir, "wip").unwrap();
        assert!(GitCli.status(&dir).unwrap().entries.is_empty());
        let list = GitCli.stashes(&dir).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "stash@{0}");
        assert!(list[0].message.contains("wip"));

        GitCli.stash_pop(&dir, &list[0].name).unwrap();
        assert!(GitCli.stashes(&dir).unwrap().is_empty());
        assert_eq!(GitCli.status(&dir).unwrap().entries.len(), 2);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn push_publishes_then_fetch_and_pull_follow() {
        let root = temp_repo();
        let remote = root.join("remote.git");
        fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "-q", "--bare", "-b", "main"]);
        let url = remote.to_string_lossy().to_string();

        let a = root.join("a");
        let b = root.join("b");
        fs::create_dir_all(&a).unwrap();
        git(&a, &["init", "-q", "-b", "main"]);
        git(&a, &["config", "user.name", "Test"]);
        git(&a, &["config", "user.email", "t@example.com"]);
        git(&a, &["config", "commit.gpgsign", "false"]);
        git(&a, &["remote", "add", "origin", &url]);
        fs::write(a.join("f.txt"), "1").unwrap();
        GitCli.stage_all(&a).unwrap();
        GitCli.commit(&a, "one", false).unwrap();

        // First push sets the upstream.
        GitCli.push(&a).unwrap();
        let s = GitCli.status(&a).unwrap();
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind), (0, 0));

        // Clone, commit, push from the clone; the original sees it after fetch.
        git(&root, &["clone", "-q", &url, "b"]);
        git(&b, &["config", "user.name", "Test"]);
        git(&b, &["config", "user.email", "t@example.com"]);
        git(&b, &["config", "commit.gpgsign", "false"]);
        fs::write(b.join("g.txt"), "2").unwrap();
        GitCli.stage_all(&b).unwrap();
        GitCli.commit(&b, "two", false).unwrap();
        GitCli.push(&b).unwrap();

        GitCli.fetch(&a).unwrap();
        assert_eq!(GitCli.status(&a).unwrap().behind, 1);
        GitCli.pull(&a).unwrap();
        let s = GitCli.status(&a).unwrap();
        assert_eq!((s.ahead, s.behind), (0, 0));
        assert!(a.join("g.txt").exists());

        // Diverged branches (one local commit, one remote commit) merge instead of failing.
        fs::write(a.join("local.txt"), "l").unwrap();
        GitCli.stage_all(&a).unwrap();
        GitCli.commit(&a, "local", false).unwrap();
        fs::write(b.join("h.txt"), "3").unwrap();
        GitCli.stage_all(&b).unwrap();
        GitCli.commit(&b, "three", false).unwrap();
        GitCli.push(&b).unwrap();
        GitCli.fetch(&a).unwrap();
        let s = GitCli.status(&a).unwrap();
        assert_eq!((s.ahead, s.behind), (1, 1));
        GitCli.pull(&a).unwrap();
        let s = GitCli.status(&a).unwrap();
        assert_eq!((s.ahead, s.behind), (2, 0)); // merge commit + local commit
        GitCli.push(&a).unwrap();
        assert_eq!(GitCli.status(&a).unwrap().ahead, 0);

        // Pushing without a remote is an error, not a hang.
        let lonely = repo_with_identity();
        fs::write(lonely.join("x"), "x").unwrap();
        GitCli.stage_all(&lonely).unwrap();
        GitCli.commit(&lonely, "x", false).unwrap();
        assert!(GitCli.push(&lonely).is_err());

        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&lonely).ok();
    }

    #[test]
    fn not_a_repo_is_an_error() {
        let dir = temp_repo();
        assert!(matches!(GitCli.status(&dir), Err(VcsError::Failed(_))));
        fs::remove_dir_all(&dir).ok();
    }
}
