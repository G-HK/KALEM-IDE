//! tree-sitter based highlighting for Rust: the same engine the real editor will use.

use crate::{Span, Token};
use tree_sitter::{Node, Parser};

/// Highlight `text`; returns non-overlapping spans in byte order covering only coloured tokens.
pub fn rust(text: &str) -> Vec<Span> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    walk(tree.root_node(), &mut spans);
    spans.sort_by_key(|(r, _)| r.start);
    spans
}

fn walk(node: Node, out: &mut Vec<Span>) {
    let kind = node.kind();
    let class = match kind {
        "line_comment" | "block_comment" => Some(Token::Comment),
        "string_literal" | "raw_string_literal" | "char_literal" => Some(Token::String),
        "integer_literal" | "float_literal" | "boolean_literal" => Some(Token::Number),
        "type_identifier" | "primitive_type" => Some(Token::Type),
        _ => None,
    };
    if let Some(t) = class {
        out.push((node.byte_range(), t));
        return;
    }
    if node.child_count() == 0 {
        if !node.is_named() && kind.chars().all(|c| c.is_ascii_alphabetic() || c == '_') {
            out.push((node.byte_range(), Token::Keyword));
        } else if kind == "identifier" {
            if let Some(p) = node.parent() {
                if matches!(p.kind(), "function_item" | "call_expression")
                    && p.child_by_field_name("name")
                        .or_else(|| p.child_by_field_name("function"))
                        .map(|n| n.id())
                        == Some(node.id())
                {
                    out.push((node.byte_range(), Token::Function));
                }
            }
        } else if matches!(kind, "self" | "mutable_specifier" | "crate" | "super") {
            out.push((node.byte_range(), Token::Keyword));
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_basic_rust() {
        let src = "// hi\nfn main() { let x: u32 = 42; println!(\"s\"); }";
        let spans = rust(src);
        let has = |t: Token, s: &str| spans.iter().any(|(r, k)| *k == t && &src[r.clone()] == s);
        assert!(has(Token::Comment, "// hi"));
        assert!(has(Token::Keyword, "fn"));
        assert!(has(Token::Keyword, "let"));
        assert!(has(Token::Type, "u32"));
        assert!(has(Token::Number, "42"));
        assert!(has(Token::Function, "main"));
    }
}
