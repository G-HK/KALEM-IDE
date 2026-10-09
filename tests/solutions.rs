//! Solution loading from real files on disk.
use kalem_solution::{load, Source};
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/solutions")
}

/// Copy the named fixtures into a fresh temp dir so `load` sees exactly those files.
fn dir_with(tag: &str, files: &[&str]) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kalem-sol-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    for f in files {
        fs::copy(fixtures().join(f), d.join(f)).unwrap();
    }
    d
}

#[test]
fn kalem_file_wins_over_sln() {
    let d = dir_with("prio", &["Veto.kalem", "Legacy.sln", "App.slnx"]);
    let s = load(&d).unwrap().unwrap();
    assert_eq!(s.source, Source::Kalem);
    assert_eq!(s.name, "Veto");
    assert_eq!(s.projects.len(), 3);
    assert_eq!(s.languages(), ["csharp", "rust", "typescript"]);
    assert_eq!(s.startup_projects()[0].name, "Api");
    let _ = fs::remove_dir_all(d);
}

#[test]
fn slnx_before_sln() {
    let d = dir_with("slnx", &["Legacy.sln", "App.slnx"]);
    let s = load(&d).unwrap().unwrap();
    assert_eq!((s.source, s.name.as_str()), (Source::Slnx, "App"));
    assert_eq!(s.projects.len(), 2);
    let _ = fs::remove_dir_all(d);
}

#[test]
fn sln_only() {
    let d = dir_with("sln", &["Legacy.sln"]);
    let s = load(&d).unwrap().unwrap();
    assert_eq!(s.source, Source::Sln);
    assert_eq!(s.projects[0].path, "src/App/App.csproj");
    let _ = fs::remove_dir_all(d);
}

#[test]
fn folder_without_solution_is_none() {
    let d = dir_with("none", &[]);
    assert!(load(&d).unwrap().is_none());
    let _ = fs::remove_dir_all(d);
}

#[test]
fn this_repository_is_a_cargo_workspace() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let s = load(&root).unwrap().unwrap();
    assert_eq!(s.source, Source::CargoWorkspace);
    assert!(s.projects.iter().any(|p| p.path == "src/kalem"));
}
