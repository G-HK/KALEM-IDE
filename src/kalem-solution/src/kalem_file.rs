//! The `.kalem` solution file (TOML).
//!
//! ```toml
//! [solution]
//! name = "Veto"
//!
//! [[project]]
//! path = "backend"
//!
//! [[project]]
//! path = "services/api/Api.csproj"
//! startup = true
//!
//! [[folder]]
//! name = "Docs"
//! items = ["docs/**", "README.md"]
//! ```

use crate::{
    guess_language, normalize_path, project_name, Project, Solution, SolutionError, SolutionFolder,
    Source,
};
use serde::Deserialize;

#[derive(Deserialize, Default)]
struct File {
    #[serde(default)]
    solution: Header,
    #[serde(default, rename = "project")]
    projects: Vec<RawProject>,
    #[serde(default, rename = "folder")]
    folders: Vec<RawFolder>,
}

#[derive(Deserialize, Default)]
struct Header {
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawProject {
    path: String,
    name: Option<String>,
    language: Option<String>,
    #[serde(default)]
    startup: bool,
    folder: Option<String>,
}

#[derive(Deserialize)]
struct RawFolder {
    name: String,
    #[serde(default)]
    items: Vec<String>,
}

/// Parse a `.kalem` file. `file_stem` names the solution when `[solution] name` is absent.
pub fn parse_kalem(text: &str, file_stem: &str) -> Result<Solution, SolutionError> {
    let f: File = toml::from_str(text).map_err(|e| SolutionError(format!(".kalem: {e}")))?;
    let mut projects = Vec::new();
    for p in f.projects {
        let path = normalize_path(&p.path);
        if path.is_empty() || path.starts_with('/') || path.split('/').any(|s| s == "..") {
            return Err(SolutionError(format!(
                ".kalem: project path '{}' must be relative and stay inside the solution",
                p.path
            )));
        }
        projects.push(Project {
            name: p.name.unwrap_or_else(|| project_name(&path)),
            language: p.language.or_else(|| guess_language(&path)),
            path,
            startup: p.startup,
            folder: p.folder,
        });
    }
    Ok(Solution {
        name: f.solution.name.unwrap_or_else(|| file_stem.to_string()),
        source: Source::Kalem,
        projects,
        folders: f
            .folders
            .into_iter()
            .map(|f| SolutionFolder {
                name: f.name,
                items: f.items.iter().map(|i| normalize_path(i)).collect(),
            })
            .collect(),
    })
}
