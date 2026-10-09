use kalem_core::{Command, CommandRegistry};
use kalem_lang::{load_dir, Os};
use kalem_test::parse_junit;
use kalem_vcs::{forge, git::GitCli, overview::ChangesOverview, VcsBackend};
use std::path::PathBuf;

/// Locate the bundled language packs.
///
/// Order: `KALEM_LANGUAGES`, `<exe dir>/languages`, `./languages`,
/// `./src/kalem-lang/languages` (running from a source checkout).
pub fn default_languages_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("KALEM_LANGUAGES") {
        return PathBuf::from(p);
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("languages"));
        }
    }
    candidates.push(PathBuf::from("languages"));
    candidates.push(PathBuf::from("src/kalem-lang/languages"));
    candidates
        .iter()
        .find(|p| p.is_dir())
        .cloned()
        .unwrap_or_else(|| PathBuf::from("languages"))
}

/// Build the registry with every built-in command.
pub fn build_registry(languages_dir: PathBuf) -> CommandRegistry {
    let mut reg = CommandRegistry::new();
    reg.register(Command::new("app.version", "Show version", |_| {
        Ok(format!("Kalem IDE {}", env!("CARGO_PKG_VERSION")))
    }));
    reg.register(Command::new(
        "languages.list",
        "List language packs",
        move |_| {
            let packs = load_dir(&languages_dir).map_err(kalem_core::CommandError::Failed)?;
            let os = Os::current();
            let lines: Vec<String> = packs
                .iter()
                .map(|p| {
                    let dbg = p.debug.as_ref().map(|d| d.adapter_for(os)).unwrap_or("-");
                    let lsp = p.lsp.as_ref().map(|l| l.command.as_str()).unwrap_or("-");
                    format!("{:<12} lsp: {:<28} debug: {}", p.language.id, lsp, dbg)
                })
                .collect();
            Ok(lines.join("\n"))
        },
    ));
    reg.register(Command::new(
        "vcs.status",
        "Git status of a folder (default: .)",
        |args| {
            let repo = PathBuf::from(args.first().map(String::as_str).unwrap_or("."));
            let st = GitCli
                .status(&repo)
                .map_err(|e| kalem_core::CommandError::Failed(e.to_string()))?;
            let mut lines = vec![format!(
                "branch: {}  upstream: {}  ahead {} / behind {}",
                st.branch.as_deref().unwrap_or("(detached)"),
                st.upstream.as_deref().unwrap_or("-"),
                st.ahead,
                st.behind
            )];
            for e in &st.entries {
                lines.push(format!("  {:?}/{:?}  {}", e.staged, e.unstaged, e.path));
            }
            Ok(lines.join("\n"))
        },
    ));
    reg.register(Command::new(
        "vcs.overview",
        "Git Changes overview of a folder (default: .)",
        |args| {
            let repo = PathBuf::from(args.first().map(String::as_str).unwrap_or("."));
            let st = GitCli
                .status(&repo)
                .map_err(|e| kalem_core::CommandError::Failed(e.to_string()))?;
            let o = ChangesOverview::from_status(&st);
            Ok(format!(
                "{} ({} ahead / {} behind), sync button: {:?}\n{}",
                o.branch.as_deref().unwrap_or("(detached)"),
                o.ahead,
                o.behind,
                o.sync_action(),
                o.changes_text()
            ))
        },
    ));
    reg.register(Command::new(
        "vcs.forge",
        "Detect the hosting platform of 'origin' (default folder: .)",
        |args| {
            let repo = PathBuf::from(args.first().map(String::as_str).unwrap_or("."));
            let url = GitCli
                .remote_url(&repo, "origin")
                .map_err(|e| kalem_core::CommandError::Failed(e.to_string()))?
                .ok_or_else(|| kalem_core::CommandError::Failed("no 'origin' remote".into()))?;
            match forge::parse_remote(&url) {
                Some(r) => Ok(format!("{:?}: {}/{} ({})", r.kind, r.owner, r.repo, r.host)),
                None => Ok(format!("unrecognized remote: {url}")),
            }
        },
    ));
    reg.register(Command::new(
        "test.report",
        "Overview of a JUnit XML test report: test.report <file.xml>",
        |args| {
            let path = args.first().ok_or_else(|| {
                kalem_core::CommandError::Failed("usage: test.report <junit.xml>".into())
            })?;
            let xml = std::fs::read_to_string(path)
                .map_err(|e| kalem_core::CommandError::Failed(format!("{path}: {e}")))?;
            let run =
                parse_junit(&xml).map_err(|e| kalem_core::CommandError::Failed(e.to_string()))?;
            Ok(run.overview(5))
        },
    ));
    reg.register(Command::new(
        "solution.show",
        "Show the solution of a folder (.kalem, .slnx, .sln or workspace): solution.show [folder]",
        |args| {
            let dir = PathBuf::from(args.first().map(String::as_str).unwrap_or("."));
            match kalem_solution::load(&dir)
                .map_err(|e| kalem_core::CommandError::Failed(e.to_string()))?
            {
                Some(s) => Ok(format!("{}\n{}", s.overview(), s.tree())),
                None => Ok("no solution file found; the folder opens as plain files".into()),
            }
        },
    ));
    reg
}
