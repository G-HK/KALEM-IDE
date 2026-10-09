# Roadmap

1. Core editor: rope buffer, undo tree, multi-cursor, tree-sitter highlighting, docking UI.
2. LSP client: diagnostics, completion, hover, rename, code actions.
3. Project model: `.sln`, `.csproj`, `.vcxproj`, CMake, Gradle, Cargo; Solution Explorer; build and problems panels.
4. DAP debugger UI and the cross-platform debug test matrix.
5. Language packs verified end to end; run configurations; terminal (PTY).
6. Renderers: Markdown and web preview first.
7. Visual Studio-style keymap preset, Find in Files, Git panel.
8. Extension API (WASM) and MCP server.

First vertical slice: Rust, C#, TypeScript and Kotlin on one platform, with the others tested in CI from day one.
