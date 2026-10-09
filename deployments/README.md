# deployments/

Everything about getting Kalem IDE to users. Distribution is through **GitHub Releases** only (not crates.io).

```
deployments/
├── env.sh                 shared: ROOT, VERSION (from Cargo.toml), DIST, warn(), sha256_file()
├── set_version.py         set the workspace version (release workflow)
├── windows/               package.ps1, sign.ps1, kalem.nsi   -> setup .exe (NSIS) + portable .zip (x64, arm64)
├── macos/                 package.sh, import-cert.sh, entitlements.plist -> .pkg + .tar.gz (universal)
└── linux/                 package.sh, nfpm.yaml              -> .deb + .rpm + .tar.gz (x86_64, aarch64)
```

Artifacts land in `dist/release/` (gitignored), each with a `.sha256`:

| Platform | Files |
|---|---|
| Windows | `kalem-ide-<v>-windows-{x64,arm64}-setup.exe`, `...-portable.zip` |
| macOS | `kalem-ide-<v>-macos-universal.pkg`, `.tar.gz` |
| Linux | `kalem-ide-<v>-linux-{x86_64,aarch64}.deb`, `.rpm`, `.tar.gz` |

Windows setup: a wizard with an **Add Kalem to PATH** page (ticked by default; the uninstaller removes the entry). Silent: `setup.exe /S`, add `/NOPATH` to skip the PATH change.

Installed layout: `<prefix>/bin/kalem` with the language packs in `<prefix>/share/kalem/languages` (Windows: `languages\` beside `kalem.exe`). Kalem finds them relative to the executable, or via `KALEM_LANGUAGES`.

## Run locally

```sh
bash deployments/linux/package.sh          # Linux (deb/rpm need nfpm, else they are skipped)
bash deployments/macos/package.sh          # macOS, ad-hoc signed
pwsh deployments/windows/package.ps1       # Windows (needs NSIS: winget install NSIS.NSIS)
```

## Releasing

Actions tab -> Release -> Run workflow, enter a version like `0.1.0`. Test builds work with no secrets: they are just unsigned. The result is a **draft** GitHub Release to review and publish.

Signing secrets (repository environment `release`, all optional):

| Secret | Used for |
|---|---|
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `KEYCHAIN_PASSWORD` | .p12 with Developer ID Application (+ Installer) |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | notarization (app-specific password) |
| `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | base64 .pfx Authenticode certificate |
| `AZURE_*` | alternative: Azure Trusted Signing |

Without signing, Windows shows SmartScreen and macOS shows a Gatekeeper warning on first run.

## Not done yet

AppImage and Flatpak, a macOS `.app`/`.dmg` and Windows Start Menu shortcut (these come with the GUI), icons, Winget/Homebrew/Scoop manifests, auto-update.
