# Desktop app (milestone 1)

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
| Git Changes | Branch, ahead/behind, Staged and Changes lists; click a row to open the file |
| Test Results | JUnit XML found in the folder; failures first |
| Header | Menu bar and toolbar; Back/Forward, Open, Save, Undo/Redo, startup project, Debug/Release, Run, Build, Test, Cancel, Refresh |
| Build / Run / Test | Run the language pack's `build`, `run` and `test` command in the startup project's folder; output streams into the Output tab. Release adds `--release` (cargo) or `-c Release` (dotnet) |
| Search | Quick Open (Ctrl+P) over the workspace files |
| Themes | See `THEMES.md` |

Build: `cargo build -p kalem-ui` (not part of the default workspace build: GPUI is heavy and needs
system libraries). Not done yet: debugging (the Debug button and menu item are disabled), drag docking, commit/stage actions, text search in files, editor syntax colours in themes. Tools and Extensions menus are empty.
