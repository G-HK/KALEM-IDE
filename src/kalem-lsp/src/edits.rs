//! Applying LSP text edits to a string, and flattening a `WorkspaceEdit`.

use lsp_types::{DocumentChanges, OneOf, Position, TextEdit, WorkspaceEdit};

/// The byte offset of a position. Columns count UTF-16 units; a column past the end of
/// the line (not counting the line break) lands at the end of the line.
fn offset_of(text: &str, starts: &[usize], pos: &Position) -> usize {
    let Some(&start) = starts.get(pos.line as usize) else {
        return text.len();
    };
    let end = starts
        .get(pos.line as usize + 1)
        .copied()
        .unwrap_or(text.len());
    let line = text[start..end].trim_end_matches(['\n', '\r']);
    let mut units = 0u32;
    for (i, c) in line.char_indices() {
        if units >= pos.character {
            return start + i;
        }
        units += c.len_utf16() as u32;
    }
    start + line.len()
}

/// Apply edits (they refer to the original text and must not overlap).
pub fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
    let mut starts = vec![0usize];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let mut spans: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|e| {
            let a = offset_of(text, &starts, &e.range.start);
            let b = offset_of(text, &starts, &e.range.end).max(a);
            (a, b, e.new_text.as_str())
        })
        .collect();
    // Back to front keeps the earlier offsets valid; equal starts keep their order.
    spans.sort_by_key(|s| std::cmp::Reverse(s.0));
    let mut out = text.to_string();
    for (a, b, new) in spans {
        out.replace_range(a..b, new);
    }
    out
}

/// The text edits of a workspace edit, per document URI. File creation, renaming and
/// deletion are not handled (servers send them for refactorings the IDE does not offer).
pub fn flatten(edit: WorkspaceEdit) -> Vec<(String, Vec<TextEdit>)> {
    let mut out: Vec<(String, Vec<TextEdit>)> = Vec::new();
    let mut add = |uri: String, edits: Vec<TextEdit>| match out.iter_mut().find(|(u, _)| *u == uri)
    {
        Some((_, e)) => e.extend(edits),
        None => out.push((uri, edits)),
    };
    if let Some(changes) = edit.changes {
        for (uri, edits) in changes {
            add(uri.to_string(), edits);
        }
    }
    let docs = match edit.document_changes {
        Some(DocumentChanges::Edits(e)) => e,
        Some(DocumentChanges::Operations(ops)) => ops
            .into_iter()
            .filter_map(|op| match op {
                lsp_types::DocumentChangeOperation::Edit(e) => Some(e),
                _ => None,
            })
            .collect(),
        None => Vec::new(),
    };
    for d in docs {
        let edits = d
            .edits
            .into_iter()
            .map(|e| match e {
                OneOf::Left(t) => t,
                OneOf::Right(a) => a.text_edit,
            })
            .collect();
        add(d.text_document.uri.to_string(), edits);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::Range;

    fn edit(l1: u32, c1: u32, l2: u32, c2: u32, new: &str) -> TextEdit {
        TextEdit {
            range: Range::new(Position::new(l1, c1), Position::new(l2, c2)),
            new_text: new.into(),
        }
    }

    #[test]
    fn replaces_inserts_and_deletes() {
        let t = "let x = 1;\nlet y = x;\n";
        let out = apply_edits(
            t,
            &[
                edit(0, 4, 0, 5, "total"),
                edit(1, 8, 1, 9, "total"),
                edit(1, 0, 1, 0, "// \n"),
            ],
        );
        assert_eq!(out, "let total = 1;\n// \nlet y = total;\n");
        assert_eq!(apply_edits("abc", &[edit(0, 1, 0, 2, "")]), "ac");
    }

    #[test]
    fn columns_are_utf16_units() {
        // "é" is one unit, "😀" is two; the edit replaces `b`.
        let t = "é😀b\n";
        assert_eq!(apply_edits(t, &[edit(0, 3, 0, 4, "X")]), "é😀X\n");
    }

    #[test]
    fn positions_past_the_end_are_clamped() {
        assert_eq!(
            apply_edits("ab\r\ncd", &[edit(0, 99, 0, 99, "!")]),
            "ab!\r\ncd"
        );
        assert_eq!(apply_edits("ab", &[edit(7, 0, 7, 0, "!")]), "ab!");
    }

    #[test]
    fn workspace_edits_are_flattened() {
        let json = serde_json::json!({"changes": {
            "file:///a.rs": [{"range": {"start": {"line":0,"character":0},
                "end": {"line":0,"character":1}}, "newText": "x"}]},
            "documentChanges": [{"textDocument": {"uri": "file:///b.rs", "version": 1},
            "edits": [{"range": {"start": {"line":0,"character":0},
                "end": {"line":0,"character":1}}, "newText": "y"}]}]});
        let w: WorkspaceEdit = serde_json::from_value(json).unwrap();
        let f = flatten(w);
        assert_eq!(f.len(), 2);
        assert!(f
            .iter()
            .any(|(u, e)| u == "file:///b.rs" && e[0].new_text == "y"));
    }
}
