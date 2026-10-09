//! Editor zoom: one percentage for every editor in the IDE, kept in the settings.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

/// The steps of the zoom list (and of Ctrl+= / Ctrl+-).
pub const LEVELS: [u32; 11] = [50, 70, 80, 90, 100, 110, 125, 150, 200, 300, 400];

struct Zoom(u32);

impl Global for Zoom {}

pub fn clamp(percent: u32) -> u32 {
    percent.clamp(LEVELS[0], LEVELS[LEVELS.len() - 1])
}

pub fn percent(cx: &App) -> u32 {
    cx.try_global::<Zoom>().map_or(100, |z| z.0)
}

pub fn set(percent: u32, cx: &mut App) {
    cx.set_global(Zoom(clamp(percent)));
}

/// Text size for an editor: the theme's code size at the current zoom.
pub fn text_size(cx: &App) -> Pixels {
    cx.theme().mono_font_size * (percent(cx) as f32 / 100.0)
}

/// The next level up (`dir > 0`) or down from `percent`.
pub fn step(percent: u32, dir: i32) -> u32 {
    if dir > 0 {
        LEVELS.iter().copied().find(|l| *l > percent)
    } else {
        LEVELS.iter().rev().copied().find(|l| *l < percent)
    }
    .unwrap_or(clamp(percent))
}
