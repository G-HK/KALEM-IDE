//! Find in Files: literal text search over the workspace's files. No UI toolkit in here.

use std::path::Path;

/// Files bigger than this are skipped (generated bundles, logs).
const MAX_SEARCH_BYTES: u64 = 2 * 1024 * 1024;
/// Lines longer than this are shown shortened in the results.
const MAX_SNIPPET: usize = 200;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Path relative to the workspace root, `/` separated.
    pub path: String,
    /// 1-based line number.
    pub line: usize,
    /// 0-based column in characters.
    pub col: usize,
    /// The matching line, trimmed and shortened.
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchResult {
    pub hits: Vec<Hit>,
    pub files_searched: usize,
    /// True when `max_hits` was reached and the rest was not collected.
    pub truncated: bool,
}

/// A compiled query: literal text or a regular expression, with the case / whole-word options.
pub struct Matcher {
    re: regex::Regex,
    literal: bool,
}

impl Matcher {
    pub fn new(query: &str, opts: SearchOptions) -> Result<Matcher, String> {
        if query.is_empty() {
            return Err("nothing to search for".into());
        }
        let mut pat = if opts.regex {
            query.to_string()
        } else {
            regex::escape(query)
        };
        if opts.whole_word {
            pat = format!(r"\b(?:{pat})\b");
        }
        if !opts.case_sensitive {
            pat = format!("(?i){pat}");
        }
        regex::RegexBuilder::new(&pat)
            .size_limit(10 << 20)
            .build()
            .map(|re| Matcher {
                re,
                literal: !opts.regex,
            })
            .map_err(|e| {
                // The regex crate's message is multi-line; the first line is enough.
                let text = e.to_string();
                let line = text
                    .lines()
                    .rev()
                    .find(|l| l.trim_start().starts_with("error:"))
                    .or_else(|| text.lines().next())
                    .unwrap_or("invalid regular expression");
                format!(
                    "Invalid regular expression: {}",
                    line.trim().trim_start_matches("error: ")
                )
            })
    }

    /// Column (in characters) of every non-empty match in `line`.
    pub fn columns(&self, line: &str) -> Vec<usize> {
        self.re
            .find_iter(line)
            .filter(|m| !m.is_empty())
            .map(|m| line[..m.start()].chars().count())
            .collect()
    }

    /// `line` with every match replaced. In literal mode `with` is used as is; in regex mode
    /// `$1` / `${name}` refer to capture groups.
    pub fn replace_line(&self, line: &str, with: &str) -> String {
        if self.literal {
            self.re
                .replace_all(line, regex::NoExpand(with))
                .into_owned()
        } else {
            self.re.replace_all(line, with).into_owned()
        }
    }
}

fn snippet(line: &str) -> String {
    let t = line.trim();
    if t.chars().count() > MAX_SNIPPET {
        let mut s: String = t.chars().take(MAX_SNIPPET).collect();
        s.push('…');
        s
    } else {
        t.to_string()
    }
}

/// Read a file for searching: `None` for missing, oversized or binary files.
fn read_text(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_SEARCH_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Search `files` (relative to `root`) for `query`. Binary and oversized files are skipped.
/// Results come in file order, then line order; at most `max_hits` are collected.
/// `Err` is a message for the user (an invalid regular expression).
pub fn search(
    root: &Path,
    files: &[String],
    query: &str,
    opts: SearchOptions,
    max_hits: usize,
) -> Result<SearchResult, String> {
    let mut result = SearchResult::default();
    if query.is_empty() {
        return Ok(result);
    }
    let matcher = Matcher::new(query, opts)?;
    for rel in files {
        let Some(text) = read_text(&root.join(rel)) else {
            continue;
        };
        result.files_searched += 1;
        for (ix, line) in text.lines().enumerate() {
            for col in matcher.columns(line) {
                if result.hits.len() >= max_hits {
                    result.truncated = true;
                    return Ok(result);
                }
                result.hits.push(Hit {
                    path: rel.clone(),
                    line: ix + 1,
                    col,
                    text: snippet(line),
                });
            }
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplaceResult {
    pub files_changed: usize,
    pub replacements: usize,
    pub changed: Vec<String>,
    /// Files that could not be written: (path, reason).
    pub failed: Vec<(String, String)>,
}

/// Replace every match of `query` with `with` in `files`, line by line (line endings are kept).
/// `skip` lists files that must not be touched (unsaved editor tabs).
pub fn replace_in_files(
    root: &Path,
    files: &[String],
    query: &str,
    with: &str,
    opts: SearchOptions,
    skip: &[String],
) -> Result<ReplaceResult, String> {
    let matcher = Matcher::new(query, opts)?;
    let mut out = ReplaceResult::default();
    for rel in files {
        if skip.contains(rel) {
            continue;
        }
        let path = root.join(rel);
        let Some(text) = read_text(&path) else {
            continue;
        };
        let mut changed = 0usize;
        let mut new_text = String::with_capacity(text.len());
        for chunk in text.split_inclusive('\n') {
            let body = chunk.trim_end_matches(['\n', '\r']);
            let eol = &chunk[body.len()..];
            let n = matcher.columns(body).len();
            if n == 0 {
                new_text.push_str(chunk);
            } else {
                changed += n;
                new_text.push_str(&matcher.replace_line(body, with));
                new_text.push_str(eol);
            }
        }
        if changed == 0 {
            continue;
        }
        match std::fs::write(&path, new_text) {
            Ok(()) => {
                out.files_changed += 1;
                out.replacements += changed;
                out.changed.push(rel.clone());
            }
            Err(e) => out.failed.push((rel.clone(), e.to_string())),
        }
    }
    Ok(out)
}

/// Does `path` match a VS-style file-type filter such as `*.rs;*.toml` (empty = everything)?
pub fn matches_file_types(path: &str, filter: &str) -> bool {
    let pats: Vec<&str> = filter
        .split([';', ',', ' '])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if pats.is_empty() {
        return true;
    }
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    pats.iter().any(|p| glob(&p.to_lowercase(), &name))
}

/// `*` and `?` wildcards.
fn glob(pat: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pat.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp() -> std::path::PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("kalem-search-{}-{n}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn cols(line: &str, q: &str, opts: SearchOptions) -> Vec<usize> {
        Matcher::new(q, opts).unwrap().columns(line)
    }

    #[test]
    fn columns_options_and_whole_words() {
        let ci = SearchOptions::default();
        assert_eq!(cols("Foo foo FOO", "foo", ci), vec![0, 4, 8]);
        let cs = SearchOptions {
            case_sensitive: true,
            ..ci
        };
        assert_eq!(cols("Foo foo FOO", "foo", cs), vec![4]);
        let ww = SearchOptions {
            whole_word: true,
            ..ci
        };
        assert_eq!(cols("foobar foo _foo foo_x", "foo", ww), vec![7]);
        // Columns count characters, not bytes.
        assert_eq!(cols("héllo wörld", "wör", ci), vec![6]);
        // Literal mode does not interpret regex characters.
        assert_eq!(cols("a.c abc", "a.c", ci), vec![0]);
        assert!(Matcher::new("", ci).is_err());
    }

    #[test]
    fn regex_mode_and_errors() {
        let re = SearchOptions {
            regex: true,
            ..Default::default()
        };
        assert_eq!(cols("a.c abc", "a.c", re), vec![0, 4]);
        assert_eq!(cols("id=12 id=7", r"id=\d+", re), vec![0, 6]);
        let err = Matcher::new("(", re).err().unwrap();
        assert!(err.starts_with("Invalid regular expression"), "{err}");
        let r = search(Path::new("."), &["x".into()], "(", re, 10);
        assert!(r.is_err());
    }

    #[test]
    fn searches_files_skips_binary_and_truncates() {
        let d = temp();
        fs::write(d.join("a.rs"), "fn main() {\n    let needle = 1;\n}\n").unwrap();
        fs::write(d.join("b.txt"), "needle\nneedle needle\n").unwrap();
        fs::write(d.join("bin.dat"), b"needle\0\0").unwrap();
        let files = vec![
            "a.rs".to_string(),
            "b.txt".into(),
            "bin.dat".into(),
            "gone.txt".into(),
        ];

        let r = search(&d, &files, "needle", SearchOptions::default(), 100).unwrap();
        assert_eq!(r.hits.len(), 4);
        assert_eq!(r.files_searched, 2);
        assert!(!r.truncated);
        assert_eq!(
            (r.hits[0].path.as_str(), r.hits[0].line, r.hits[0].col),
            ("a.rs", 2, 8)
        );
        assert_eq!(r.hits[0].text, "let needle = 1;");
        assert_eq!((r.hits[3].line, r.hits[3].col), (2, 7));

        let r = search(&d, &files, "needle", SearchOptions::default(), 2).unwrap();
        assert_eq!(r.hits.len(), 2);
        assert!(r.truncated);

        let r = search(&d, &files, "", SearchOptions::default(), 10).unwrap();
        assert!(r.hits.is_empty());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn replace_keeps_line_endings_and_respects_skip() {
        let d = temp();
        fs::write(d.join("a.txt"), "foo bar\r\nfoo\nlast foo").unwrap();
        fs::write(d.join("b.txt"), "foo\n").unwrap();
        fs::write(d.join("c.txt"), "nothing\n").unwrap();
        let files: Vec<String> = ["a.txt", "b.txt", "c.txt"].map(String::from).to_vec();

        let r = replace_in_files(
            &d,
            &files,
            "foo",
            "$1",
            SearchOptions::default(),
            &["b.txt".to_string()],
        )
        .unwrap();
        assert_eq!((r.files_changed, r.replacements), (1, 3));
        // Literal mode: "$1" is inserted as is, CRLF and the missing final newline survive.
        assert_eq!(
            fs::read_to_string(d.join("a.txt")).unwrap(),
            "$1 bar\r\n$1\nlast $1"
        );
        assert_eq!(fs::read_to_string(d.join("b.txt")).unwrap(), "foo\n");

        // Regex mode expands capture groups.
        let re = SearchOptions {
            regex: true,
            ..Default::default()
        };
        let r = replace_in_files(&d, &files, r"(n)othing", "<$1>", re, &[]).unwrap();
        assert_eq!(r.replacements, 1);
        assert_eq!(fs::read_to_string(d.join("c.txt")).unwrap(), "<n>\n");
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn file_type_filters() {
        assert!(matches_file_types("src/a.rs", ""));
        assert!(matches_file_types("src/a.rs", "*.rs"));
        assert!(matches_file_types("src/a.RS", "*.toml; *.rs"));
        assert!(!matches_file_types("src/a.rs", "*.toml"));
        assert!(matches_file_types("Cargo.toml", "Cargo.*"));
        assert!(matches_file_types("a.cs", "?.cs"));
        assert!(!matches_file_types("ab.cs", "?.cs"));
    }

    #[test]
    fn long_lines_are_shortened() {
        let long = "x".repeat(500);
        assert!(snippet(&long).chars().count() <= MAX_SNIPPET + 1);
    }
}
