# Desktop app

`kalem-ide [folder | .kalem | .slnx | .sln]` opens the window. Code: `src/kalem-ui` (drawing only) on
`src/kalem-workspace` (open folder, file list, git status, JUnit results; tested in CI).

Layout, as in Visual Studio: editor tabs in the centre, **Solution Explorer on the right**, and a
bottom panel with **Git Changes** and **Test Results**, plus a status bar. Panels resize by dragging.

| Feature | Notes |
|---|---|
| Open | Toolbar button or Ctrl/Cmd+O, or pass a path on the command line |
| Solution Explorer | Files from `git ls-files` (honours `.gitignore`), else a folder walk |
| Sync with active document | Toolbar toggle; expands the folders above the active tab and selects it |
| Editor | Tabs, line numbers, tree-sitter highlighting, Ctrl/Cmd+S saves, `*` marks unsaved |
| Git Changes | Branch picker, Fetch / Pull / Push / Sync, outgoing / incoming counts, "View all commits" (graph in Output), Staged and Changes lists; click a row to open the file |
| Test Results | JUnit XML found in the folder; failures first |
| Header | Menu bar and toolbar; Back/Forward, Open, Save, Undo/Redo, startup project, Debug/Release, Run, Build, Test, Cancel, Refresh |
| Build / Run / Test | Run the language pack's `build`, `run` and `test` command in the startup project's folder; output streams into the Output tab. Release adds `--release` (cargo) or `-c Release` (dotnet) |
| Search | Quick Open (Ctrl+P) over the workspace files |
| Themes | See `THEMES.md` |

Run and build instructions: [RUNNING.md](RUNNING.md). Since milestone 1 the app also has Find / Replace in Files, a Git panel with diffs, tab tear-off, a debugger and language-server features; see the [roadmap](ROADMAP.md) for the current state and what is missing.

## Side panels: dock, auto-hide, edge strip

Each right-dock panel (Solution Explorer, Git Changes, Find) has a title bar with a
window-position menu (Dock / Auto Hide), a pin button and a close button, like Visual Studio.

- **Docked** panels share the dock; their tabs are at the bottom.
- **Auto-hidden** panels (pin button, or the X) collapse to a strip on the right edge, shown as
  vertical tabs. Click a tab to slide the panel over the editor; click in the editor to hide it
  again; pin it to dock it back. The X never loses a panel: it stays on the strip.
- View > Solution Explorer / Git Changes always brings a panel back.

The pin state is not saved between runs yet.

## Diff view

Clicking a file in **Git Changes** opens a diff tab (side by side or unified).

- Changed blocks are tinted across both panes and the gutter: blue = changed, green = added, grey = removed. A ribbon joins the lines of one side to the lines of the other (5 lines become 1, 0 become 3, ...), and the changed words inside a modified line are picked out.
- **Working Tree** diffs are editable: the left pane is the index version (read-only), the right pane ("Current version") is the real file. Edit it and the differences, highlights and ribbons are recomputed on every keystroke; **Ctrl+S** saves the file. The left pane follows the right one when you scroll. If the same file is open in a normal tab, both tabs share one editor.
- **Staged** diffs and diffs in a torn-off window are read-only.
- Previous / next difference buttons jump between blocks.
