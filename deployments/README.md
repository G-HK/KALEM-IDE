# deployments/

Everything about getting Kalem IDE to users. Distribution is through **GitHub Releases** only (not crates.io).

- `scripts/package-unix.sh` – tar.gz + SHA256 for Linux and macOS
- `scripts/package-windows.ps1` – zip + SHA256 for Windows
- `.github/workflows/release.yml` – manual workflow that builds all targets and calls the scripts

Try locally (Linux):

```sh
cargo build --release -p kalem --target x86_64-unknown-linux-gnu
bash deployments/scripts/package-unix.sh v0.0.0-local x86_64-unknown-linux-gnu linux-x86_64
```

Not done yet: code signing (Windows SmartScreen, macOS notarization), installers (MSI/dmg/deb), auto-update.
