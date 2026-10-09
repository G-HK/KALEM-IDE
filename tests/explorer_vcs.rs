//! Explorer tree and forge detection working together on realistic input.
use kalem_core::Explorer;
use kalem_vcs::forge::{parse_remote, ForgeKind};

#[test]
fn reveal_expands_parents_of_active_file() {
    let ex = Explorer::from_paths(["src/a/b.rs", "src/a/c.rs", "README.md"]);
    let plan = ex.reveal("src/a/b.rs").unwrap();
    assert_eq!(plan.select, "src/a/b.rs");
    assert!(plan.expand.iter().any(|p| p == "src/a"));
    assert!(ex.reveal("missing.rs").is_none());
}

#[test]
fn forge_detection_for_common_hosts() {
    let cases = [
        ("https://github.com/o/r.git", ForgeKind::GitHub),
        ("git@gitlab.com:o/r.git", ForgeKind::GitLab),
        ("https://bitbucket.org/o/r.git", ForgeKind::Bitbucket),
        ("https://codeberg.org/o/r.git", ForgeKind::Codeberg),
        ("https://git.example.com/o/r.git", ForgeKind::Other),
    ];
    for (url, kind) in cases {
        assert_eq!(parse_remote(url).unwrap().kind, kind, "{url}");
    }
}
