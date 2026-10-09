# Architecture

Layered Cargo workspace. Nothing below the front ends depends on a UI toolkit or OS API.

| Layer | Crates | Notes |
|---|---|---|
| Core | `kalem-core` | Command registry, text buffer (rope + undo tree planned) |
| Protocol clients | `kalem-lsp`, `kalem-dap` | LSP and DAP over stdio / TCP (planned) |
| Language packs | `kalem-lang`, `languages/*.toml` | Declarative manifests wiring tools together |
| Front ends | `apps/kalem` | CLI now; GUI and MCP server later |

## Rules

1. **Everything is a command** (`kalem-core::Command`): an id, a title, an enabled state and typed arguments.
   UI, CLI and the future MCP server dispatch the same commands.
2. **Languages are data.** New language = new TOML file + a test that it parses.
3. **Adapters are downloaded, never bundled**, and checksum-verified.
4. **Clean-room.** Specs come from public protocols and documentation. No decompiled or proprietary code.

## Planned UI stack

Options under evaluation: egui + wgpu (fast to build, desktop + web), GPUI (better text rendering),
or Tauri + a web editor component. The UI will be a single crate over the core so it can be swapped.
