//! JUnit parsing against realistic reports from different runners.
use kalem_test::parse_junit;

fn load(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/fixtures/junit/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn pytest_report() {
    let run = parse_junit(&load("pytest.xml")).unwrap();
    let s = run.summary();
    assert_eq!((s.total, s.passed, s.failed, s.skipped), (3, 1, 1, 1));
    assert!(!s.is_green());
    assert_eq!(run.failures().len(), 1);
}

#[test]
fn surefire_report_with_properties() {
    let run = parse_junit(&load("surefire.xml")).unwrap();
    let s = run.summary();
    assert_eq!((s.total, s.passed), (2, 2));
    assert!(s.is_green());
}
