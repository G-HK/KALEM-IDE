# Kalem IDE: project overview

**Kalem** (Turkish for "pen") is an open-source, multi-language, cross-platform IDE written in Rust, with the
familiar Visual Studio-style workflow. Binaries are distributed through GitHub Releases.

## What it should do
- **Code** in many languages out of the box: C / C++, Rust, C# / .NET, Python, TypeScript / JavaScript, Go, Java, Kotlin.
- **Run and preview**: run configurations, a terminal, and preview panels (web, Markdown, images, shaders).
- **Debug on Linux, macOS and Windows** through one UI built on the Debug Adapter Protocol, reading debug metadata
  into panels ([TESTING_AND_DEBUG_PANELS.md](TESTING_AND_DEBUG_PANELS.md)).
- **Test overview**: a Test Explorer and Test Results panel for every language ([same doc](TESTING_AND_DEBUG_PANELS.md)).
- **Version control**: Git first, plus GitHub, GitLab, Bitbucket, Azure DevOps and Gitea/Forgejo features
  ([VERSION_CONTROL.md](VERSION_CONTROL.md)).
- **Agent-native**: every action is a command that the UI, CLI and an MCP server share.
- **Look and feel**: dark, docked, Visual Studio-style ([UI_DIRECTION.md](UI_DIRECTION.md)).

## Principles
1. Clean-room: copy the workflow, never proprietary code, icons or names. Open protocols and open tools only.
2. Languages are data (TOML packs); language servers and debug adapters are downloaded on first use.
3. Everything is a command. Destructive actions need confirmation; agents cannot push or rewrite history unasked.
4. Secrets stay out of the repo: strict `.gitignore`, a secret guard before commits, tokens only in the OS keychain.
5. Releases are manual (GitHub Actions `workflow_dispatch`), with checksums.

## Status

| Area | State |
|---|---|
| Workspace, command registry, CLI | Done (skeleton) |
| Language packs (8) + loader + per-OS adapter overrides | Done |
| Git status + hosting-platform detection | Done (`vcs.status`, `vcs.forge`) |
| Git Changes panel logic (branch/sync state, commit button rules, explorer badges) | Done (`vcs.overview`) |
| Project Explorer model (tree, sync with active document, stats) | Done (no UI yet) |
| Test report reader (JUnit XML) + overview | Done (`test.report`) |
| CI and release workflows (manual) | Written, not yet run on GitHub |
| Editor core (rope buffer, undo tree, tree-sitter) | Planned (placeholder buffer only) |
| LSP client | Planned (stub) |
| DAP client + debug panels | Planned (stub) |
| Docking UI and panels | Planned (toolkit not chosen) |
| Git write operations, platform sign-in, PRs | Planned |
| Run configurations, terminal, previews | Planned |
| Extension API (WASM) and MCP server | Planned |

## Repository layout
```
src/kalem            CLI entry point
src/kalem-core       command registry, text buffer, project explorer tree
src/kalem-lang       language packs (languages/*.toml)
src/kalem-vcs        Git backend, forge detection, Git Changes overview
src/kalem-solution   solutions: .kalem file, .slnx/.sln readers
src/kalem-test       test results model, JUnit reader
src/kalem-lsp        LSP client (stub)
src/kalem-dap        DAP client (stub)
tests/               integration tests and fixtures
deployments/         packaging scripts
docs/                design documents
.github/workflows     manual CI and release
```

## Try it
```
cargo test --workspace
cargo run -p kalem -- help
cargo run -p kalem -- languages.list
cargo run -p kalem -- vcs.status .
cargo run -p kalem -- vcs.overview .
cargo run -p kalem -- test.report path/to/junit.xml
```

## Open decisions
- UI toolkit (egui + docking, GPUI, or Tauri).
- Kotlin language server choice and licence check.
- Code signing for Windows and macOS releases.
- Trademark: a Turkish software company holds "kalem" in class 42 in Turkey (see the plan notes).
