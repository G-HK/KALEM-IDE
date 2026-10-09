//! What kind of thing a Solution Explorer row is, so the UI can pick an icon. No drawing here.

use std::collections::BTreeSet;

/// Short coloured label drawn on a file icon (like `YML` in Visual Studio).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    pub label: &'static str,
    /// 0xRRGGBB
    pub color: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectKind {
    Web,
    App,
    Library,
    Test,
    Generic,
}

const fn b(label: &'static str, color: u32) -> Option<Badge> {
    Some(Badge { label, color })
}

/// Badge for a file name; `None` means a plain file icon.
pub fn badge_for(name: &str) -> Option<Badge> {
    let lower = name.to_lowercase();
    match lower.as_str() {
        ".gitignore" | ".gitattributes" | ".gitmodules" => return b("GIT", 0xf05133),
        "dockerfile" => return b("DKR", 0x4a9eed),
        "makefile" => return b("MK", 0x9aa5b1),
        "cargo.lock" | "package-lock.json" | "yarn.lock" | "go.sum" => return b("LCK", 0x8b949e),
        ".editorconfig" => return b("CFG", 0x9aa5b1),
        _ => {}
    }
    if lower.starts_with("license") {
        return b("LIC", 0xd4b84a);
    }
    if lower.starts_with("readme") {
        return b("MD", 0x6aa5e8);
    }
    let ext = lower.rsplit_once('.')?.1;
    match ext {
        "yml" | "yaml" => b("YML", 0xe0524d),
        "json" | "jsonc" => b("JSON", 0xf5c542),
        "toml" => b("TML", 0xc58f5a),
        "xml" | "props" | "targets" | "config" => b("XML", 0xe8985a),
        "md" | "markdown" => b("MD", 0x6aa5e8),
        "txt" => b("TXT", 0x9aa5b1),
        "rs" => b("RS", 0xe8885a),
        "cs" => b("C#", 0x68c868),
        "csproj" | "fsproj" | "vbproj" => b("C#", 0xb48ead),
        "razor" | "cshtml" => b("RZR", 0xa78bfa),
        "sln" | "slnx" => b("SLN", 0xb48ead),
        "kalem" => b("KLM", 0xb48ead),
        "py" | "pyw" => b("PY", 0x5aa7e0),
        "js" | "mjs" | "cjs" => b("JS", 0xf1e05a),
        "jsx" => b("JSX", 0xf1e05a),
        "ts" | "mts" | "cts" => b("TS", 0x3b8eea),
        "tsx" => b("TSX", 0x3b8eea),
        "go" => b("GO", 0x29beb0),
        "java" => b("JAVA", 0xe76f00),
        "kt" | "kts" => b("KT", 0xa97bff),
        "c" => b("C", 0x5c9bd1),
        "h" => b("H", 0x5c9bd1),
        "cpp" | "cc" | "cxx" => b("C++", 0x5c9bd1),
        "hpp" | "hh" | "hxx" => b("H++", 0x5c9bd1),
        "html" | "htm" => b("HTML", 0xe8985a),
        "css" | "scss" => b("CSS", 0x5aa7e0),
        "sh" | "bash" => b("SH", 0x68c868),
        "ps1" | "psm1" => b("PS", 0x5aa7e0),
        "bat" | "cmd" => b("BAT", 0x9aa5b1),
        "nsi" | "nsh" => b("NSI", 0x9aa5b1),
        "wxs" => b("WXS", 0x9aa5b1),
        "sql" => b("SQL", 0xe8985a),
        "lock" => b("LCK", 0x8b949e),
        _ => None,
    }
}

/// Image files get a picture icon instead of a badge.
pub fn is_image(name: &str) -> bool {
    let l = name.to_lowercase();
    ["png", "jpg", "jpeg", "gif", "svg", "ico", "bmp", "webp"]
        .iter()
        .any(|e| l.ends_with(&format!(".{e}")))
}

/// Guess what a project folder is from the files below it (`files` are workspace-relative).
pub fn project_kind(dir: &str, files: &[String]) -> ProjectKind {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let has = |f: &str| files.iter().any(|p| p.strip_prefix(&prefix) == Some(f));
    let any_ext = |e: &str| {
        files
            .iter()
            .any(|p| p.starts_with(&prefix) && p.to_lowercase().ends_with(e))
    };
    let name = dir.rsplit('/').next().unwrap_or(dir).to_lowercase();
    if name == "tests"
        || name.ends_with("tests")
        || name.ends_with("test")
        || name.ends_with(".test")
        || name.ends_with("-test")
    {
        return ProjectKind::Test;
    }
    if any_ext(".razor")
        || any_ext(".cshtml")
        || has("appsettings.json")
        || has("index.html")
        || has("vite.config.ts")
    {
        return ProjectKind::Web;
    }
    if has("src/main.rs")
        || has("Program.cs")
        || has("main.go")
        || has("main.py")
        || has("__main__.py")
    {
        return ProjectKind::App;
    }
    if has("src/lib.rs") || has("lib.rs") {
        return ProjectKind::Library;
    }
    ProjectKind::Generic
}

/// Folders (relative, `/`-separated) that exist in `files`.
pub fn dirs_of(files: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for f in files {
        let mut cur = f.as_str();
        while let Some((parent, _)) = cur.rsplit_once('/') {
            if !out.insert(parent.to_string()) {
                break;
            }
            cur = parent;
        }
    }
    out
}

/// `*` wildcard match within one path segment.
pub fn glob_segment(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((pre, post)) => {
            text.len() >= pre.len() + post.len() && text.starts_with(pre) && text.ends_with(post)
        }
    }
}

/// Folders that are projects: solution entries, with workspace globs expanded and project files
/// (`Foo/Foo.csproj`) mapped to their folder.
pub fn project_dirs(project_paths: &[String], files: &[String]) -> BTreeSet<String> {
    let dirs = dirs_of(files);
    let mut out = BTreeSet::new();
    for p in project_paths {
        let p = p.trim_matches('/').trim_start_matches("./");
        if p.contains('*') {
            let (parent, pat) = p.rsplit_once('/').unwrap_or(("", p));
            for d in &dirs {
                let (dp, dn) = d.rsplit_once('/').unwrap_or(("", d));
                if dp == parent && glob_segment(pat, dn) {
                    out.insert(d.clone());
                }
            }
        } else if files.iter().any(|f| f == p) {
            out.insert(p.rsplit_once('/').map(|(d, _)| d).unwrap_or("").to_string());
        } else if dirs.contains(p) {
            out.insert(p.to_string());
        }
    }
    out.remove("");
    out
}

/// Which project a file belongs to: index into `dirs` of the deepest project folder that
/// contains `path` (used to colour editor tabs by project).
pub fn project_index(path: &str, dirs: &[String]) -> Option<usize> {
    dirs.iter()
        .enumerate()
        .filter(|(_, d)| {
            path.strip_prefix(d.as_str())
                .is_some_and(|r| r.starts_with('/'))
        })
        .max_by_key(|(_, d)| d.len())
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn badges() {
        assert_eq!(badge_for("ci.yml").unwrap().label, "YML");
        assert_eq!(badge_for("Program.cs").unwrap().label, "C#");
        assert_eq!(badge_for(".gitignore").unwrap().label, "GIT");
        assert_eq!(badge_for("LICENSE-MIT").unwrap().label, "LIC");
        assert_eq!(badge_for("Cargo.lock").unwrap().label, "LCK");
        assert!(badge_for("data.unknown").is_none());
        assert!(is_image("Logo.PNG"));
    }

    #[test]
    fn project_dirs_expand_globs_and_project_files() {
        let files = v(&[
            "src/a/Cargo.toml",
            "src/a/src/lib.rs",
            "src/b/x.rs",
            "src/kalem-core/x.rs",
            "src/kalem-vcs/x.rs",
            "tests/t.rs",
            "Veto/Veto.csproj",
            "Veto/Program.cs",
        ]);
        let got = project_dirs(
            &v(&["src/kalem-*", "tests", "Veto/Veto.csproj", "src/a"]),
            &files,
        );
        let want: BTreeSet<String> = ["src/kalem-core", "src/kalem-vcs", "tests", "Veto", "src/a"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn files_belong_to_the_deepest_project() {
        let dirs: Vec<String> = ["src/a", "src/a/inner", "web"].map(String::from).to_vec();
        assert_eq!(project_index("src/a/x.rs", &dirs), Some(0));
        assert_eq!(project_index("src/a/inner/y.rs", &dirs), Some(1));
        assert_eq!(project_index("web/Index.razor", &dirs), Some(2));
        assert_eq!(project_index("README.md", &dirs), None);
        // A sibling folder that merely starts with the same letters is not inside the project.
        assert_eq!(project_index("src/ab/x.rs", &dirs), None);
    }

    #[test]
    fn kinds() {
        let files = v(&[
            "web/Pages/Index.razor",
            "tool/src/main.rs",
            "lib/src/lib.rs",
            "misc/x.txt",
        ]);
        assert_eq!(project_kind("web", &files), ProjectKind::Web);
        assert_eq!(project_kind("tool", &files), ProjectKind::App);
        assert_eq!(project_kind("lib", &files), ProjectKind::Library);
        assert_eq!(project_kind("misc", &files), ProjectKind::Generic);
        assert_eq!(project_kind("tests", &files), ProjectKind::Test);
        assert_eq!(project_kind("Veto.Core.Tests", &files), ProjectKind::Test);
    }
}
