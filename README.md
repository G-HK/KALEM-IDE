# Kalem IDE

A multi-language, cross-platform IDE with a Visual Studio-style workflow, written in Rust.
*Kalem* is Turkish for "pen".

> **Status: very early.** Today this repository contains the project skeleton: a command
> registry, language-pack loading and a small CLI. The editor UI, LSP client and debugger
> UI are on the [roadmap](docs/ROADMAP.md).

## Goals

- **Many languages out of the box:** C / C++, Rust, C# / .NET, Python, TypeScript / JavaScript, Go, Java and Kotlin.
- **Code, run, debug and preview** in one place, on **Linux, macOS and Windows**.
- **One debugger UI for everything**, built on the Debug Adapter Protocol; language servers via LSP.
- **Agent-native:** every action is a typed command that the UI, the CLI and an MCP server all share.
- **Clean-room:** it copies the *workflow* people know, not any proprietary code, icons or names.
  It is not affiliated with or endorsed by Microsoft.

## Language packs

A language is a small TOML file in [`languages/`](languages), not code:

```toml
[language]
id = "kotlin"
name = "Kotlin (JVM)"
extensions = ["kt", "kts"]

[lsp]
command = "kotlin-lsp"

[debug]
adapter = "kotlin-debug-adapter"
```

Debug adapters can be overridden per OS (`[debug.windows]`, `[debug.macos]`, `[debug.linux]`).
Language servers and debug adapters are downloaded on first use, not bundled.

## Try it

```
cargo run -p kalem -- help
cargo run -p kalem -- languages.list
cargo test
```

## Install

Prebuilt downloads for Windows, macOS and Linux are attached to each
[GitHub Release](../../releases). Verify the SHA256 checksum next to each file.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Roadmap](docs/ROADMAP.md)
- [Debugging on every platform](docs/DEBUGGING.md)

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
