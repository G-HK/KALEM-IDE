//! The shipped language packs must all parse and declare debugging per OS.
use kalem_lang::{load_dir, Os};
use std::path::Path;

fn packs() -> Vec<kalem_lang::LanguagePack> {
    load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/kalem-lang/languages")).unwrap()
}

#[test]
fn eight_packs_load() {
    assert_eq!(packs().len(), 8);
}

#[test]
fn every_pack_has_a_debug_adapter_on_every_os() {
    for p in packs() {
        let d = p
            .debug
            .as_ref()
            .unwrap_or_else(|| panic!("{} has no debug", p.language.id));
        for os in [Os::Linux, Os::MacOs, Os::Windows] {
            assert!(!d.adapter_for(os).is_empty(), "{} on {os:?}", p.language.id);
        }
    }
}

#[test]
fn csharp_detects_slnx() {
    let cs = packs()
        .into_iter()
        .find(|p| p.language.id == "csharp")
        .unwrap();
    assert!(cs
        .detect
        .as_ref()
        .unwrap()
        .project_files
        .iter()
        .any(|f| f == "*.slnx"));
}
