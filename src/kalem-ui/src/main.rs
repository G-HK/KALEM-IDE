//! Kalem IDE desktop app. All behaviour that is not drawing lives in `kalem-workspace`.

mod app;
mod detached;
mod diffview;
mod icons;
mod theming;

use gpui_kit::component::Root;
use gpui_kit::*;
use std::path::PathBuf;

fn main() {
    // `kalem-ide [folder | solution file]`
    let start: Option<PathBuf> = std::env::args_os().nth(1).map(PathBuf::from);
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
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
