# UI direction

Target: the familiar **Visual Studio-style dark layout**. Panels, positions and workflows are copied; icons, logo,
wording of branding and exact artwork are **not** (clean-room). Kalem draws its own icons and uses its own palette.

## Layout (from the reference screenshot)

```
┌ menu: File Edit View Git Project Build Debug Test Tools Extensions Window Help   [Search]  [solution name] ┐
├ toolbar: back/fwd · open/new · save/save all · undo/redo │ startup project ▾ │ configuration ▾ │ platform ▾ │ run profile ▾ ▶ ▷ ⟳ ┤
│ auto-hide │                                                             │ Solution/Project Explorer      │ auto-hide │
│ tool win- │                  editor area (tabs, splits)                 │ ├ search box                   │ tool win- │
│ dows (L)  │                                                             │ ├ solution → folders → projects│ dows (R)  │
│           ├─────────────────────────────────────────────────────────────┤ └ tabs: Explorer · Git Changes │           │
│           │ bottom dock: [Terminal][Error List][Task List][Test Explorer][Output][Test Results]         │ │           │
├ status bar: activity message … │ push/pull counts · pending edits · branch · repository · notifications ┘
```

### Regions
1. **Menu bar** with a global search box and the solution/workspace name in the centre.
2. **Toolbar:** startup-project, configuration (Debug/Release), platform and run-profile dropdowns, Start (debug),
   Start without debugging, hot reload, restart.
3. **Editor area** in the middle: tabbed, splittable.
4. **Right dock:** Project Explorer (tree with a search box and view toggles), tabbed with Git Changes and an
   AI/agent panel. The focused panel gets an accent outline.
5. **Bottom dock:** tabs for Terminal, Error List, Task List, Test Explorer, Output (with a "show output from"
   source picker), Test Results.
6. **Edge tool windows:** collapsed, vertical-text tabs on the left and right edges (e.g. database explorer,
   Notifications, Properties) that slide out on click (auto-hide).
7. **Status bar:** current activity (e.g. "Restoring packages…"), outgoing/incoming commits, pending changes,
   branch, repository, notification bell.

### Project Explorer tree
Solution (with "N of N projects") → solution folders and real folders → projects → Dependencies and files.
Source-control state shown on each item (small badge). Works for `.sln`/`.csproj`, Cargo workspaces, Gradle/Maven,
CMake, package.json and plain folders.

## Project Explorer panel

An overview of the project and everything in it.

- **Header line:** the solution/workspace name with counts (for example "39 of 39 projects"), from `Explorer::stats`.
- **Tree:** solution folders and real folders first, then files (case-insensitive); projects expand into
  Dependencies and their files. `Explorer::visible_rows` gives the flattened rows for a virtualised list.
- **Sync with active document:** a toggle in the panel toolbar. When on, switching editor tabs expands the folders
  above the file and selects it (`Explorer::reveal`). Also available as a one-off "Locate in explorer" command.
- **Source-control badges** on every item and on folders that contain changes (`overview::badge`).
- **Toolbar:** switch view (solution / folders / pending changes), sync with active document, collapse all,
  show all files, properties, preview selected item.
- **Search box** filters the tree as you type.

## Git Changes panel (tab next to the explorer)

Top to bottom, as in the reference:

1. **Title:** "Git Changes - <repository>", pin and close.
2. **Branch row:** branch picker, then Fetch, Pull, Push and Sync buttons and an overflow menu. The primary action
   follows the state (`SyncAction`: fetch when level, pull when behind, push when ahead, sync when both).
3. **Counts row:** outgoing/incoming commits (`0 / 0`), a "View all commits" link, and a pull-request / branch
   actions menu on the right.
4. **Commit message box** with a "required" placeholder, a generate-message button and a link-issue (`#`) button.
5. **Commit button** (split button: Commit All, Commit Staged, Commit and Push, Commit and Sync) with an
   **Amend** checkbox. Enabled/disabled and its label come from `ChangesOverview::commit_button`:
   disabled with a reason for "nothing to commit", "enter a commit message" and "resolve merge conflicts first".
6. **Sections:** *Related Items* (linked issues, collapsed), *Changes* (staged / unstaged / untracked with stage-all and
   view-diff actions; empty state "There are no changes in the working directory."), *Stashes* (collapsed).

Implemented so far: the data and rules (`ChangesOverview`, `SyncAction`, `CommitButton`, `badge`) and the CLI
command `kalem vcs.overview`. The panels themselves need the UI toolkit.

## Docking requirements
- Dock, tab, float, auto-hide and re-dock any panel; layouts saved per workspace and as named presets.
- Presets: "Visual Studio" (the layout above), "Compact", "Debug" (call stack, locals, watch open) and "Test".
- Panels are plugins registered by command id, so an agent can open and focus them too.

## Theme
Dark neutral greys with a single violet accent for focus and selection; light and high-contrast themes later.
All colours live in one theme file (design tokens), not in widget code.

## Reference image
The screenshot used for this direction is a private reference kept in the author's notes, not in this repository
(it is a third-party product UI and shows a private solution).

## UI toolkit
Still open: egui + wgpu with a docking crate, GPUI, or Tauri with a web UI. Whatever is chosen must support: tabbed
dock with drag-to-rearrange, auto-hide edge bars, a virtualised tree, and a fast text editor view.
