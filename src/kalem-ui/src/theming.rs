//! Maps a Kalem theme palette onto the toolkit's theme.

use gpui_kit::component::{theme::ThemeMode, Theme};
use gpui_kit::*;
use kalem_theme::{Mode, Palette, Theme as KalemTheme};

pub fn rgba(c: u32) -> Hsla {
    rgb(c).into()
}

pub fn is_dark(mode: Mode, cx: &App) -> bool {
    match mode {
        Mode::Dark => true,
        Mode::Light => false,
        Mode::System => matches!(cx.window_appearance(), WindowAppearance::Dark),
    }
}

pub fn apply(theme: &KalemTheme, dark: bool, cx: &mut App) -> Palette {
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    let p = theme.palette(dark).clone();
    let c = |t: &str| rgba(p.get(t));
    // Colours go in a second update: switching mode reloads the default theme over them.
    Theme::update(cx, |t| {
        t.background = c("background");
        t.foreground = c("foreground");
        t.muted_foreground = c("muted_foreground");
        t.border = c("border");
        t.input = c("border");
        t.list_even = c("background");
        t.list_head = c("panel");
        t.list_active = c("list_active");
        t.list_hover = c("list_active");
        t.list_active_border = c("accent");
        t.accent = c("list_active");
        t.accent_foreground = c("foreground");
        t.primary = c("accent");
        t.primary_hover = c("accent");
        t.primary_active = c("accent");
        t.primary_foreground = readable_on(c("accent"), c("background"), c("foreground"));
        t.ring = c("accent");
        t.caret = c("foreground");
        t.link = c("accent");
        t.selection = c("accent").opacity(0.3);
        t.sidebar = c("panel");
        t.sidebar_border = c("border");
        t.sidebar_foreground = c("foreground");
        t.popover = c("panel");
        t.popover_foreground = c("foreground");
        t.tab_bar = c("panel");
        t.tab = c("panel");
        t.tab_active = c("background");
        t.tab_active_foreground = c("foreground");
        t.tab_foreground = c("muted_foreground");
        t.title_bar = c("panel");
        t.title_bar_border = c("border");
        t.secondary = c("panel");
        t.secondary_foreground = c("foreground");
        t.muted = c("panel");
        t.success = c("success");
        t.warning = c("warning");
        t.danger = c("danger");
    });
    p
}

/// Whichever of two colours reads better on `bg` (WCAG contrast ratio).
pub fn readable_on(bg: Hsla, a: Hsla, b: Hsla) -> Hsla {
    fn lum(c: Hsla) -> f32 {
        let c = c.to_rgb();
        let f = |v: f32| {
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    }
    let ratio = |x: Hsla| {
        let (l1, l2) = (lum(bg).max(lum(x)), lum(bg).min(lum(x)));
        (l1 + 0.05) / (l2 + 0.05)
    };
    if ratio(a) >= ratio(b) {
        a
    } else {
        b
    }
}
