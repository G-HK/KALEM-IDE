//! Visual Studio `.slnx`: a short XML solution format.
//!
//! ```xml
//! <Solution>
//!   <Folder Name="/Docs/"><File Path="README.md" /></Folder>
//!   <Project Path="src/App/App.csproj" />
//! </Solution>
//! ```

use crate::{
    guess_language, normalize_path, project_name, Project, Solution, SolutionError, SolutionFolder,
    Source,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

fn attr(e: &BytesStart, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key.as_bytes())
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

pub fn parse_slnx(text: &str, file_stem: &str) -> Result<Solution, SolutionError> {
    let mut reader = Reader::from_str(text);
    let mut folders: Vec<SolutionFolder> = Vec::new();
    let mut projects: Vec<Project> = Vec::new();
    let mut current_folder: Option<usize> = None;
    let mut saw_root = false;
    loop {
        let ev = reader
            .read_event()
            .map_err(|e| SolutionError(format!(".slnx: {e}")))?;
        match ev {
            Event::Eof => break,
            ev @ (Event::Start(_) | Event::Empty(_)) => {
                let is_empty = matches!(ev, Event::Empty(_));
                let (Event::Start(e) | Event::Empty(e)) = ev else {
                    unreachable!()
                };
                match e.name().as_ref() {
                    b"Solution" => saw_root = true,
                    b"Folder" => {
                        let name = attr(&e, "Name").unwrap_or_default();
                        let name = name.trim_matches('/').to_string();
                        folders.push(SolutionFolder {
                            name,
                            items: Vec::new(),
                        });
                        // A self-closing <Folder /> has no End event, so it must not capture
                        // the entries that follow it.
                        current_folder = (!is_empty).then(|| folders.len() - 1);
                    }
                    b"File" => {
                        if let (Some(i), Some(p)) = (current_folder, attr(&e, "Path")) {
                            folders[i].items.push(normalize_path(&p));
                        }
                    }
                    b"Project" => {
                        if let Some(p) = attr(&e, "Path") {
                            let path = normalize_path(&p);
                            projects.push(Project {
                                name: project_name(&path),
                                language: guess_language(&path),
                                path,
                                startup: false,
                                folder: current_folder.map(|i| folders[i].name.clone()),
                            });
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) if e.name().as_ref() == b"Folder" => current_folder = None,
            _ => {}
        }
    }
    if !saw_root {
        return Err(SolutionError(".slnx: no <Solution> element".into()));
    }
    Ok(Solution {
        name: file_stem.to_string(),
        source: Source::Slnx,
        projects,
        folders,
    })
}
