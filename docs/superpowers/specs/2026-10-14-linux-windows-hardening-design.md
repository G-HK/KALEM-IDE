# Linux + Windows hardening

Sub-project 1 of 4. The others (Start window with pinned/recent projects, core VS features,
extensions) get their own specs; their order is decided after this one.

## Goal

A user on Windows or Linux installs a release and gets a working **IDE** (not only the CLI), launched
from the Start menu / app launcher. Every push proves the GUI still builds, passes its checks and
starts on Linux and Windows.

## Current state (2026-10-14)

- `deployments/*/package.*` build only `-p kalem` (the CLI). `kalem-ide` is never packaged; no
  shortcuts, no `.desktop` file.
- `ci.yml` is manual-only and skips `kalem-ui` (it is not a default workspace member).
- `kalem-ide.exe` has no `windows_subsystem` attribute, so a console window opens behind it.
- The Linux GUI is untested; deb/rpm declare only glibc/libgcc dependencies.

## Out of scope

macOS `.app` bundle (macOS packaging stays CLI-only), new IDE features, interactive Linux testing
(WSLg/VNC), code signing changes.

## 1. Installers ship the IDE

### Windows (`deployments/windows/package.ps1`, `kalem.nsi`)

- Build both binaries: `cargo build --release --locked -p kalem -p kalem-ui --target $Target`.
- The PE machine-type check runs on `kalem.exe` and `kalem-ide.exe`; `sign.ps1` signs both.
- Portable zip contains `kalem.exe`, `kalem-ide.exe`, `languages\`, README, licences.
- NSIS installer:
  - installs `kalem-ide.exe` beside `kalem.exe`;
  - Start-menu shortcut `Kalem IDE` → `kalem-ide.exe` (all users, `SetShellVarContext all`);
  - desktop shortcut, optional: a checkbox on the existing custom page, ticked by default; silent
    installs skip it with `/NODESKTOP`;
  - `App Paths\kalem-ide.exe` registry entry; `DisplayIcon` in the uninstall key points at
    `kalem-ide.exe`;
  - the uninstaller removes the exe, both shortcuts and the `App Paths` key.
- `release.yml` Windows smoke test also checks that `kalem-ide.exe` and the Start-menu shortcut
  exist after install and are both gone after uninstall.

### Linux (`deployments/linux/package.sh`, `nfpm.yaml`)

- Build `-p kalem -p kalem-ui`; stage and strip `usr/bin/kalem-ide`.
- New file `deployments/linux/kalem-ide.desktop` (`Type=Application`, `Name=Kalem IDE`,
  `Exec=kalem-ide %F`, `Icon=kalem-ide`, `Categories=Development;IDE;`, `Terminal=false`), staged to
  `usr/share/applications/`.
- Icon: `src/kalem-ui/assets/kalem-ide.png` (512×512) staged as-is to
  `usr/share/icons/hicolor/512x512/apps/kalem-ide.png`.
- `nfpm.yaml` lists `/usr/bin/kalem-ide`; deb and rpm `depends` gain the shared libraries that
  `ldd kalem-ide` reports in the Docker image (section 3), mapped to package names for Ubuntu
  and Fedora.
- tar.gz gets the same tree.
- `release.yml` Linux check also greps the deb for `usr/bin/kalem-ide` and the `.desktop` file, and
  runs `desktop-file-validate` on it.

## 2. CI (`.github/workflows/ci.yml`)

- Triggers: `push`, `pull_request`, `workflow_dispatch`.
- Matrix: `ubuntu-latest`, `windows-latest`, `macos-latest`.
- All OSes: `cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `cargo test`
  (default members, unchanged).
- Windows: additionally `cargo clippy -p kalem-ui --all-targets -- -D warnings` and
  `cargo build -p kalem-ui`.
- Linux: additionally builds the Docker image and runs `check.sh` in it (section 3), which covers
  `kalem-ui` clippy, build and the start-up check.
- macOS: core crates only (no GUI step).
- `Swatinem/rust-cache@v2` on every job.

## 3. Docker Linux check

- `deployments/linux/Dockerfile`: `ubuntu:latest` (current Ubuntu LTS), rustup stable with
  clippy/rustfmt, `build-essential pkg-config`, GPUI's build libraries (X11/xcb, xkbcommon, Wayland,
  fontconfig/freetype, Vulkan loader), `xvfb`, `mesa-vulkan-drivers` (lavapipe), `imagemagick`
  (screenshot via `import -window root`).
- Release builds stay on `ubuntu-22.04` (glibc 2.35 floor). The check on `ubuntu:latest` does not
  prove a release starts on 22.04; it tracks what current Linux users run. `ubuntu:latest` moves to
  each new LTS, so a check can start failing without a Kalem change.
- `deployments/linux/check.sh`, run in the container with the repo mounted at `/src` and a named
  volume for `target/` and the cargo registry:
  1. `cargo clippy -p kalem-ui --all-targets -- -D warnings`, `cargo build -p kalem-ui`,
     `cargo test`;
  2. prints `ldd target/debug/kalem-ide`; fails if any library is `not found`;
  3. starts `Xvfb :99`, runs `kalem-ide /src` with lavapipe selected via `VK_ICD_FILENAMES`, waits
     10 s, saves `dist/linux-check/kalem-ide.png`; fails if `kalem-ide` exited before the
     screenshot, or the screenshot has a single colour (ImageMagick `%k` unique-colour count ≤ 1);
     then kills it.
- Local use (documented in `docs/RUNNING.md`, Docker Desktop must be running):

  ```
  docker build -t kalem-linux-check deployments/linux
  docker run --rm -v "${PWD}:/src" -v kalem-target:/src/target kalem-linux-check
  ```

## 4. Fixes

- `src/kalem-ui/src/main.rs`: `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`.
  Release builds open no console window. `docs/RUNNING.md` notes that `KALEM_LSP_TRACE` /
  `KALEM_DAP_TRACE` output needs a debug build (`cargo run -p kalem-ui`).
- Problems found by sections 1–3 are fixed at their root cause, each listed in the commit message.
  If a problem is large enough to be a feature of its own (e.g. a Wayland-only rendering bug in
  GPUI), it is recorded in `docs/ROADMAP.md` instead and the check is adjusted only if the problem
  is outside Kalem's code.

## Done when

1. `check.sh` passes in Docker locally and the screenshot shows the IDE window.
2. CI is green on push for Linux, Windows and macOS.
3. A `release.yml` run produces a Windows setup and zip containing `kalem-ide.exe` and Linux
   deb/rpm/tar containing `kalem-ide` with the `.desktop` file; the extended smoke tests pass.
4. Manually on Windows: install, launch from the Start menu, no console window, uninstall leaves no
   files, shortcuts or registry keys.
