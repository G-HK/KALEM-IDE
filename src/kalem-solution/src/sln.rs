//! Classic `.sln` files: only the `Project(...) = "Name", "path", "{guid}"` lines matter here.

use crate::{
    guess_language, normalize_path, Project, Solution, SolutionError, SolutionFolder, Source,
};

const SOLUTION_FOLDER: &str = "{2150E333-8FDC-42A3-9474-1A3956D46DE8}";

fn quoted(s: &str) -> Vec<&str> {
    s.split('"').skip(1).step_by(2).collect()
}

pub fn parse_sln(text: &str, file_stem: &str) -> Result<Solution, SolutionError> {
    let text = text.trim_start_matches('\u{feff}');
    if !text.contains("Microsoft Visual Studio Solution File") && !text.contains("Project(") {
        return Err(SolutionError(".sln: not a Visual Studio solution".into()));
    }
    let mut projects = Vec::new();
    let mut folders = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("Project(") {
            continue;
        }
        // Project("{type}") = "Name", "path", "{guid}"
        let q = quoted(line);
        if q.len() < 4 {
            continue;
        }
        let (kind, name, path) = (q[0], q[1], q[2]);
        if kind.eq_ignore_ascii_case(SOLUTION_FOLDER) {
            folders.push(SolutionFolder {
                name: name.to_string(),
                items: Vec::new(),
            });
        } else {
            let path = normalize_path(path);
            projects.push(Project {
                name: name.to_string(),
                language: guess_language(&path),
                path,
                startup: false,
                folder: None,
            });
        }
    }
    Ok(Solution {
        name: file_stem.to_string(),
        source: Source::Sln,
        projects,
        folders,
    })
}
