# Kalem IDE

A multi-language, cross-platform IDE with a Visual Studio-style workflow, written in Rust.
*Kalem* is Turkish for "pen".

> **Status: early, but usable.** There is a working desktop IDE (GPUI): Solution Explorer, tabbed
> editor with syntax highlighting, Find and Replace in Files, a Git panel with side-by-side diffs,
> build / run / test tasks, a debugger (DAP) and language-server features (LSP). Tabs can be torn off
> into their own windows. Verified on Linux; Windows and macOS builds are still being exercised.
> See the [roadmap](docs/ROADMAP.md) for what is done and what is next.

## What works today

- **Editor:** tabs coloured per project, syntax highlighting, Quick Open (Ctrl+P), Find in Files with
  regex, replace and Find Next / Previous (F3 / Shift+F3), light and dark themes (Black Panda palette).
- **Solution Explorer:** `.kalem`, `.slnx` / `.sln`, workspace manifests or a plain folder; sync with the
  active document; source-control badges.
- **Build, run, test:** the language pack's `[tasks]` commands, streamed to Output; Test Results panel.
- **Git:** branches, stage / unstage, commit, amend, push / pull / fetch, stashes, side-by-side diff view.
- **Debugger:** breakpoints (F9), step / continue (F5, F10, F11), call stack, variables you can expand,
  watch expressions, conditional breakpoints. See [DEBUGGING.md](docs/DEBUGGING.md).
- **Language servers:** diagnostics, completion, hover and go to definition through the server named in
  the language pack. See [LSP.md](docs/LSP.md).
- **Windows:** drag a tab out to get a new window, or drop it into another one.

## Goals

- **Many languages out of the box:** C / C++, Rust, C# / .NET, Python, TypeScript / JavaScript, Go, Java and Kotlin.
- **Code, run, debug and preview** in one place, on **Linux, macOS and Windows**.
- **One debugger UI for everything**, built on the Debug Adapter Protocol; language servers via LSP.
- **Version control built in:** Git first (stage, commit, push, pull, ...), plus hosting-platform features for GitHub, GitLab, Bitbucket, Azure DevOps and Gitea/Forgejo; other systems (Jujutsu, Mercurial, SVN) later.
- **Test and debug overview:** Test Explorer, Test Results and debug metadata panels (call stack, locals, modules, exceptions).
- **Familiar look:** a dark, docked, Visual Studio-style layout.
- **Agent-native:** every action is a typed command that the UI, the CLI and an MCP server all share.
- **Clean-room:** it copies the *workflow* people know, not any proprietary code, icons or names.
  It is not affiliated with or endorsed by Microsoft.

## Language packs

A language is a small TOML file in [`src/kalem-lang/languages/`](src/kalem-lang/languages), not code:

```toml
[language]
id = "kotlin"
name = "Kotlin (JVM)"
extensions = ["kt", "kts"]

[lsp]
command = "kotlin-lsp"

[debug]
adapter = "kotlin-debug-adapter"
```

Debug adapters can be overridden per OS (`[debug.windows]`, `[debug.macos]`, `[debug.linux]`).
Language servers and debug adapters are not bundled: install them and keep them on your `PATH`
(see [LSP.md](docs/LSP.md) and [DEBUGGING.md](docs/DEBUGGING.md)); automatic download is planned.

## Try it

```
cargo run -p kalem-ui        # the IDE (needs the GPUI system libraries)
cargo run -p kalem -- help
cargo run -p kalem -- languages.list
cargo run -p kalem -- vcs.status .
cargo run -p kalem -- vcs.overview .
cargo run -p kalem -- vcs.forge .
cargo run -p kalem -- test.report path/to/junit.xml
cargo test
```

## Install

Prebuilt downloads for Windows, macOS and Linux are attached to each
[GitHub Release](../../releases). Verify the SHA256 checksum next to each file.

## Documentation

- [Project structure](docs/PROJECT_STRUCTURE.md)
- [Solutions and the .kalem file](docs/SOLUTION_FORMAT.md)
- [Overview](docs/OVERVIEW.md)
- [Architecture](docs/ARCHITECTURE.md)
- [UI direction](docs/UI_DIRECTION.md)
- [Debug metadata and test overview](docs/TESTING_AND_DEBUG_PANELS.md)
- [Roadmap](docs/ROADMAP.md)
- [Debugging on every platform](docs/DEBUGGING.md)
- [Language servers (LSP)](docs/LSP.md)
- [Version control and hosting platforms](docs/VERSION_CONTROL.md)

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
