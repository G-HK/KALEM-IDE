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

## Test matrix (planned)

For each language a tiny fixture program and a scripted DAP session: hit a breakpoint, step, read locals,
evaluate an expression, exit cleanly. Run on Linux, macOS (Intel and ARM) and Windows in CI.

## Known weak spots

- Native debugging on Windows (PDB support in LLDB is less polished than Visual Studio's debugger).
- macOS needs the Developer Tools permission and, for own binaries, the `get-task-allow` entitlement.
- Microsoft's `vsdbg` is licensed for Microsoft products only and is not used.
