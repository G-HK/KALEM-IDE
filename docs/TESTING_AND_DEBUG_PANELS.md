# Debug metadata and test overview

Goal: while debugging and testing, the IDE reads the **metadata** the tools expose and shows an overview in
panels, so specific things can be checked quickly.

## Debug metadata (via the Debug Adapter Protocol)

| Panel | Data | DAP source |
|---|---|---|
| Debug Capabilities / Doctor | What this adapter supports (conditional breakpoints, data breakpoints, hot reload, step-back, ...) | `initialize` response |
| Call Stack | Frames per thread, source locations, frame hints | `stackTrace` |
| Locals / Watch / Autos | Scopes, variables, types, values, expandable children; watch expressions | `scopes`, `variables`, `evaluate` |
| Threads / Processes | Thread list, state, process id and name | `threads`, `process` and `thread` events |
| Modules / Loaded Sources | Loaded assemblies, libraries, symbol status, version, path | `modules`, `loadedSources` |
| Breakpoints | Which are verified, moved, or rejected, with the adapter's message | `setBreakpoints`, `breakpoint` events |
| Exceptions | Exception type, message, stack, inner exceptions; break-on-exception settings | `exceptionInfo`, `setExceptionBreakpoints` |
| Debug Console / Output | Output by category (stdout, stderr, console, telemetry) | `output` events |

Adapters differ in what they implement, so every panel is driven by the capability list and says why something is
unavailable instead of staying empty.

## Test metadata

Two inputs, one model (`kalem-test`):
1. **JUnit XML**: the common export. Maven/Gradle (Java, Kotlin), pytest, ctest, cargo nextest, Go via gotestsum,
   dotnet with a JUnit logger, and Jest/Vitest reporters can all write it. **Implemented.**
2. **Discovery** per language (planned): list tests without running them (`pytest --collect-only`,
   `go test -list`, `cargo test -- --list`, `dotnet test --list-tests`, Gradle/Maven reports), so the explorer shows
   tests before they run.

### Test Explorer (tree) and Test Results (overview)
- Tree: suite → class → test, with outcome icons, duration, filters (failed, skipped, slow, changed) and search.
- Run / debug / re-run failed for: all, a suite, a single test, or the test under the cursor. Debugging a test
  reuses the normal debug launch with a test filter.
- Results overview: pass/fail/error/skipped counts, total time, per-suite breakdown, failures with message and stack
  (click to jump to the source line), slowest tests. Implemented as `TestRun::overview`.
- Planned: history across runs (flaky and newly failing tests), code coverage (LCOV/Cobertura) with gutter
  marks, output per test, and rerun-on-save.

## Status
- Implemented: JUnit XML reader, summary, per-suite grouping, failures and slowest lists, and the CLI command
  `kalem test.report <junit.xml>` (checked against real `pytest --junitxml` output).
- Planned: DAP metadata panels (needs the DAP client), test discovery, the panels themselves (needs the UI).
