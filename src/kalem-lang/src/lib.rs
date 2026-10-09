//! Language packs.
//!
//! A language pack is a small TOML manifest, not code. Adding a language means
//! adding a file under `languages/`, wiring together open tools (language
//! server, debug adapter, build tool).

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LanguagePack {
    pub language: Language,
    pub lsp: Option<Lsp>,
    pub debug: Option<Debug>,
    pub detect: Option<Detect>,
    pub tasks: Option<Tasks>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Language {
    pub id: String,
    pub name: String,
    pub extensions: Vec<String>,
    pub grammar: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Lsp {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DebugAdapter {
    pub adapter: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Debug {
    /// Default adapter for all platforms.
    pub adapter: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Optional per-OS overrides.
    pub windows: Option<DebugAdapter>,
    pub macos: Option<DebugAdapter>,
    pub linux: Option<DebugAdapter>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Detect {
    #[serde(default)]
    pub project_files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Tasks {
    pub build: Option<String>,
    pub run: Option<String>,
    pub test: Option<String>,
}

/// Operating systems a debug adapter can be overridden for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

impl Debug {
    /// Adapter to launch on `os`: the per-OS override if present, else the default.
    pub fn adapter_for(&self, os: Os) -> &str {
        let over = match os {
            Os::Windows => self.windows.as_ref(),
            Os::MacOs => self.macos.as_ref(),
            Os::Linux => self.linux.as_ref(),
        };
        over.map(|d| d.adapter.as_str()).unwrap_or(&self.adapter)
    }
}

impl LanguagePack {
    pub fn parse(toml_text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_text)
    }

    pub fn matches_extension(&self, ext: &str) -> bool {
        let ext = ext.trim_start_matches('.');
        self.language
            .extensions
            .iter()
            .any(|e| e.eq_ignore_ascii_case(ext))
    }
}

/// The packs that ship inside the binary, so an installed IDE needs no `languages/` folder.
pub fn builtin() -> Vec<LanguagePack> {
    [
        include_str!("../languages/cpp.toml"),
        include_str!("../languages/csharp.toml"),
        include_str!("../languages/go.toml"),
        include_str!("../languages/java.toml"),
        include_str!("../languages/kotlin.toml"),
        include_str!("../languages/python.toml"),
        include_str!("../languages/rust.toml"),
        include_str!("../languages/typescript.toml"),
    ]
    .iter()
    .map(|t| LanguagePack::parse(t).expect("built-in language pack is valid"))
    .collect()
}

/// Load every `*.toml` pack in `dir`, sorted by language id.
pub fn load_dir(dir: &Path) -> Result<Vec<LanguagePack>, String> {
    let mut packs = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let pack = LanguagePack::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        packs.push(pack);
    }
    packs.sort_by(|a, b| a.language.id.cmp(&b.language.id));
    Ok(packs)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KOTLIN: &str = r#"
[language]
id = "kotlin"
name = "Kotlin"
extensions = ["kt", "kts"]

[debug]
adapter = "kotlin-debug-adapter"
[debug.windows]
adapter = "java-debug"
"#;

    #[test]
    fn parses_and_overrides_per_os() {
        let p = LanguagePack::parse(KOTLIN).unwrap();
        assert!(p.matches_extension(".KT"));
        let d = p.debug.unwrap();
        assert_eq!(d.adapter_for(Os::Linux), "kotlin-debug-adapter");
        assert_eq!(d.adapter_for(Os::Windows), "java-debug");
    }

    #[test]
    fn bundled_packs_are_valid() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("languages");
        let packs = load_dir(&dir).unwrap();
        let ids: Vec<_> = packs.iter().map(|p| p.language.id.as_str()).collect();
        for want in [
            "cpp",
            "csharp",
            "go",
            "java",
            "kotlin",
            "python",
            "rust",
            "typescript",
        ] {
            assert!(ids.contains(&want), "missing language pack: {want}");
        }
        for p in &packs {
            assert!(
                !p.language.extensions.is_empty(),
                "{} has no extensions",
                p.language.id
            );
            assert!(p.lsp.is_some(), "{} has no lsp", p.language.id);
            assert!(p.debug.is_some(), "{} has no debug adapter", p.language.id);
        }
    }
}

#[cfg(test)]
mod builtin_tests {
    #[test]
    fn built_in_packs_parse() {
        let packs = super::builtin();
        assert_eq!(packs.len(), 8);
        assert!(packs.iter().any(|p| p.language.id == "rust"));
    }
}
