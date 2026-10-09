//! End-to-end: drive the command registry the same way the `kalem` binary does.
use kalem::build_registry;
use std::path::PathBuf;

fn languages() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/kalem-lang/languages")
}
fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/junit")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn version_command() {
    let reg = build_registry(languages());
    let out = reg.execute("app.version", &[]).unwrap();
    assert!(out.starts_with("Kalem IDE "));
}

#[test]
fn lists_all_eight_languages() {
    let reg = build_registry(languages());
    let out = reg.execute("languages.list", &[]).unwrap();
    for id in [
        "cpp",
        "csharp",
        "go",
        "java",
        "kotlin",
        "python",
        "rust",
        "typescript",
    ] {
        assert!(out.contains(id), "missing {id}");
    }
}

#[test]
fn test_report_reads_fixture() {
    let reg = build_registry(languages());
    let out = reg
        .execute("test.report", &[fixture("pytest.xml")])
        .unwrap();
    assert!(out.contains("test_div"));
}

#[test]
fn unknown_command_errors() {
    let reg = build_registry(languages());
    assert!(reg.execute("nope", &[]).is_err());
}

#[test]
fn solution_show_reads_this_repo() {
    let reg = build_registry(languages());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let out = reg
        .execute("solution.show", &[root.to_string_lossy().into_owned()])
        .unwrap();
    assert!(out.contains("CargoWorkspace"));
}
