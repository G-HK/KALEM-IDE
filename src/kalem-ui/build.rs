//! Windows build steps for the desktop app:
//!
//! * The main thread gets a 1 MB stack there (Linux and macOS: 8 MB). GPUI's element tree and the
//!   editor need more than that in debug builds, so ask the linker for 16 MB.
//! * `assets/kalem-ide.ico` becomes the icon of `kalem-ide.exe`. If the resource compiler is
//!   missing this only warns; the app still builds, it just has the default icon.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/kalem-ide.ico");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os != "windows" {
        return;
    }
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if env == "msvc" {
        println!("cargo:rustc-link-arg-bins=/STACK:16777216");
    } else {
        println!("cargo:rustc-link-arg-bins=-Wl,--stack,16777216");
    }
    embed_icon();
}

#[cfg(windows)]
fn embed_icon() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/kalem-ide.ico");
    res.set("ProductName", "Kalem IDE");
    res.set("FileDescription", "Kalem IDE");
    if let Err(e) = res.compile() {
        println!("cargo:warning=could not embed the Kalem icon: {e}");
    }
}

#[cfg(not(windows))]
fn embed_icon() {}
