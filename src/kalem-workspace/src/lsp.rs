//! Which language server serves a file.

use crate::debug::on_path;
use crate::language_for;

/// The language server a language pack names for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspSpec {
    /// The language pack id; one server runs per pack.
    pub pack: String,
    /// Human name of the language ("Rust").
    pub language: String,
    /// The LSP `languageId` of the file ("rust", "typescriptreact", ...).
    pub language_id: String,
    pub command: String,
    pub args: Vec<String>,
}

impl LspSpec {
    /// Is the server program installed?
    pub fn installed(&self) -> bool {
        on_path(&self.command)
    }
}

/// The server for a workspace-relative path, if its language pack names one.
pub fn lsp_for(rel: &str) -> Option<LspSpec> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let ext = name.rsplit_once('.')?.1;
    let pack = kalem_lang::builtin()
        .into_iter()
        .find(|p| p.matches_extension(ext))?;
    let lsp = pack.lsp.clone()?;
    let language_id = match language_for(rel) {
        Some("tsx") => "typescriptreact".to_string(),
        Some(l) => l.to_string(),
        None => pack.language.id.clone(),
    };
    Some(LspSpec {
        pack: pack.language.id.clone(),
        language: pack.language.name.clone(),
        language_id,
        command: lsp.command,
        args: lsp.args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_files_use_rust_analyzer() {
        let s = lsp_for("src/main.rs").unwrap();
        assert_eq!(s.pack, "rust");
        assert_eq!(s.language_id, "rust");
        assert_eq!(s.command, "rust-analyzer");
    }

    #[test]
    fn python_gets_stdio_args_and_tsx_its_own_language_id() {
        assert_eq!(lsp_for("a/b.py").unwrap().args, vec!["--stdio"]);
        let t = lsp_for("ui/App.tsx");
        if let Some(t) = t {
            assert_eq!(t.language_id, "typescriptreact");
        }
    }

    #[test]
    fn files_without_a_pack_have_no_server() {
        assert!(lsp_for("README.md").is_none());
        assert!(lsp_for("Makefile").is_none());
    }
}
