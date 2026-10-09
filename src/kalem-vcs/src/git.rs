use crate::{Capabilities, ChangeKind, Status, StatusEntry, VcsBackend, VcsError};
use std::path::Path;
use std::process::Command;

/// Git through the `git` command line.
pub struct GitCli;

impl GitCli {
    fn run(&self, repo: &Path, args: &[&str]) -> Result<Vec<u8>, VcsError> {
        // --no-optional-locks: an IDE polling status must not fight the user's terminal for index.lock.
        let out = Command::new("git")
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .map_err(|e| VcsError::NotInstalled(format!("git: {e}")))?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(VcsError::Failed(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        }
    }
}

impl VcsBackend for GitCli {
    fn name(&self) -> &'static str {
        "git"
    }

    fn capabilities(&self) -> Capabilities {
        // Only what is implemented today; the rest is on the roadmap.
        Capabilities {
            status: true,
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

    #[test]
    fn not_a_repo_is_an_error() {
        let dir = temp_repo();
        assert!(matches!(GitCli.status(&dir), Err(VcsError::Failed(_))));
        fs::remove_dir_all(&dir).ok();
    }
}
