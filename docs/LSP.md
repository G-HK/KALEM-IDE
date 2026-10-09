# Language servers (LSP)

Kalem talks to the language server named in a language pack (`[lsp] command`, see
`src/kalem-lang/languages/*.toml`) over stdin/stdout. One server runs per language for the open folder,
started when the first file of that language opens. Nothing is bundled: install the server and make sure it
is on your `PATH`.

| Language | Server | Install |
|---|---|---|
| Rust | `rust-analyzer` | `rustup component add rust-analyzer` |
| Python | `pyright-langserver` | `npm i -g pyright` |
| TypeScript / JavaScript | `typescript-language-server` | `npm i -g typescript-language-server typescript` |
| Go | `gopls` | `go install golang.org/x/tools/gopls@latest` |
| C / C++ | `clangd` | with LLVM / your package manager |
| C# | `roslyn-language-server` | .NET tool, see Roslyn's docs |
| Java | `jdtls` | Eclipse JDT LS |
| Kotlin | `kotlin-lsp` | JetBrains / fwcd server |

## What works today

- **Diagnostics**: squiggles in the editor, with the message on hover, and the error / warning count of the
  active file in the status bar.
- **Completion**: pops up while you type and after the server's trigger characters (`.`, `::`, ...).
- **Hover**: type and documentation under the mouse.
- **Go to definition**: Ctrl/Cmd+click on a symbol; other files of the folder open at the target.
- **Find all references** (Shift+F12): the results fill the Find Results panel (F3 steps through them).
- **Rename symbol** (F2): asks for the new name; open files change in their editors (save to keep them),
  other files are rewritten on disk.
- **Format document** (Shift+Alt+F), for servers that can format (rust-analyzer, gopls, clangd, ...).
- The server is told about every edit (after a 150 ms pause, or at once when a request needs it), and about
  saves (rust-analyzer runs `cargo check` on save) and closed tabs.
- A missing or crashing server is reported in the Output panel and the status bar; the editor keeps working.

Not yet: code actions, format selection / on save, signature help, snippets in completions,
semantic highlighting, definitions outside the open folder (standard library, dependencies), server
settings, and restarting a stopped server without reopening the folder.

`KALEM_LSP_TRACE=1` prints every protocol message to stderr.

## Tests

`src/kalem-lsp/tests/mock.rs` drives the client against an in-process server; `tests/pyright.rs` talks to a
real `pyright-langserver` (skipped where it is not installed).
