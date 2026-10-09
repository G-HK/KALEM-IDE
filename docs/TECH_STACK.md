# Tech stack

Decision record for the UI toolkit and the supporting libraries. The UI choice was made by building
the same screen twice (`spikes/ui-egui`, `spikes/ui-gpui`) on the real Kalem crates.

## UI toolkit spike

Same screen in both: Solution Explorer with "sync with active document", tabbed editor with Rust
highlighting and line numbers, Git Changes, Test Results, status bar.

| | egui 0.36 + egui_dock | GPUI + gpui-component 0.7.1 |
|---|---|---|
| Spike size | 497 lines | 189 lines |
| Code editor | own work: tree-sitter spans, layouter, gutter, no editing engine | built in: rope, undo, selection, line numbers, tree-sitter highlighting, search, LSP hooks |
| Tree view | build from collapsing headers; sync is manual | `TreeState` with `index_of`, `set_selected_index`, `scroll_to_item` |
| Docking | `egui_dock`, drag and split work out of the box | `DockArea` exists (tabs, split); not used in the spike, needs more wiring |
| Rendering model | immediate mode, repaints per frame | retained, GPU, closer to a native editor feel |
| Debug binary / build time | 408 MB, quick | 728 MB, ~5 min cold build |
| Maturity | stable releases | pre-1.0, pinned GPUI snapshot (`gpui-pre`) |
| License | MIT/Apache | Apache-2.0 |
| Platforms | Windows, macOS, Linux | Windows (DX), macOS (Metal), Linux (Vulkan) |

Findings:

- The editor decides it. A usable code editor (IME, selection, undo, large files, folding) is the
  largest single piece of an IDE, and gpui-component ships one; with egui we would write it.
- GPUI needs a single `tree-sitter` 0.26 across the workspace (native `links` conflict), so all
  Kalem crates must use that version.
- GPUI default styling is plain; theming and icons are work for us either way.
- Risk for GPUI: pre-1.0 API churn and a pinned snapshot. Mitigation: keep all UI in `src/kalem-ui`,
  keep logic in toolkit-free crates (already true: the spikes share `kalem-core`, `kalem-vcs`,
  `kalem-test`, `kalem-solution`).

**Recommendation: GPUI + gpui-component.** egui stays the fallback if GPUI blocks us; the toolkit-free
crates make that switch bounded.

## Stack

| Concern | Choice |
|---|---|
| UI | GPUI + gpui-component |
| Text buffer | ropey (gpui-component's rope inside the editor) |
| Syntax | tree-sitter 0.26 |
| Language servers | lsp-types + own JSON-RPC client (`kalem-lsp`) |
| Debugging | own DAP client (`kalem-dap`) |
| Terminal | portable-pty + a terminal emulator crate |
| Git | git CLI first, gix later |
| Hosting platforms | reqwest + keyring + OAuth device flow |
| Settings, keymaps | TOML |
| Plugins | wasmtime |
| Automation | MCP server on the shared command registry |
| Markdown preview | pulldown-cmark; OS web view for rendered output |
| Updates | GitHub Releases check |

## First GUI milestone

1. Open a folder or `.kalem` file.
2. Solution Explorer with sync with active document.
3. Editor tabs with highlighting.
4. Git Changes panel.
5. Test Results panel.
6. Dock layout (replace the fixed layout in the spike).
