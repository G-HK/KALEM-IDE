//! Solutions for Kalem IDE: what the Solution Explorer shows at its root.
//!
//! A solution is a named set of projects and virtual folders. It comes from, in priority order:
//!
//! 1. a `<Name>.kalem` file (TOML, Kalem's own format, works across languages),
//! 2. a Visual Studio `.slnx` or `.sln`,
//! 3. a workspace manifest (`Cargo.toml` `[workspace]`, `package.json` `workspaces`, `go.work`).
//!
//! If none exists the folder is simply opened and projects are auto-detected later.
//! All parsers here are pure (text in, [`Solution`] out); [`load`] does the file lookup.

mod kalem_file;
mod sln;
mod slnx;
mod workspace;

pub use kalem_file::parse_kalem;
pub use sln::parse_sln;
pub use slnx::parse_slnx;
pub use workspace::{parse_cargo_workspace, parse_go_work, parse_package_json};

use std::path::Path;

/// Where a solution definition came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Kalem,
    Slnx,
    Sln,
    CargoWorkspace,
    NpmWorkspace,
    GoWork,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    /// Path relative to the solution file, always with `/` separators. May contain `*` globs
    /// for workspace manifests.
    pub path: String,
    /// Language pack id (`csharp`, `rust`, ...) when it can be told from the path.
    pub language: Option<String>,
    /// Project F5 starts.
    pub startup: bool,
    /// Solution folder this project is shown under, if any.
    pub folder: Option<String>,
}

/// A virtual folder in the explorer holding loose files (docs, scripts, ...).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SolutionFolder {
    pub name: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Solution {
    pub name: String,
    pub source: Source,
    pub projects: Vec<Project>,
    pub folders: Vec<SolutionFolder>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolutionError(pub String);

impl std::fmt::Display for SolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for SolutionError {}

impl Solution {
    pub fn startup_projects(&self) -> Vec<&Project> {
        self.projects.iter().filter(|p| p.startup).collect()
    }

    /// Distinct languages used, sorted.
    pub fn languages(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .projects
            .iter()
            .filter_map(|p| p.language.clone())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Short overview for the top of the explorer.
    pub fn overview(&self) -> String {
        let langs = self.languages();
        format!(
            "Solution '{}' ({:?}): {} projects, {} folders, languages: {}",
            self.name,
            self.source,
            self.projects.len(),
            self.folders.len(),
            if langs.is_empty() {
                "-".to_string()
            } else {
                langs.join(", ")
            }
        )
    }

    /// Indented tree for the CLI and tests: folders first, then projects.
    pub fn tree(&self) -> String {
        let mut out = vec![self.name.clone()];
        for f in &self.folders {
            out.push(format!("  [{}]", f.name));
            for i in &f.items {
                out.push(format!("    {i}"));
            }
        }
        for p in &self.projects {
            let lang = p.language.as_deref().unwrap_or("?");
            let star = if p.startup { " *" } else { "" };
            out.push(format!("  {} ({lang}){star}", p.name));
        }
        out.join("\n")
    }
}

/// Guess the language pack id from a project path (file or folder name).
pub fn guess_language(path: &str) -> Option<String> {
    let lower = path.to_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    let ext = file.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let id = match (file, ext) {
        (_, "csproj" | "fsproj" | "vbproj") => "csharp",
        ("cargo.toml", _) => "rust",
        ("package.json" | "tsconfig.json", _) => "typescript",
        ("go.mod", _) => "go",
        ("pom.xml", _) => "java",
        ("build.gradle.kts", _) => "kotlin",
        ("build.gradle", _) => "java",
        ("cmakelists.txt", _) | (_, "vcxproj") => "cpp",
        ("pyproject.toml" | "setup.py", _) => "python",
        _ => return None,
    };
    Some(id.to_string())
}

pub(crate) fn normalize_path(p: &str) -> String {
    p.replace('\\', "/")
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_string()
}

pub(crate) fn project_name(path: &str) -> String {
    let p = normalize_path(path);
    let last = p.rsplit('/').next().unwrap_or(&p);
    let stem = match last.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s,
        _ => last,
    };
    // `crates/*` style globs and manifest names fall back to the parent folder.
    if matches!(
        stem.to_lowercase().as_str(),
        "cargo" | "package" | "go" | "pom" | "build"
    ) {
        if let Some(parent) = p.rsplit('/').nth(1) {
            return parent.to_string();
        }
    }
    stem.to_string()
}

/// Find and read the solution in `dir`.
///
/// Picks the first `*.kalem`, else `*.slnx`, else `*.sln`, else a workspace manifest.
/// Returns `Ok(None)` when the folder has no solution definition.
pub fn load(dir: &Path) -> Result<Option<Solution>, SolutionError> {
    let read = |p: &Path| {
        std::fs::read_to_string(p).map_err(|e| SolutionError(format!("{}: {e}", p.display())))
    };
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| SolutionError(format!("{}: {e}", dir.display())))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let stem = |n: &str| {
        n.rsplit_once('.')
            .map(|(s, _)| s.to_string())
            .unwrap_or_default()
    };
    for ext in ["kalem", "slnx", "sln"] {
        if let Some(n) = names
            .iter()
            .find(|n| n.to_lowercase().ends_with(&format!(".{ext}")))
        {
            let text = read(&dir.join(n))?;
            return match ext {
                "kalem" => parse_kalem(&text, &stem(n)).map(Some),
                "slnx" => parse_slnx(&text, &stem(n)).map(Some),
                _ => parse_sln(&text, &stem(n)).map(Some),
            };
        }
    }
    let folder_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace".into());
    if names.iter().any(|n| n == "Cargo.toml") {
        if let Some(s) = parse_cargo_workspace(&read(&dir.join("Cargo.toml"))?, &folder_name)? {
            return Ok(Some(s));
        }
    }
    if names.iter().any(|n| n == "package.json") {
        if let Some(s) = parse_package_json(&read(&dir.join("package.json"))?, &folder_name)? {
            return Ok(Some(s));
        }
    }
    if names.iter().any(|n| n == "go.work") {
        return Ok(Some(parse_go_work(
            &read(&dir.join("go.work"))?,
            &folder_name,
        )));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_languages() {
        assert_eq!(
            guess_language("src/App/App.csproj").as_deref(),
            Some("csharp")
        );
        assert_eq!(
            guess_language("backend/Cargo.toml").as_deref(),
            Some("rust")
        );
        assert_eq!(
            guess_language("web/package.json").as_deref(),
            Some("typescript")
        );
        assert_eq!(
            guess_language("svc/build.gradle.kts").as_deref(),
            Some("kotlin")
        );
        assert_eq!(guess_language("README.md"), None);
    }

    #[test]
    fn kalem_file_roundtrip() {
        let s = parse_kalem(
            "[solution]\nname='X'\n[[project]]\npath='a/Cargo.toml'\nstartup=true\n[[folder]]\nname='Docs'\nitems=['docs\\a.md']\n",
            "fallback",
        )
        .unwrap();
        assert_eq!(s.name, "X");
        assert_eq!(s.projects[0].name, "a");
        assert_eq!(s.projects[0].language.as_deref(), Some("rust"));
        assert_eq!(s.startup_projects().len(), 1);
        assert_eq!(s.folders[0].items, vec!["docs/a.md"]);
    }

    #[test]
    fn kalem_name_falls_back_to_file_stem() {
        assert_eq!(parse_kalem("", "Veto").unwrap().name, "Veto");
    }

    #[test]
    fn kalem_rejects_paths_outside_the_solution() {
        assert!(parse_kalem("[[project]]\npath='../evil'\n", "x").is_err());
        assert!(parse_kalem("[[project]]\npath='/etc'\n", "x").is_err());
        assert!(parse_kalem("this is = not [toml", "x").is_err());
    }

    #[test]
    fn slnx_projects_and_folders() {
        let s = parse_slnx(
            "<Solution><Folder Name=\"/Docs/\"><File Path=\"a.md\"/></Folder><Folder Name=\"/E/\"/><Project Path=\"x/X.csproj\"/></Solution>",
            "S",
        )
        .unwrap();
        assert_eq!(s.folders.len(), 2);
        assert_eq!(s.folders[0].name, "Docs");
        assert_eq!(
            s.projects[0].folder, None,
            "empty folder must not capture later projects"
        );
        assert!(parse_slnx("<nope/>", "S").is_err());
    }

    #[test]
    fn sln_skips_solution_folders() {
        let s = parse_sln(
            "Microsoft Visual Studio Solution File, Format Version 12.00\nProject(\"{FAE04EC0-301F-11D3-BF4B-00C04F79EFBC}\") = \"App\", \"src\\App\\App.csproj\", \"{1}\"\nEndProject\nProject(\"{2150E333-8FDC-42A3-9474-1A3956D46DE8}\") = \"Items\", \"Items\", \"{2}\"\nEndProject\n",
            "S",
        )
        .unwrap();
        assert_eq!(s.projects.len(), 1);
        assert_eq!(s.projects[0].path, "src/App/App.csproj");
        assert_eq!(s.folders.len(), 1);
        assert!(parse_sln("hello", "S").is_err());
    }

    #[test]
    fn cargo_workspace_members() {
        let s = parse_cargo_workspace(
            "[workspace]\nmembers=[\"src/kalem\",\"src/kalem-*\"]\n",
            "kalem-ide",
        )
        .unwrap()
        .unwrap();
        assert_eq!(s.projects.len(), 2);
        assert_eq!(s.projects[1].path, "src/kalem-*");
        assert!(parse_cargo_workspace("[package]\nname='a'\n", "x")
            .unwrap()
            .is_none());
    }

    #[test]
    fn npm_and_go_workspaces() {
        let s = parse_package_json(
            "{\"name\":\"r\",\"workspaces\":[\"packages/a\",\"packages/b\"]}",
            "r",
        )
        .unwrap()
        .unwrap();
        assert_eq!(s.projects.len(), 2);
        assert!(parse_package_json("{\"name\":\"r\"}", "r")
            .unwrap()
            .is_none());
        let g = parse_go_work("go 1.22\nuse (\n\t./a // x\n\t./b\n)\nuse ./c\n", "g");
        assert_eq!(
            g.projects
                .iter()
                .map(|p| p.path.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn overview_and_tree() {
        let s = parse_kalem("[[project]]\npath='a/Cargo.toml'\nstartup=true\n", "S").unwrap();
        assert!(s.overview().contains("1 projects"));
        assert!(s.tree().contains("a (rust) *"));
    }
}
