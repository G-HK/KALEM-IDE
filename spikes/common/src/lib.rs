//! Toolkit-neutral demo data for the UI spikes.
//!
//! Everything here comes from the real Kalem crates (explorer tree, Git Changes overview, JUnit
//! test run, solution), so both spikes render exactly the same content and the comparison is
//! about the toolkit, not the data.

use kalem_core::Explorer;
use kalem_test::{parse_junit, TestRun};
use kalem_vcs::overview::ChangesOverview;
use kalem_vcs::{ChangeKind, Status, StatusEntry};
use std::ops::Range;

pub mod highlight;

/// A file open in the editor.
pub struct DemoFile {
    pub path: &'static str,
    pub text: &'static str,
}

pub struct Demo {
    pub solution_name: String,
    pub solution_overview: String,
    pub project_count: usize,
    pub explorer: Explorer,
    pub git_status: Status,
    pub git: ChangesOverview,
    pub tests: TestRun,
    pub files: Vec<DemoFile>,
}

const REPO_FILES: &str = include_str!("../repo_files.txt");

fn entry(path: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> StatusEntry {
    StatusEntry {
        path: path.into(),
        orig_path: None,
        staged,
        unstaged,
    }
}

pub fn demo() -> Demo {
    let explorer = Explorer::from_paths(REPO_FILES.lines().filter(|l| !l.trim().is_empty()));

    let sol = kalem_solution::parse_cargo_workspace(
        include_str!("../../../Cargo.toml"),
        "kalem-ide",
    )
    .ok()
    .flatten();
    let (solution_name, solution_overview, project_count) = match &sol {
        Some(s) => (s.name.clone(), s.overview(), s.projects.len()),
        None => ("kalem-ide".into(), "no solution".into(), 0),
    };

    let git_status = Status {
        branch: Some("main".into()),
        upstream: Some("origin/main".into()),
        ahead: 2,
        behind: 0,
        entries: vec![
            entry(
                "src/kalem-solution/src/lib.rs",
                Some(ChangeKind::Modified),
                None,
            ),
            entry(
                "deployments/windows/kalem.nsi",
                Some(ChangeKind::Added),
                Some(ChangeKind::Modified),
            ),
            entry("src/kalem/src/lib.rs", None, Some(ChangeKind::Modified)),
            entry("docs/TECH_STACK.md", None, Some(ChangeKind::Untracked)),
            entry("spikes/ui-egui/src/main.rs", None, Some(ChangeKind::Untracked)),
        ],
    };
    let git = ChangesOverview::from_status(&git_status);

    let mut cases = Vec::new();
    for xml in [
        include_str!("../../../tests/fixtures/junit/pytest.xml"),
        include_str!("../../../tests/fixtures/junit/surefire.xml"),
    ] {
        if let Ok(run) = parse_junit(xml) {
            cases.extend(run.cases);
        }
    }
    let tests = TestRun { cases };

    let files = vec![
        DemoFile {
            path: "src/kalem-core/src/explorer.rs",
            text: include_str!("../../../src/kalem-core/src/explorer.rs"),
        },
        DemoFile {
            path: "src/kalem-vcs/src/overview.rs",
            text: include_str!("../../../src/kalem-vcs/src/overview.rs"),
        },
        DemoFile {
            path: "src/kalem-solution/src/lib.rs",
            text: include_str!("../../../src/kalem-solution/src/lib.rs"),
        },
    ];

    Demo {
        solution_name,
        solution_overview,
        project_count,
        explorer,
        git_status,
        git,
        tests,
        files,
    }
}

/// Syntax token classes shared by both spikes (each maps them to its own colors).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Plain,
    Keyword,
    String,
    Comment,
    Number,
    Type,
    Function,
}

pub type Span = (Range<usize>, Token);
