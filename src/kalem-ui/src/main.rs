//! Kalem IDE desktop app. All behaviour that is not drawing lives in `kalem-workspace`.
// Release builds are a GUI app on Windows: no console window behind the IDE. Debug builds keep it
// for KALEM_LSP_TRACE / KALEM_DAP_TRACE output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod assets;
mod detached;
mod diffview;
mod grammars;
mod icons;
mod theming;
mod zoom;

use gpui_kit::component::Root;
use gpui_kit::*;
use std::path::PathBuf;

fn main() {
    // `kalem-ide [folder | solution file]`
    let start: Option<PathBuf> = std::env::args_os().nth(1).map(PathBuf::from);
    gpui_kit::application()
        .with_assets(assets::KalemAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            grammars::register();
            app::bind_keys(cx);
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Kalem IDE".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                let view = cx.new(|cx| app::Kalem::new(start.clone(), window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open the window");
            cx.activate(true);
        });
}
