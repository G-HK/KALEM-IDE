# Debugging on every platform

One debugger UI built on the Debug Adapter Protocol. Platform-specific work lives in the adapter process.

| Language | Linux | macOS | Windows | Adapter |
|---|---|---|---|---|
| C / C++ / Rust | Good | Good | Okay | lldb-dap, gdb (DAP mode) |
| C# / .NET | Good | Good | Good | netcoredbg |
| Python | Good | Good | Good | debugpy |
| TS / JS | Good | Good | Good | js-debug |
| Go | Good | Good* | Good | delve |
| Java | Good | Good | Good | java-debug |
| Kotlin (JVM) | Good | Good | Good | kotlin-debug-adapter / java-debug |

\*Go on macOS needs the developer-tools permission and sometimes code signing.

## What works today

- `kalem-dap`: the DAP client (framing, request matching, events) and a debugging session
  (launch, breakpoints, step, stack, variables, evaluate). `KALEM_DAP_TRACE=1` prints every message.
- The IDE: **F9** toggles a breakpoint on the cursor line, **F5** builds and starts (or continues),
  **F10 / F11 / Shift+F11** step over / into / out, **Shift+F5** stops. The Debug tab shows the call
  stack, variables and breakpoints; the stopped line is highlighted in the editor.
- Rust projects, and any project whose language pack `run` command is a `./path` program.
  Adapter: the pack's choice for your OS, or `gdb` (DAP mode, gdb 14+) when that one is not installed.
- Tested against real gdb 15 (`src/kalem-dap/tests/gdb.rs`, skipped where gdb or a C compiler is missing).

Not yet: gutter clicks for breakpoints, conditions and logpoints, watch expressions, expanding
structures, debugging in torn-off editor windows, program arguments and environment settings,
other languages' launch settings.

## Test matrix (planned)

For each language a tiny fixture program and a scripted DAP session: hit a breakpoint, step, read locals,
evaluate an expression, exit cleanly. Run on Linux, macOS (Intel and ARM) and Windows in CI.

## Known weak spots

- Native debugging on Windows (PDB support in LLDB is less polished than Visual Studio's debugger).
- macOS needs the Developer Tools permission and, for own binaries, the `get-task-allow` entitlement.
- Microsoft's `vsdbg` is licensed for Microsoft products only and is not used.
