//! Version control for Kalem IDE.
//!
//! Two separate layers, on purpose:
//!
//! * [`VcsBackend`]: the local tool that tracks files (Git first; Jujutsu,
//!   Mercurial, SVN, ... can be added behind the same trait).
//! * [`forge`]: the hosting platform behind a remote (GitHub, GitLab, Bitbucket,
//!   Azure DevOps, Gitea/Forgejo/Codeberg): pull requests, issues, CI status.
//!
//! Write operations go through the real `git` executable so the user's config,
//! credential helpers, hooks, LFS and signing behave exactly as in a terminal.

pub mod diff;
pub mod forge;
pub mod git;
pub mod overview;

use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum VcsError {
    /// The VCS executable could not be started (not installed / not on PATH).
    NotInstalled(String),
    /// The command ran but failed (stderr is included).
    Failed(String),
    /// Output could not be understood.
    Parse(String),
}

impl std::fmt::Display for VcsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VcsError::NotInstalled(s) => write!(f, "VCS tool not available: {s}"),
            VcsError::Failed(s) => write!(f, "VCS command failed: {s}"),
            VcsError::Parse(s) => write!(f, "could not parse VCS output: {s}"),
        }
    }
}

impl std::error::Error for VcsError {}

/// What a backend can do. The UI greys out anything that is `false`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub status: bool,
    pub stage_hunks: bool,
    pub commit: bool,
    pub branches: bool,
    pub push_pull: bool,
    pub stash: bool,
    pub rebase: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Conflicted,
    Ignored,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: String,
    /// Original path for renames/copies.
    pub orig_path: Option<String>,
    /// Staged state (index column), `None` if unchanged.
    pub staged: Option<ChangeKind>,
    /// Working-tree state, `None` if unchanged.
    pub unstaged: Option<ChangeKind>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub entries: Vec<StatusEntry>,
}

pub trait VcsBackend {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn status(&self, repo: &Path) -> Result<Status, VcsError>;
    /// URL of the named remote, if any.
    fn remote_url(&self, repo: &Path, remote: &str) -> Result<Option<String>, VcsError>;
}
