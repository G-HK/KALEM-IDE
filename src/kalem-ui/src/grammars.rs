//! Languages the toolkit has no grammar for, added to its highlighter at start-up.

use gpui_kit::component::highlighter::{LanguageConfig, LanguageRegistry};

/// Razor (`.razor`, `.cshtml`): the Razor grammar for directives and C# blocks, with the C#
/// queries for the code and the HTML grammar for the markup (injected by the grammar).
pub fn register() {
    let reg = LanguageRegistry::singleton();
    // The toolkit ships the C# grammar with an empty highlight query, so `.cs` files stayed
    // plain: give it the grammar's own queries (and Razor, below, reuses them).
    let cs_highlights = tree_sitter_c_sharp::HIGHLIGHTS_QUERY;
    reg.register(
        "csharp",
        &LanguageConfig::new(
            "csharp",
            tree_sitter_c_sharp::LANGUAGE.into(),
            vec![],
            cs_highlights,
            "",
            "",
        ),
    );
    // The grammar's queries say `inherits: c_sharp`, which the highlighter does not follow:
    // the C# queries are put in front by hand.
    let strip = |q: &str| {
        q.lines()
            .filter(|l| !l.trim_start().starts_with("; inherits"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let highlights = format!(
        "{}\n{}",
        cs_highlights,
        strip(tree_sitter_razor_modern::HIGHLIGHTS_QUERY)
    );
    // Only the grammar's own injections: a query naming a node the grammar lacks is dropped whole.
    // Elements nest, and one combined parse cannot take overlapping ranges, so each element is
    // its own injection (no `injection.combined`).
    let injections =
        strip(tree_sitter_razor_modern::INJECTIONS_QUERY).replace("(#set! injection.combined)", "");
    reg.register(
        "razor",
        &LanguageConfig::new(
            "razor",
            tree_sitter_razor_modern::LANGUAGE.into(),
            vec!["html".into(), "csharp".into()],
            &highlights,
            &injections,
            "",
        ),
    );
}
