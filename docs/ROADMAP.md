# Roadmap

Status: **done**, **partly** or **planned**.

1. **partly**: Core editor and docking UI. Done: tabs, highlighting, themes, docked panels, Find / Replace in Files, tab tear-off into windows. Next: multi-cursor polish, drag docking for all panels, terminal pane (see 5), themeable syntax colours (see [UI_DIRECTION.md](UI_DIRECTION.md)).
2. **partly**: LSP client (`kalem-lsp`). Done: diagnostics, completion, hover, go to definition. Next: rename, find references, code actions, formatting, signature help, semantic highlighting, automatic server download (see [LSP.md](LSP.md)).
3. **partly**: Project model. Done: `.kalem`, `.slnx` / `.sln`, workspace manifests, Solution Explorer with sync. Next: `.csproj` / `.vcxproj` / CMake / Gradle item trees, problems panel.
4. **partly**: DAP debugger (`kalem-dap`). Done: breakpoints, stepping, call stack, expandable variables, watches, conditional breakpoints; tested against real gdb (see [DEBUGGING.md](DEBUGGING.md)). Next: gutter breakpoints, logpoints, other languages' launch settings, modules / exceptions panels, the cross-platform debug test matrix.
4b. **partly**: Test Explorer and Test Results. Done: JUnit reader and Test Results panel. Next: discovery and Test Explorer (see [TESTING_AND_DEBUG_PANELS.md](TESTING_AND_DEBUG_PANELS.md)).
5. **partly**: Language packs, run configurations, terminal. Done: packs with build / run / test tasks. Next: verify every pack end to end, run configurations (args, env), terminal (PTY).
6. **planned**: Renderers: Markdown and web preview first.
7. **partly**: Visual Studio-style keymap preset, Find in Files. Done: Find / Replace in Files, Find Next / Previous, F5 / F9 / F10 / F11 keys. Next: full keymap preset, replace preview.
8. **partly**: Version control. Done: Git panel (branches, stage, commit, push / pull, stashes, side-by-side diff). Next: hunks, conflicts, blame, history graph, GitHub and other hosting platforms, other VCS (see [VERSION_CONTROL.md](VERSION_CONTROL.md)).
9. **planned**: Extension API (WASM) and MCP server.

Distribution: GitHub Releases workflow exists; the Windows installer was tested locally, macOS and the release workflow are untested. Code signing is open.

First vertical slice: Rust, C#, TypeScript and Kotlin on one platform, with the others tested in CI from day one.
