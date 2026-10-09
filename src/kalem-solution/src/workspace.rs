//! Workspace manifests treated as implicit solutions.

use crate::{guess_language, normalize_path, Project, Solution, SolutionError, Source};

fn member(path: &str, manifest: &str) -> Project {
    let path = normalize_path(path);
    let name = path.rsplit('/').next().unwrap_or(&path).to_string();
    Project {
        name,
        language: guess_language(manifest),
        path,
        startup: false,
        folder: None,
    }
}

/// `[workspace] members = [...]` of a Cargo.toml. `Ok(None)` if the manifest is not a workspace.
pub fn parse_cargo_workspace(text: &str, folder: &str) -> Result<Option<Solution>, SolutionError> {
    let v: toml::Value =
        toml::from_str(text).map_err(|e| SolutionError(format!("Cargo.toml: {e}")))?;
    let Some(members) = v
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    else {
        return Ok(None);
    };
    Ok(Some(Solution {
        name: folder.to_string(),
        source: Source::CargoWorkspace,
        projects: members
            .iter()
            .filter_map(|m| m.as_str())
            .map(|m| member(m, "Cargo.toml"))
            .collect(),
        folders: Vec::new(),
    }))
}

/// `"workspaces": [...]` (or `{ "packages": [...] }`) of a package.json.
pub fn parse_package_json(text: &str, folder: &str) -> Result<Option<Solution>, SolutionError> {
    // Minimal JSON scan to avoid a JSON dependency for one field.
    let Some(i) = text.find("\"workspaces\"") else {
        return Ok(None);
    };
    let rest = &text[i + "\"workspaces\"".len()..];
    let Some(open) = rest.find('[') else {
        return Err(SolutionError(
            "package.json: unsupported workspaces form".into(),
        ));
    };
    let Some(close) = rest[open..].find(']') else {
        return Err(SolutionError(
            "package.json: unterminated workspaces list".into(),
        ));
    };
    let list = &rest[open + 1..open + close];
    let projects: Vec<Project> = list
        .split('"')
        .skip(1)
        .step_by(2)
        .map(|m| member(m, "package.json"))
        .collect();
    Ok(Some(Solution {
        name: folder.to_string(),
        source: Source::NpmWorkspace,
        projects,
        folders: Vec::new(),
    }))
}

/// `use ./a` and `use ( ./a ./b )` directives of a go.work.
pub fn parse_go_work(text: &str, folder: &str) -> Solution {
    let mut projects = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        if in_block {
            if line == ")" {
                in_block = false;
            } else if !line.is_empty() {
                projects.push(member(line, "go.mod"));
            }
        } else if let Some(rest) = line.strip_prefix("use") {
            let rest = rest.trim();
            if rest == "(" {
                in_block = true;
            } else if !rest.is_empty() {
                projects.push(member(rest, "go.mod"));
            }
        }
    }
    Solution {
        name: folder.to_string(),
        source: Source::GoWork,
        projects,
        folders: Vec::new(),
    }
}
