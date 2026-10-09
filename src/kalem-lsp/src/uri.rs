//! `file://` URIs. Servers identify documents by URI; the IDE by path.

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use std::path::{Path, PathBuf};

/// Everything except unreserved characters and `/` is escaped (so `C:` becomes `C%3A` as
/// VS Code writes it; servers decode either form).
const SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'[')
    .add(b']')
    .add(b'^')
    .add(b'|')
    .add(b'\\');

/// `/home/me/a b.rs` -> `file:///home/me/a%20b.rs`; `C:\x\y.rs` -> `file:///C:/x/y.rs`.
pub fn path_to_uri(path: &Path) -> String {
    let mut p = path.display().to_string().replace('\\', "/");
    // `\\?\C:\...` (verbatim) paths from canonicalize on Windows.
    if let Some(rest) = p.strip_prefix("//?/") {
        p = rest.to_string();
    }
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    format!("file://{}", utf8_percent_encode(&p, SET))
}

/// The reverse of [`path_to_uri`]; `None` for other schemes.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file://localhost/x` is legal; the host is dropped.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode_str(rest).decode_utf8().ok()?.to_string();
    // `/C:/x` -> `C:/x` on Windows-style paths.
    let b = decoded.as_bytes();
    let windows = b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':';
    Some(PathBuf::from(if windows {
        decoded[1..].to_string()
    } else {
        decoded
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_paths_round_trip() {
        let p = Path::new("/home/me/my project/a#b.rs");
        let u = path_to_uri(p);
        assert_eq!(u, "file:///home/me/my%20project/a%23b.rs");
        assert_eq!(uri_to_path(&u).unwrap(), p);
    }

    #[test]
    fn windows_paths_get_a_leading_slash() {
        let u = path_to_uri(Path::new(r"C:\Users\me\x.rs"));
        assert_eq!(u, "file:///C:/Users/me/x.rs");
        assert_eq!(uri_to_path(&u).unwrap(), Path::new("C:/Users/me/x.rs"));
        assert_eq!(
            path_to_uri(Path::new(r"\\?\C:\a\b.rs")),
            "file:///C:/a/b.rs"
        );
    }

    #[test]
    fn other_schemes_are_not_paths() {
        assert!(uri_to_path("https://example.com/x").is_none());
        assert!(uri_to_path("untitled:Untitled-1").is_none());
    }
}
