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

## Side panel title bar and auto-hide

The right dock has a title bar like Visual Studio's: the panel name, a window-position menu
(Dock, Auto Hide, Close), a pin button and a close button. Unpinning (or Auto Hide) collapses the
dock to a strip of icons on the right edge; clicking an icon slides the panel over the editor, and
clicking in the editor slides it away again. Pin it to dock it back. After Close, View >
Solution Explorer / Git Changes brings it back. The pin state is not saved between runs yet.
