# Linux + Windows Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Releases ship the Kalem IDE (not only the CLI) on Windows and Linux, and every push proves the GUI builds, passes clippy/tests and starts on Linux and Windows.

**Architecture:** A Docker image (`ubuntu:latest`) plus `check.sh` is the single Linux GUI check, run locally and in CI. Packaging scripts gain the `kalem-ide` binary, shortcuts / `.desktop` entry and dependencies; the release workflow's smoke tests are extended to prove it. CI runs on push/PR.

**Tech Stack:** Rust (cargo), GPUI via `gpui-kit`, Docker, Xvfb + Mesa lavapipe, ImageMagick, NSIS 3, nfpm, GitHub Actions, bash, PowerShell 7.

**Spec:** `docs/superpowers/specs/2026-10-14-linux-windows-hardening-design.md`

## Global Constraints

- Docker check image: `ubuntu:latest`. Linux CI runner: `ubuntu-latest`.
- Release Linux builds stay on `ubuntu-22.04` (glibc 2.35 floor) — do not change `release.yml` runners.
- macOS packaging stays CLI-only; macOS CI runs core crates only.
- GUI crate is `kalem-ui`, its binary is `kalem-ide` (`kalem-ide.exe`). CLI crate/binary is `kalem`.
- `kalem-ui` is not a default workspace member: always pass `-p kalem-ui`.
- Windows shortcut name: `Kalem IDE`. Silent-install flags: `/S`, `/NOPATH`, new `/NODESKTOP`.
- Linux desktop entry: `Name=Kalem IDE`, `Exec=kalem-ide %F`, `Icon=kalem-ide`, `Categories=Development;IDE;`.
- Icon source: `src/kalem-ui/assets/kalem-ide.png` (512×512) → `usr/share/icons/hicolor/512x512/apps/kalem-ide.png`.
- Clean-room rules and existing code style apply: comments explain *why*, short, plain English.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. Installing the `.deb` on a stock Ubuntu with no dev packages → `kalem-ide` has no missing libraries (Task 3: `apt-get install ./*.deb` + `ldd` check in `release.yml`).
2. Launching from the Start menu / app launcher passes **no** arguments → IDE still starts (Task 1: `check.sh` also starts `kalem-ide` with no arguments).
3. Machine without a GPU → IDE starts on software Vulkan (Task 1: `check.sh` runs on lavapipe only).
4. Silent install with `/NODESKTOP` → no desktop shortcut; plain `/S` → desktop shortcut exists (Task 4: smoke test installs both ways).
5. Installing over an existing install, then uninstalling → nothing left behind (Task 4: smoke test installs twice before uninstalling).

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `deployments/linux/Dockerfile` | create | Ubuntu latest + Rust + GPUI libs + Xvfb + lavapipe + ImageMagick |
| `deployments/linux/check.sh` | create | clippy/build/test GUI, `ldd`, start-up screenshot |
| `src/kalem-ui/src/main.rs` | modify | no console window in release builds |
| `deployments/linux/kalem-ide.desktop` | create | app-launcher entry |
| `deployments/linux/package.sh` | modify | build/stage `kalem-ide`, desktop file, icon |
| `deployments/linux/nfpm.yaml` | modify | ship `kalem-ide`, GUI library dependencies |
| `deployments/windows/package.ps1` | modify | build/check/sign/zip `kalem-ide.exe` |
| `deployments/windows/kalem.nsi` | modify | install exe, shortcuts, App Paths, uninstall |
| `.github/workflows/release.yml` | modify | extended Linux + Windows package checks |
| `.github/workflows/ci.yml` | modify | push/PR triggers, GUI steps, Docker Linux job |
| `docs/RUNNING.md`, `deployments/README.md` | modify | how to run the check, what installs where |
| `.gitignore` | check | `dist/` already ignored? (`dist/linux-check/` must not be committed) |

---

### Task 0: Branch

- [ ] **Step 1: Create the work branch**

```bash
git checkout -b linux-windows-hardening
git add docs/superpowers/specs/2026-10-14-linux-windows-hardening-design.md docs/superpowers/plans/2026-10-14-linux-windows-hardening.md
git commit -m "Spec and plan: Linux + Windows hardening

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 1: Docker Linux check

**Files:**
- Create: `deployments/linux/Dockerfile`
- Create: `deployments/linux/check.sh`
- Modify: `docs/RUNNING.md` (new section "Linux check in Docker")

**Interfaces:**
- Produces: image tag `kalem-linux-check`; `check.sh` exits non-zero on any failure; writes `dist/linux-check/kalem-ide.png` and `dist/linux-check/kalem-ide.log`. Task 3 uses its `ldd` output; Task 5 runs it in CI.

- [ ] **Step 1: Make sure `dist/` is ignored**

Run: `git check-ignore dist/linux-check/x.png`
Expected: prints the path. If it prints nothing, add `/dist/` to `.gitignore`.

- [ ] **Step 2: Write `deployments/linux/Dockerfile`**

```dockerfile
# Linux check for the desktop app: builds kalem-ui and starts it on a virtual screen with software
# Vulkan (lavapipe), so it runs anywhere Docker does, with no GPU. Used locally and by CI:
#
#   docker build -t kalem-linux-check deployments/linux
#   docker run --rm -v "$PWD:/src" -v kalem-target:/src/target -v kalem-cargo:/usr/local/cargo/registry kalem-linux-check
FROM ubuntu:latest
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git build-essential pkg-config \
      libx11-dev libx11-xcb-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
      libfontconfig1-dev libfreetype-dev libvulkan-dev libssl-dev \
      xvfb mesa-vulkan-drivers imagemagick \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --component clippy,rustfmt
WORKDIR /src
CMD ["bash", "deployments/linux/check.sh"]
```

- [ ] **Step 3: Write `deployments/linux/check.sh`**

```bash
#!/usr/bin/env bash
# Linux check for the desktop app, run inside deployments/linux/Dockerfile (see there for usage):
# clippy, build and tests; no missing shared libraries; the IDE starts with no arguments (as the
# app launcher starts it) and with a folder, and draws something (screenshot is not one colour).
# Output: dist/linux-check/kalem-ide.png and kalem-ide.log.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
# The mounted repo belongs to another user; without this git (and so Solution Explorer) refuses it.
git config --global --add safe.directory "$PWD"

cargo clippy -p kalem-ui --all-targets -- -D warnings
cargo build -p kalem-ui
cargo test

BIN=target/debug/kalem-ide
ldd "$BIN"
if ldd "$BIN" | grep -q 'not found'; then echo "kalem-ide: missing shared libraries" >&2; exit 1; fi

OUT=dist/linux-check
mkdir -p "$OUT"
Xvfb :99 -screen 0 1600x1000x24 &
XVFB=$!
APP=
trap 'kill $APP $XVFB 2>/dev/null || true' EXIT
export DISPLAY=:99
# Software Vulkan: the container has no GPU.
VK_ICD_FILENAMES="$(ls /usr/share/vulkan/icd.d/lvp_icd.*.json | head -n1)"
export VK_ICD_FILENAMES
sleep 1

# start <seconds> [args...]: start kalem-ide, fail if it exits within <seconds>.
start() {
  local wait="$1"; shift
  "$BIN" "$@" >"$OUT/kalem-ide.log" 2>&1 &
  APP=$!
  sleep "$wait"
  if ! kill -0 "$APP" 2>/dev/null; then
    cat "$OUT/kalem-ide.log"
    echo "kalem-ide $* exited during start-up" >&2
    exit 1
  fi
}

start 5
kill "$APP"; wait "$APP" 2>/dev/null || true

start 10 "$PWD"
import -window root "$OUT/kalem-ide.png"
kill "$APP"; wait "$APP" 2>/dev/null || true

colours="$(identify -format %k "$OUT/kalem-ide.png")"
if [ "$colours" -le 1 ]; then echo "screenshot is blank" >&2; exit 1; fi
echo "OK: $OUT/kalem-ide.png ($colours colours)"
```

Then: `git update-index --chmod=+x deployments/linux/check.sh` after adding it (Windows checkouts do not keep the bit).

- [ ] **Step 4: Run it and watch it fail or pass**

Docker Desktop must be running.

```bash
docker build -t kalem-linux-check deployments/linux
docker run --rm -v "$PWD:/src" -v kalem-target:/src/target -v kalem-cargo:/usr/local/cargo/registry kalem-linux-check
```

Expected on success: last line `OK: dist/linux-check/kalem-ide.png (N colours)`. Open the PNG and confirm it shows the Kalem window (menu bar, Solution Explorer with this repo).

**If it fails:** this is the spec's section 4. Fix the root cause in Kalem's code (or add a missing `-dev` package to the Dockerfile if it is a build-time library), re-run, repeat. Record every fix in the commit message. A problem inside GPUI that Kalem cannot fix goes into `docs/ROADMAP.md` as an item under "Known platform issues"; only then may the check be adjusted, with a comment naming the roadmap item.

- [ ] **Step 5: Save the `ldd` output for Task 3**

```bash
docker run --rm -v "$PWD:/src" -v kalem-target:/src/target kalem-linux-check \
  bash -c 'ldd target/debug/kalem-ide | awk "/=>/{print \$3}" | xargs dpkg -S 2>/dev/null | cut -d: -f1 | sort -u'
```

Expected: a list of runtime package names (e.g. `libc6`, `libgcc-s1`, `libxcb1`, `libxkbcommon0`, `libfontconfig1`, `libfreetype6`, ...). Keep it for Task 3.

- [ ] **Step 6: Document it in `docs/RUNNING.md`**

Add after the "Settings and troubleshooting" section:

```markdown
## Linux check in Docker

Builds the desktop app on the latest Ubuntu, runs clippy and the tests, and starts the IDE on a
virtual screen with software rendering. Docker Desktop must be running.

    docker build -t kalem-linux-check deployments/linux
    docker run --rm -v "${PWD}:/src" -v kalem-target:/src/target -v kalem-cargo:/usr/local/cargo/registry kalem-linux-check

It ends with `OK: dist/linux-check/kalem-ide.png`; open the screenshot to see the IDE. The first run
takes a while (GPUI); the two volumes keep the build for the next run. CI runs the same check.
```

- [ ] **Step 7: Commit**

```bash
git add deployments/linux/Dockerfile deployments/linux/check.sh docs/RUNNING.md
git commit -m "Linux check in Docker: build, test and start the IDE on a virtual screen

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

(Include any fixes from Step 4 in this commit or in separate commits before it, each named.)

---

### Task 2: No console window on Windows release builds

**Files:**
- Modify: `src/kalem-ui/src/main.rs:1`
- Modify: `docs/RUNNING.md` (troubleshooting list)

- [ ] **Step 1: See the problem**

Run: `cargo build -p kalem-ui --release` then double-click `target\release\kalem-ide.exe` in Explorer.
Expected: a console window opens behind the IDE.

- [ ] **Step 2: Add the attribute**

At the top of `src/kalem-ui/src/main.rs`, after the `//!` line:

```rust
//! Kalem IDE desktop app. All behaviour that is not drawing lives in `kalem-workspace`.
// Release builds are a GUI app on Windows: no console window behind the IDE. Debug builds keep it
// for KALEM_LSP_TRACE / KALEM_DAP_TRACE output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
```

- [ ] **Step 3: Verify**

Run: `cargo build -p kalem-ui --release`, double-click `target\release\kalem-ide.exe`.
Expected: only the IDE window. Then `cargo clippy -p kalem-ui --all-targets -- -D warnings` → no warnings.

- [ ] **Step 4: Note it in `docs/RUNNING.md`**

In "Settings and troubleshooting", after the `KALEM_LSP_TRACE` bullet add:

```markdown
- On Windows, release builds have no console, so the traces only show with a debug build
  (`cargo run -p kalem-ui`).
```

- [ ] **Step 5: Commit**

```bash
git add src/kalem-ui/src/main.rs docs/RUNNING.md
git commit -m "Windows: no console window behind release builds of the IDE

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Linux packages ship the IDE

**Files:**
- Create: `deployments/linux/kalem-ide.desktop`
- Modify: `deployments/linux/package.sh:39-51`
- Modify: `deployments/linux/nfpm.yaml` (`overrides`, `contents`)
- Modify: `.github/workflows/release.yml` (Linux "Check the packages" step)
- Modify: `deployments/README.md` (installed layout line)

**Interfaces:**
- Consumes: package list from Task 1 Step 5.

- [ ] **Step 1: Write `deployments/linux/kalem-ide.desktop`**

```ini
[Desktop Entry]
Type=Application
Name=Kalem IDE
Comment=Multi-language IDE
Exec=kalem-ide %F
Icon=kalem-ide
Terminal=false
Categories=Development;IDE;
StartupWMClass=kalem-ide
```

Validate: `docker run --rm -v "$PWD:/src" ubuntu:latest bash -c "apt-get update -qq && apt-get install -yqq desktop-file-utils >/dev/null && desktop-file-validate /src/deployments/linux/kalem-ide.desktop"`
Expected: no output, exit 0.

- [ ] **Step 2: Build and stage `kalem-ide` in `package.sh`**

Replace the build line and the staging block (lines 39–51) with:

```bash
if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p kalem -p kalem-ui)
fi
BIN="$CARGO_TARGET_DIR/release/kalem"
IDE="$CARGO_TARGET_DIR/release/kalem-ide"
for f in "$BIN" "$IDE"; do
  [ -f "$f" ] || { echo "missing $f - build first" >&2; exit 1; }
done

WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"
install -Dm755 "$BIN" "$STAGE/usr/bin/kalem"
install -Dm755 "$IDE" "$STAGE/usr/bin/kalem-ide"
install -Dm644 "$HERE/kalem-ide.desktop" "$STAGE/usr/share/applications/kalem-ide.desktop"
install -Dm644 "$ROOT/src/kalem-ui/assets/kalem-ide.png" "$STAGE/usr/share/icons/hicolor/512x512/apps/kalem-ide.png"
mkdir -p "$STAGE/usr/share/kalem/languages" "$STAGE/usr/share/doc/kalem-ide"
cp "$LANG_SRC"/*.toml "$STAGE/usr/share/kalem/languages/"
cp "$ROOT/README.md" "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$STAGE/usr/share/doc/kalem-ide/"
strip "$STAGE/usr/bin/kalem" "$STAGE/usr/bin/kalem-ide" 2>/dev/null || true
```

Also update the header comment line 12 to: `# missing. Build on an old distro (CI: Ubuntu 22.04, glibc 2.35) so the binaries run on newer ones.`

- [ ] **Step 3: `nfpm.yaml` contents and dependencies**

Add to `contents` after the `kalem` entry:

```yaml
  - src: ${STAGE}/usr/bin/kalem-ide
    dst: /usr/bin/kalem-ide
    file_info: { mode: 0755 }
```

Replace the comment `# The binary links only glibc and libgcc_s directly.` and the two `depends` lists. Deb: every package from Task 1 Step 5 **plus** the libraries GPUI loads at run time with `dlopen` (not visible to `ldd`): `libvulkan1`, `libwayland-client0`, `libxkbcommon0`, `libxkbcommon-x11-0`. Use names without `t64` suffixes or versions (except the existing `libc6 (>= 2.35)`) so they resolve on 22.04 and later. Rpm: the Fedora names for the same libraries. Example result (adjust to the real Step 5 list):

```yaml
# kalem links glibc and libgcc_s; kalem-ide also links the X11/font libraries below (from ldd)
# and loads Vulkan, Wayland and xkbcommon at run time (dlopen), so those are listed by hand.
overrides:
  deb:
    depends:
      - libc6 (>= 2.35)
      - libgcc-s1
      - libxcb1
      - libx11-xcb1
      - libxkbcommon0
      - libxkbcommon-x11-0
      - libwayland-client0
      - libfontconfig1
      - libfreetype6
      - libvulkan1
    recommends:
      - git
      - mesa-vulkan-drivers
  rpm:
    depends:
      # Same floor as the deb: built on glibc 2.35, so older systems must refuse the install
      # instead of installing a binary that cannot start.
      - glibc >= 2.35
      - libxcb
      - libxkbcommon
      - libxkbcommon-x11
      - libwayland-client
      - fontconfig
      - freetype
      - vulkan-loader
    recommends:
      - git
      - mesa-vulkan-drivers
```

- [ ] **Step 4: Extend the `release.yml` Linux check**

Replace the "Check the packages" `run:` block with:

```yaml
        run: |
          dpkg-deb --info dist/release/*.deb
          contents="$(dpkg-deb --contents dist/release/*.deb)"
          for f in 'usr/bin/kalem$' 'usr/bin/kalem-ide$' 'share/kalem/languages/rust.toml' \
                   'share/applications/kalem-ide.desktop' 'hicolor/512x512/apps/kalem-ide.png'; do
            grep -Eq "$f" <<<"$contents" || { echo "::error::deb is missing $f"; exit 1; }
          done
          sudo apt-get update -qq
          sudo apt-get install -y desktop-file-utils
          desktop-file-validate deployments/linux/kalem-ide.desktop
          # A real install pulls the declared dependencies; nothing may be missing afterwards.
          sudo apt-get install -y ./dist/release/*.deb
          if ldd /usr/bin/kalem-ide | grep 'not found'; then echo "::error::kalem-ide has missing libraries"; exit 1; fi
          kalem languages.list
          sudo apt-get remove -y kalem-ide
```

- [ ] **Step 5: Verify the tarball locally (no nfpm needed)**

```bash
docker run --rm -v "$PWD:/src" -v kalem-target:/src/target -v kalem-cargo:/usr/local/cargo/registry kalem-linux-check \
  bash -c 'bash deployments/linux/package.sh --formats tar && tar -tzf dist/release/*.tar.gz'
```

Expected: the listing contains `bin/kalem`, `bin/kalem-ide`, `share/applications/kalem-ide.desktop`, `share/icons/hicolor/512x512/apps/kalem-ide.png`, `share/kalem/languages/rust.toml`. (deb/rpm and the install test run in `release.yml`.)

- [ ] **Step 6: Update `deployments/README.md`**

Change the "Installed layout" sentence to:

```markdown
Installed layout: `<prefix>/bin/kalem` (CLI) and `<prefix>/bin/kalem-ide` (the IDE; Linux also gets
`share/applications/kalem-ide.desktop` and an icon) with the language packs in
`<prefix>/share/kalem/languages` (Windows: `kalem.exe`, `kalem-ide.exe` and `languages\` in one folder,
plus a Start-menu shortcut). macOS packages contain the CLI only. Kalem finds the language packs
relative to the executable, or via `KALEM_LANGUAGES`.
```

- [ ] **Step 7: Commit**

```bash
git add deployments/linux/kalem-ide.desktop deployments/linux/package.sh deployments/linux/nfpm.yaml .github/workflows/release.yml deployments/README.md
git commit -m "Linux packages ship the IDE: kalem-ide, desktop entry, icon, GUI dependencies

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Windows packages ship the IDE

**Files:**
- Modify: `deployments/windows/package.ps1` (build, PE check, sign, zip; header comment)
- Modify: `deployments/windows/kalem.nsi`
- Modify: `.github/workflows/release.yml` (Windows "Smoke test" step)

- [ ] **Step 1: `package.ps1` builds, checks and signs both exes**

Replace from `Invoke-Native "cargo build ($Target)"` through `& (Join-Path $PSScriptRoot 'sign.ps1') $Exe` with:

```powershell
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p kalem -p kalem-ui --target $Target }
}

$Bin = Join-Path $TargetDir "$Target\release"
$Exes = @('kalem.exe', 'kalem-ide.exe') | ForEach-Object { Join-Path $Bin $_ }
$expected = switch ($Arch) { 'x64' { 0x8664 } 'arm64' { 0xAA64 } }
foreach ($Exe in $Exes) {
  if (-not (Test-Path $Exe)) { throw "missing $Exe - build first" }
  # Check the PE header: Machine must match -Arch, so an x64 build can never ship labelled arm64.
  $bytes = [System.IO.File]::ReadAllBytes($Exe)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  $machine = [BitConverter]::ToUInt16($bytes, $pe + 4)
  if ($machine -ne $expected) { throw ("{0} machine type 0x{1:X} does not match -Arch {2}" -f (Split-Path $Exe -Leaf), $machine, $Arch) }
  & (Join-Path $PSScriptRoot 'sign.ps1') $Exe
}
```

Change `Copy-Item $Exe $Stage` to `Copy-Item $Exes $Stage`. In the header, change the zip line to `kalem.exe, kalem-ide.exe + languages\ (unzip and run)` and the setup line to `installer (NSIS): IDE + CLI, Start-menu shortcut, optionally PATH and a desktop shortcut`.

- [ ] **Step 2: `kalem.nsi` — options, files, shortcuts, uninstall**

Header comment: add `/NODESKTOP` to the silent-install line:
`; Silent install: setup.exe /S   (/NOPATH skips the PATH change, /NODESKTOP the desktop shortcut,`
`; /D=C:\dir last to change the folder). Silent uninstall: "%ProgramFiles%\Kalem IDE\uninstall.exe" /S`

Variables, after `Var PathCheckbox`:

```nsis
Var AddDesktop
Var DesktopCheckbox
```

In `.onInit`, after the `/NOPATH` block:

```nsis
  StrCpy $AddDesktop 1
  ClearErrors
  ${GetOptions} $R0 "/NODESKTOP" $R1
  ${IfNot} ${Errors}
    StrCpy $AddDesktop 0
  ${EndIf}
```

In `PathPageCreate`, before `nsDialogs::Show`:

```nsis
  ${NSD_CreateCheckbox} 0 72u 100% 12u "Create a desktop shortcut"
  Pop $DesktopCheckbox
  ${If} $AddDesktop == 1
    ${NSD_Check} $DesktopCheckbox
  ${EndIf}
```

In `PathPageLeave`, add: `${NSD_GetState} $DesktopCheckbox $AddDesktop`

In `Section "Kalem IDE (required)"`: after `File "${BINDIR}\kalem.exe"` add `File "${BINDIR}\kalem-ide.exe"`. After the existing `App Paths\kalem.exe` line add:

```nsis
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\kalem-ide.exe" "" "$INSTDIR\kalem-ide.exe"
  ; Shortcuts for all users, matching the per-machine install.
  SetShellVarContext all
  CreateShortcut "$SMPROGRAMS\Kalem IDE.lnk" "$INSTDIR\kalem-ide.exe"
  ${If} $AddDesktop == 1
    CreateShortcut "$DESKTOP\Kalem IDE.lnk" "$INSTDIR\kalem-ide.exe"
  ${EndIf}
```

After the `DisplayVersion` line add: `WriteRegStr HKLM "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\kalem-ide.exe"`

In `Section "Uninstall"`, before `Delete "$INSTDIR\kalem.exe"`:

```nsis
  SetShellVarContext all
  Delete "$SMPROGRAMS\Kalem IDE.lnk"
  Delete "$DESKTOP\Kalem IDE.lnk"
  Delete "$INSTDIR\kalem-ide.exe"
```

and after the `App Paths\kalem.exe` `DeleteRegKey`: `DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\kalem-ide.exe"`

- [ ] **Step 3: Extend the `release.yml` Windows smoke test**

Replace the smoke-test `run:` block with:

```powershell
          Expand-Archive dist/release/*-portable.zip -DestinationPath $env:RUNNER_TEMP/portable
          & (Get-ChildItem $env:RUNNER_TEMP/portable -Recurse -Filter kalem.exe).FullName languages.list
          if ($LASTEXITCODE -ne 0) { throw 'portable kalem.exe failed' }
          if (-not (Get-ChildItem $env:RUNNER_TEMP/portable -Recurse -Filter kalem-ide.exe)) { throw 'portable zip has no kalem-ide.exe' }
          $setup = (Get-ChildItem dist/release -Filter *-setup.exe).FullName
          $dir = "$env:ProgramFiles\Kalem IDE"
          $menu = "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Kalem IDE.lnk"
          $desk = "$env:PUBLIC\Desktop\Kalem IDE.lnk"
          # Silent install without a desktop shortcut first ...
          Start-Process $setup -ArgumentList '/S', '/NODESKTOP' -Wait
          if (Test-Path $desk) { throw '/NODESKTOP still created a desktop shortcut' }
          # ... then install again over it (an upgrade), with the default desktop shortcut.
          Start-Process $setup -ArgumentList '/S' -Wait
          & "$dir\kalem.exe" languages.list
          if ($LASTEXITCODE -ne 0) { throw 'installed kalem.exe failed' }
          foreach ($p in @("$dir\kalem-ide.exe", $menu, $desk)) { if (-not (Test-Path $p)) { throw "missing after install: $p" } }
          $machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
          if (($machinePath -split ';') -notcontains $dir) { throw 'install folder is not on PATH' }
          Start-Process "$dir\uninstall.exe" -ArgumentList '/S', "_?=$dir" -Wait
          foreach ($p in @("$dir\kalem.exe", "$dir\kalem-ide.exe", $menu, $desk)) { if (Test-Path $p) { throw "uninstall left $p behind" } }
          if (Test-Path 'HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths\kalem-ide.exe') { throw 'uninstall left the App Paths key' }
```

- [ ] **Step 4: Build locally**

Needs NSIS (`winget install NSIS.NSIS`).
Run: `pwsh deployments/windows/package.ps1 -Arch x64`
Expected: `dist\release\kalem-ide-<v>-windows-x64-setup.exe` and `-portable.zip`; `Expand-Archive` the zip and confirm it has `kalem.exe` and `kalem-ide.exe`.

- [ ] **Step 5: Manual install check (admin PowerShell)**

Run the setup.exe interactively: the options page shows both checkboxes. After install, open Start → "Kalem IDE": the IDE opens, **no console window**, taskbar shows the Kalem icon. Win+R → `kalem-ide` opens it too. Uninstall from Settings → Apps (the entry shows the Kalem icon). Afterwards `C:\Program Files\Kalem IDE`, the Start-menu entry and the desktop shortcut are gone.

- [ ] **Step 6: Commit**

```bash
git add deployments/windows/package.ps1 deployments/windows/kalem.nsi .github/workflows/release.yml
git commit -m "Windows packages ship the IDE: kalem-ide.exe, Start-menu and desktop shortcuts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: CI on push and pull requests

**Files:**
- Modify: `.github/workflows/ci.yml` (whole file)
- Modify: `docs/RUNNING.md` ("Checks before pushing" last sentence)

- [ ] **Step 1: Replace `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
  pull_request:
  workflow_dispatch:

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
      # The desktop app on Windows. Linux runs it in the Docker job below; macOS is not covered yet.
      - if: runner.os == 'Windows'
        run: cargo clippy -p kalem-ui --all-targets -- -D warnings
      - if: runner.os == 'Windows'
        run: cargo build -p kalem-ui

  # The desktop app on Linux: same check as `docker run ... kalem-linux-check` locally.
  linux-gui:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: docker build -t kalem-linux-check deployments/linux
      - run: docker run --rm -v "$PWD:/src" kalem-linux-check
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: linux-check
          path: dist/linux-check/
          if-no-files-found: ignore
```

- [ ] **Step 2: Update `docs/RUNNING.md`**

Replace the last sentence of "Checks before pushing" with:

```markdown
CI (`.github/workflows/ci.yml`) runs these on every push and pull request on Linux, macOS and Windows,
builds the desktop app on Windows, and runs the Docker Linux check (screenshot in the `linux-check`
artifact).
```

- [ ] **Step 3: Verify**

Ask the user before pushing (pushing is outward-facing). After `git push -u origin linux-windows-hardening`:
Run: `gh run watch` (or `gh run list --branch linux-windows-hardening`).
Expected: `check (ubuntu-latest)`, `check (macos-latest)`, `check (windows-latest)` and `linux-gui` all green; the `linux-check` artifact contains `kalem-ide.png` showing the IDE.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml docs/RUNNING.md
git commit -m "CI on push and pull requests; desktop app checked on Windows and in Docker on Linux

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Release dry run

- [ ] **Step 1: Run the release workflow on the branch** (ask the user first; it creates a draft release)

Run: `gh workflow run release.yml --ref linux-windows-hardening -f version=0.0.2-rc.1 -f prerelease=true`, then `gh run watch`.
Expected: Linux x86_64/aarch64 and Windows x64/arm64 jobs green, including the extended checks; the draft release lists the deb/rpm/tar/setup/zip files. Delete the draft afterwards if it was only a test (ask the user).

- [ ] **Step 2: Walk the spec's "Done when" list**

Mark each of its four items done with the evidence (local check output, CI run URL, release run URL, manual Windows check from Task 4 Step 5).
