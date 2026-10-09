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

## Editor bottom bar

- **Zoom** dropdown (50 %-400 %), also **Ctrl+=** / **Ctrl+-** / **Ctrl+0** and **Ctrl + mouse wheel** over the editor (up = zoom in). One zoom for every editor, kept in the settings (`zoom`).
- **Error and warning counts** of the open file (from the language server) with **up / down arrows**; **F8** / **Shift+F8** go to the next / previous problem and wrap around. The status bar shows "Error 2 of 5: message".

## Run profiles

The **Run** button shows what it will run and has a dropdown next to it: **Default** (the project's own command, e.g. `cargo run`), every profile from `.kalem-ide/launch.toml`, and **Configure Run Profiles...**, which creates the file with commented examples and opens it. A profile has `name`, `command` (or `project`), `args`, `cwd` and `env`:

```toml
[[profile]]
name = "Server (verbose)"
command = "cargo run --release"
args = ["--", "--port", "8080"]
env = { RUST_LOG = "debug" }
```

## Syntax colours

Colours for Rust, Python, JS/TS/TSX, Go, Java, Kotlin, C/C++, C# (also `.csproj`/XML), Razor (`.razor`, `.cshtml`: markup, directives and C#), Markdown, TOML, YAML, JSON, Bash, HTML/CSS, PHP, Ruby, Lua, Make/CMake, Diff, Proto, GraphQL, Scala. File types are picked by extension (`language_for`).

## Solution Explorer file nesting

Like Visual Studio, related files sit under their parent file: `Gauge.razor` > `Gauge.razor.cs`, `Gauge.razor.css`; `Main.xaml` > `Main.xaml.cs`; `Form1.cs` > `Form1.Designer.cs`; `site.scss` > `site.css`, `site.min.css`; `appsettings.json` > `appsettings.Development.json`. Clicking the parent opens it and expands/collapses the group; Sync with Active Document expands it for a nested file.

## Tabs

- **Dragging a tab**: the tab dims, a label follows the mouse, and an insertion bar with arrows shows the gap between tabs where it will land (drag within the strip to reorder, onto another Kalem window's strip to move it there). That window gets a frame and "Release to move ... here". Over nothing of Kalem's, the window shows "Release to open ... in a new window".
- **Right-click a tab**: Save, Close, Close Other Tabs, Close All Tabs, Restore Closed Tab (**Ctrl+Shift+T**), Move To New Window, All To New Window, Move >, Pin Tab (pinned tabs gather at the left, show a pin and survive Close All/Others), Set Tab Color >, Locate in Solution Explorer, Copy Full Path, Open in File Explorer.
- **Right-click the empty part of the strip**: Close All Tabs, Restore Closed Tab, All To New Window.

## Opening a project

- **Open Folder...** (Ctrl+O) picks a folder. The system dialog can only show folders in this mode, so files are not listed.
- **Open Solution or Project File...** (Ctrl+Shift+O, or the layers button in the toolbar) shows files: pick a `.sln`, `.slnx` or `.kalem` file, or a project file of a supported language: C# (`.csproj`), Rust (`Cargo.toml`), Go (`go.mod`, `go.work`), Java/Kotlin (`pom.xml`, `build.gradle(.kts)`), JS/TS (`package.json`), Python (`pyproject.toml`, `requirements.txt`, `setup.py`). Its folder is opened, and a picked solution file is the one used even if the folder holds several.

## Git Changes

- Changed files are shown as a **folder tree** (folders first; a chain of folders with nothing else in it is one row, e.g. `src/Veto/Pages`). A project's folder gets the project icon. Click a folder to fold it; its **+ / -** button stages or unstages everything in it. The number is how many changed files it holds.
- The letter on the right is the change: **M** modified, **A** added (new files too), **D** deleted, **R** renamed. The **undo** button throws away the unstaged changes of a file (an untracked file is deleted); both ask first.
- **Right-click a file**: Open, Stage / Unstage, Undo Changes, View History and Blame (printed in the Output panel; greyed for new files), Compare with Unmodified..., Delete (asks first), Ignore this local item (adds `/path` to `.gitignore`) and Ignore this Extension (adds `*.ext`).
