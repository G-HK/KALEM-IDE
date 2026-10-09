# Roadmap

1. Core editor: rope buffer, undo tree, multi-cursor, tree-sitter highlighting, Visual Studio-style docking UI (see [UI_DIRECTION.md](UI_DIRECTION.md)).
2. LSP client: diagnostics, completion, hover, rename, code actions.
3. Project model: `.sln`, `.csproj`, `.vcxproj`, CMake, Gradle, Cargo; Solution Explorer; build and problems panels.
4. DAP debugger UI with metadata panels (call stack, locals, modules, exceptions) and the cross-platform debug test matrix.
4b. Test Explorer and Test Results panels: JUnit reader done, discovery and UI next (see [TESTING_AND_DEBUG_PANELS.md](TESTING_AND_DEBUG_PANELS.md)).
5. Language packs verified end to end; run configurations; terminal (PTY).
6. Renderers: Markdown and web preview first.
7. Visual Studio-style keymap preset, Find in Files.
8. Version control: Git panel, then GitHub and other hosting platforms, then other VCS (see [VERSION_CONTROL.md](VERSION_CONTROL.md)).
9. Extension API (WASM) and MCP server.

First vertical slice: Rust, C#, TypeScript and Kotlin on one platform, with the others tested in CI from day one.
