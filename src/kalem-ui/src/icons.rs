//! Icons: small SVG files embedded in the binary (Lucide, ISC licence, see assets/icons).

use gpui_kit::component::Icon;

macro_rules! svgs {
    ($($n:literal),* $(,)?) => {
        fn svg(name: &str) -> &'static [u8] {
            match name {
                $($n => include_bytes!(concat!("../assets/icons/", $n, ".svg")),)*
                _ => b"",
            }
        }
    };
}

svgs!(
    "folder",
    "folder-open",
    "file",
    "image",
    "globe",
    "app-window",
    "library",
    "flask-conical",
    "package",
    "layers",
    "chevron-right",
    "chevron-down",
    "lock",
    "pencil",
    "plus",
    "arrow-left",
    "arrow-right",
    "save",
    "undo-2",
    "redo-2",
    "play",
    "hammer",
    "refresh-cw",
    "search",
    "bug",
    "square",
    "pause",
    "circle-check",
    "arrow-down",
    "arrow-up",
    "minus",
    "git-branch",
    "archive",
    "trash",
    "sun",
    "moon",
    "x",
    "pin",
    "pin-off",
    "ellipsis",
    "arrow-down-to-line",
    "arrow-up-to-line",
    "arrow-up-down",
    "git-graph",
    "label-explorer",
    "label-git",
    "label-find",
);

pub fn icon(name: &str) -> Icon {
    Icon::default().data(svg(name))
}
