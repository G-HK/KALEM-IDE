# Running Kalem IDE locally

## Prerequisites

| | Windows | macOS | Linux |
|---|---|---|---|
| Rust (stable) | [rustup](https://rustup.rs) | rustup | rustup |
| C toolchain | Visual Studio Build Tools, workload "Desktop development with C++" (MSVC + Windows SDK) | Xcode command line tools (`xcode-select --install`) | `build-essential`, `pkg-config` |
| GPU / windowing libs | none | none | X11 or Wayland dev packages (`libxcb`, `libxkbcommon`, `libwayland`), Vulkan driver (`mesa-vulkan-drivers`) |
| Git | Git for Windows | Xcode tools or Homebrew | package manager |

The first build downloads and compiles GPUI and takes several minutes; later builds are fast.

## Start the IDE

From the repository root (PowerShell, Terminal or a shell):

```
cargo run -p kalem-ui                     # starts with no folder open: use File > Open Folder (Ctrl+O)
cargo run -p kalem-ui -- C:\path\to\project
cargo run -p kalem-ui --release -- .      # faster editor on big files
```

`kalem-ui` is not in the default workspace build, so always name it with `-p kalem-ui`. The binary is
`target\debug\kalem-ide.exe` (`target/debug/kalem-ide` elsewhere); you can also run it directly with a
folder, a `.kalem`, `.slnx` or `.sln` file as the argument.

## Try the features

Open this repository itself (`cargo run -p kalem-ui -- .`) or any small project.

| Try | How |
|---|---|
| Open a file, Quick Open | click in Solution Explorer, or Ctrl+P |
| Find in Files, next / previous | Ctrl+Shift+F, Find All, then F3 / Shift+F3 |
| Build / Run / Test | toolbar buttons (uses the language pack's `[tasks]`) |
| Git Changes, diff | Git Changes tab, click a changed file |
| Tear a tab off | right-click a tab > Move to New Window, or drag it out of the window |
| Debug (Rust) | F9 on a line, F5; F10 / F11 / Shift+F11 step; watches and "if" conditions in the Debug tab |
| Language server | open a file whose server is installed (below): squiggles, completion, hover, Ctrl+click, F2, Shift+F12 |

## Optional tools

The IDE starts without them; features that need one tell you in the Output panel or the status bar.

- **Language servers** (see [LSP.md](LSP.md)): `rustup component add rust-analyzer`, `npm i -g pyright`, ...
  Make sure the program runs from a terminal, then restart Kalem (a server is started per folder).
- **Debugger** ([DEBUGGING.md](DEBUGGING.md)): `gdb` 14 or newer, or `lldb-dap` (ships with LLVM 18+;
  on macOS with Xcode). Native debugging on Windows is the weakest platform: use LLVM's `lldb-dap`.

## Settings and troubleshooting

- Settings and user themes live in `%APPDATA%\kalem` (Windows), `~/Library/Application Support/kalem`
  (macOS) or `~/.config/kalem` (Linux). Set `KALEM_CONFIG_DIR` to use another folder (a clean start).
- `KALEM_LSP_TRACE=1` and `KALEM_DAP_TRACE=1` print every language-server / debug-adapter message to the
  console you started it from.
- On Windows, release builds have no console, so the traces only show with a debug build
  (`cargo run -p kalem-ui`).
- Blank or black window on Linux without a GPU: `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`
  (`lvp_icd.x86_64.json` / `lvp_icd.aarch64.json` before Ubuntu 26.04; package `mesa-vulkan-drivers`)
  selects the software renderer.
- `STATUS_STACK_OVERFLOW` (0xc00000fd) at start on Windows: debug builds need more than Windows'
  default 1 MB main-thread stack. The build asks the linker for 16 MB (`src/kalem-ui/build.rs`); if you
  still see it, run `cargo clean -p kalem-ui` once, or use `--release`.
- Build errors about a missing linker or `link.exe` on Windows: the C++ Build Tools workload is missing.

## Linux check in Docker

Builds the desktop app on the latest Ubuntu, runs clippy and the tests, and starts the IDE on a
virtual screen with software rendering. Docker Desktop must be running.

    docker build -t kalem-linux-check deployments/linux
    docker run --rm -v "${PWD}:/src" -v kalem-target:/src/target -v kalem-cargo:/usr/local/cargo/registry kalem-linux-check

It ends with `OK: dist/linux-check/kalem-ide.png`; open the screenshot to see the IDE. The first run
takes a while (GPUI); the two volumes keep the build for the next run. If Docker runs out of memory
during the build, add `-e CARGO_BUILD_JOBS=4`. CI runs the same check.

## Checks before pushing

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo clippy -p kalem-ui --all-targets -- -D warnings
cargo test
```

CI (`.github/workflows/ci.yml`) runs these on every push and pull request on Linux, macOS and Windows,
builds the desktop app on Windows, and runs the Docker Linux check (screenshot in the `linux-check`
artifact).
