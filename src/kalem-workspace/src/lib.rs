//! Workspace model behind the IDE window: what is open, which files it has, its git state and its
//! test results. No UI toolkit in here, so the CLI, the GUI and MCP can share it and CI can test it.

pub mod kinds;
pub mod search;
pub mod tasks;

pub use kalem_core::explorer::Node;
use kalem_core::explorer::{Explorer, RevealPlan};
use kalem_solution::Solution;
use kalem_test::{parse_junit, TestRun};
use kalem_vcs::{git::GitCli, Status, VcsBackend, VcsError};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Files bigger than this are not opened in the editor.
pub const MAX_OPEN_BYTES: u64 = 2 * 1024 * 1024;
/// Stop listing after this many files so a stray `node_modules` cannot freeze the UI.
const MAX_FILES: usize = 50_000;
/// Folders the fallback walker skips when the folder is not a git repository.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "bin",
    "obj",
    "dist",
    ".vs",
    ".idea",
    "__pycache__",
    ".venv",
];

#[derive(Debug)]
pub struct Workspace {
    /// Root folder (the parent folder when a solution file was opened).
    pub root: PathBuf,
    pub solution: Option<Solution>,
    pub explorer: Explorer,
    /// Files relative to `root`, `/` separated, sorted.
    pub files: Vec<String>,
    /// The solution file that was opened, if any (relative file name).
    pub solution_file: Option<String>,
}

#[derive(Debug)]
pub struct OpenError(pub String);

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for OpenError {}

fn is_solution_file(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .as_deref(),
        Some("kalem" | "slnx" | "sln")
    )
}

impl Workspace {
    /// Open a folder, or a `.kalem` / `.slnx` / `.sln` file (its folder becomes the root).
    pub fn open(path: &Path) -> Result<Workspace, OpenError> {
        let meta =
            std::fs::metadata(path).map_err(|e| OpenError(format!("{}: {e}", path.display())))?;
        let (root, solution_file) = if meta.is_dir() {
            (path.to_path_buf(), None)
        } else if is_solution_file(path) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            (path.parent().unwrap_or(Path::new(".")).to_path_buf(), name)
        } else {
            return Err(OpenError(format!(
                "{}: not a folder or solution file",
                path.display()
            )));
        };
        let root = if root.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            root
        };
        let solution = kalem_solution::load(&root).map_err(|e| OpenError(e.0))?;
        let files = list_files(&root);
        let explorer = Explorer::from_paths(files.iter().map(String::as_str));
        Ok(Workspace {
            root,
            solution,
            explorer,
            files,
            solution_file,
        })
    }

    /// Display name: the solution name, else the folder name.
    pub fn name(&self) -> String {
        if let Some(s) = &self.solution {
            return s.name.clone();
        }
        self.root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone())
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "workspace".into())
    }

    /// Project folders (workspace globs expanded, project files mapped to their folder).
    pub fn project_dirs(&self) -> std::collections::BTreeSet<String> {
        let paths: Vec<String> = self
            .solution
            .as_ref()
            .map(|s| s.projects.iter().map(|p| p.path.clone()).collect())
            .unwrap_or_default();
        kinds::project_dirs(&paths, &self.files)
    }

    pub fn project_count(&self) -> usize {
        let n = self.project_dirs().len();
        if n > 0 {
            n
        } else {
            self.solution.as_ref().map_or(0, |s| s.projects.len())
        }
    }

    /// Read a file for the editor. Refuses big and binary files.
    pub fn read_file(&self, rel: &str) -> Result<String, String> {
        let p = self.root.join(rel);
        let meta = std::fs::metadata(&p).map_err(|e| format!("{rel}: {e}"))?;
        if meta.len() > MAX_OPEN_BYTES {
            return Err(format!(
                "{rel}: file is larger than {} MB",
                MAX_OPEN_BYTES >> 20
            ));
        }
        let bytes = std::fs::read(&p).map_err(|e| format!("{rel}: {e}"))?;
        if bytes.contains(&0) {
            return Err(format!("{rel}: binary file"));
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn write_file(&self, rel: &str, text: &str) -> Result<(), String> {
        std::fs::write(self.root.join(rel), text).map_err(|e| format!("{rel}: {e}"))
    }

    /// Git status of the root, `Err` when it is not a repository or git is missing.
    pub fn git_status(&self) -> Result<Status, VcsError> {
        GitCli.status(&self.root)
    }

    /// "Sync with active document": what the Explorer must expand and select.
    pub fn reveal(&self, rel: &str) -> Option<RevealPlan> {
        self.explorer.reveal(rel)
    }

    /// Test results from JUnit XML files found in the workspace.
    pub fn test_results(&self) -> TestRun {
        let mut cases = Vec::new();
        for rel in self.files.iter().filter(|f| looks_like_junit(f)).take(50) {
            let p = self.root.join(rel);
            if std::fs::metadata(&p).map_or(true, |m| m.len() > 5 << 20) {
                continue;
            }
            if let Ok(xml) = std::fs::read_to_string(&p) {
                if let Ok(run) = parse_junit(&xml) {
                    cases.extend(run.cases);
                }
            }
        }
        TestRun { cases }
    }
}

/// JUnit report file names used by common runners (surefire `TEST-*.xml`, pytest `--junitxml`, ...).
pub fn looks_like_junit(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel).to_lowercase();
    name.ends_with(".xml")
        && (name.starts_with("test-")
            || name.contains("junit")
            || name.contains("pytest")
            || name.contains("surefire")
            || name == "results.xml"
            || name == "report.xml")
}

/// List files below `root`: `git ls-files` when it is a repository (honours .gitignore, includes new
/// untracked files), else a walk that skips build output folders.
pub fn list_files(root: &Path) -> Vec<String> {
    let mut files = git_files(root).unwrap_or_else(|| walk(root));
    files.sort();
    files.dedup();
    files.truncate(MAX_FILES);
    files
}

fn git_files(root: &Path) -> Option<Vec<String>> {
    let out = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let files: Vec<String> = out
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        // A deleted-but-tracked file is still listed by --cached; hide it.
        .filter(|f| root.join(f).is_file())
        .collect();
    Some(files)
}

fn walk(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(root.join(&rel)) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel_child = rel.join(&name);
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push(rel_child);
                }
            } else if ft.is_file() {
                out.push(rel_child.to_string_lossy().replace('\\', "/"));
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
    }
    out
}

/// Editor language for a file name, as the names the syntax highlighter knows.
/// `None` means plain text.
pub fn language_for(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_lowercase();
    if lower == "makefile" {
        return Some("make");
    }
    if lower == "cmakelists.txt" {
        return Some("cmake");
    }
    let ext = lower.rsplit_once('.')?.1;
    Some(match ext {
        "rs" => "rust",
        "py" | "pyw" => "python",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "cs" => "csharp",
        "json" | "jsonc" => "json",
        "toml" | "kalem" => "toml",
        "md" | "markdown" => "markdown",
        "yml" | "yaml" => "yaml",
        "html" | "htm" => "html",
        "css" => "css",
        "sh" | "bash" => "bash",
        "xml" | "slnx" | "csproj" | "props" | "targets" => "html",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kalem-ws-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn language_names() {
        assert_eq!(language_for("src/lib.rs"), Some("rust"));
        assert_eq!(language_for("a/B.CS"), Some("csharp"));
        assert_eq!(language_for("x.kalem"), Some("toml"));
        assert_eq!(language_for("Makefile"), Some("make"));
        assert_eq!(language_for("LICENSE"), None);
        assert_eq!(language_for("data.bin"), None);
    }

    #[test]
    fn junit_names() {
        assert!(looks_like_junit("target/surefire-reports/TEST-a.b.C.xml"));
        assert!(looks_like_junit("out/pytest.xml"));
        assert!(looks_like_junit("tests/fixtures/junit/surefire.xml"));
        assert!(!looks_like_junit("pom.xml"));
        assert!(!looks_like_junit("docs/readme.md"));
    }

    #[test]
    fn open_plain_folder_skips_build_output() {
        let d = scratch("plain");
        fs::create_dir_all(d.join("src")).unwrap();
        fs::create_dir_all(d.join("node_modules/x")).unwrap();
        fs::write(d.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(d.join("node_modules/x/i.js"), "1").unwrap();
        let ws = Workspace::open(&d).unwrap();
        assert_eq!(ws.files, vec!["src/main.rs".to_string()]);
        assert!(ws.solution.is_none());
        assert!(ws.reveal("src/main.rs").is_some());
        assert_eq!(ws.read_file("src/main.rs").unwrap(), "fn main() {}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn open_kalem_file_uses_its_folder_and_name() {
        let d = scratch("sol");
        fs::write(
            d.join("Demo.kalem"),
            "[solution]\nname = \"Demo\"\n\n[[project]]\nname = \"app\"\npath = \"app\"\nlanguage = \"rust\"\n",
        )
        .unwrap();
        fs::create_dir_all(d.join("app")).unwrap();
        fs::write(d.join("app/main.rs"), "").unwrap();
        let ws = Workspace::open(&d.join("Demo.kalem")).unwrap();
        assert_eq!(ws.root, d);
        assert_eq!(ws.solution_file.as_deref(), Some("Demo.kalem"));
        assert_eq!(ws.name(), "Demo");
        assert_eq!(ws.project_count(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn binary_and_missing_files_are_refused() {
        let d = scratch("bin");
        fs::write(d.join("a.bin"), [0u8, 1, 2]).unwrap();
        let ws = Workspace::open(&d).unwrap();
        assert!(ws.read_file("a.bin").unwrap_err().contains("binary"));
        assert!(ws.read_file("nope.txt").is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn finds_junit_results() {
        let d = scratch("junit");
        fs::write(
            d.join("pytest.xml"),
            include_str!("../../../tests/fixtures/junit/pytest.xml"),
        )
        .unwrap();
        let ws = Workspace::open(&d).unwrap();
        assert!(!ws.test_results().cases.is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn not_a_folder_or_solution() {
        let d = scratch("file");
        fs::write(d.join("a.txt"), "x").unwrap();
        assert!(Workspace::open(&d.join("a.txt")).is_err());
        assert!(Workspace::open(&d.join("missing")).is_err());
        let _ = fs::remove_dir_all(&d);
    }
}

/// Quick Open: files whose path contains the letters of `query` in order, best matches first.
/// File-name matches rank above folder matches, shorter paths above longer ones.
pub fn quick_open(files: &[String], query: &str, limit: usize) -> Vec<String> {
    let q: Vec<char> = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if q.is_empty() {
        return files.iter().take(limit).cloned().collect();
    }
    let mut scored: Vec<(i64, &String)> = files
        .iter()
        .filter_map(|f| {
            let lower = f.to_lowercase();
            let mut it = lower.chars();
            if !q.iter().all(|c| it.any(|x| x == *c)) {
                return None;
            }
            let name = lower.rsplit('/').next().unwrap_or(&lower);
            let qs: String = q.iter().collect();
            let mut score = -(f.len() as i64);
            if name.contains(&qs) {
                score += 1000;
            }
            if name.starts_with(&qs) {
                score += 500;
            }
            if lower.contains(&qs) {
                score += 200;
            }
            Some((score, f))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, f)| f.clone())
        .collect()
}

#[cfg(test)]
mod quick_tests {
    use super::quick_open;

    #[test]
    fn ranks_file_name_matches_first() {
        let files: Vec<String> = [
            "docs/explorer-notes.md",
            "src/explorer.rs",
            "src/e/x/p/l/orer.rs",
            "a.txt",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let hits = quick_open(&files, "explorer", 10);
        assert_eq!(hits[0], "src/explorer.rs");
        assert_eq!(hits[1], "docs/explorer-notes.md");
        assert!(!hits.contains(&"a.txt".to_string()));
        assert_eq!(quick_open(&files, "", 2).len(), 2);
        assert!(quick_open(&files, "zzz", 5).is_empty());
    }
}
