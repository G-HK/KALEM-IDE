# Solutions and the `.kalem` file

The Solution Explorer needs one root that says "these projects belong together". Kalem finds it in this order:

1. `<Name>.kalem` – Kalem's own format, works across languages
2. `<Name>.slnx` – Visual Studio's XML solution
3. `<Name>.sln` – classic Visual Studio solution
4. A workspace manifest: `Cargo.toml` `[workspace]`, `package.json` `workspaces`, `go.work`
5. Nothing: the folder opens as plain files and projects are auto-detected

`.kalem` is committed to the repository. Per-user state (open tabs, breakpoints, layout) lives in `.kalem/`, which is gitignored.

## Format (TOML)

```toml
[solution]
name = "Veto"                  # optional; defaults to the file name

[[project]]
path = "backend"               # relative, forward slashes, must stay inside the solution
language = "rust"              # optional; guessed from the path (Cargo.toml, *.csproj, package.json, ...)

[[project]]
path = "services/api/Api.csproj"
name = "Api"                   # optional; defaults to the file or folder name
startup = true                 # what F5 runs; several allowed (multi-startup)
folder = "Services"            # optional virtual folder

[[folder]]                     # virtual folder holding loose files
name = "Docs"
items = ["docs/**", "README.md"]
```

Rules: paths are relative and may not contain `..` or start with `/`; no GUIDs; unknown keys are ignored so older Kalem versions can open newer files.

## Language guessing

| Path | Language pack |
|---|---|
| `*.csproj`, `*.fsproj`, `*.vbproj` | csharp |
| `Cargo.toml` | rust |
| `package.json`, `tsconfig.json` | typescript |
| `go.mod` | go |
| `pom.xml`, `build.gradle` | java |
| `build.gradle.kts` | kotlin |
| `CMakeLists.txt`, `*.vcxproj` | cpp |
| `pyproject.toml`, `setup.py` | python |

## Implementation

`src/kalem-solution`: pure parsers (`parse_kalem`, `parse_slnx`, `parse_sln`, `parse_cargo_workspace`, `parse_package_json`, `parse_go_work`) plus `load(dir)`. Try it: `kalem solution.show <folder>`.
Next: feed the result into the Explorer tree and add "Sync with active document" for solution items.
