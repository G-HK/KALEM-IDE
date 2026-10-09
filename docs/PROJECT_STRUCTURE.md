# Project structure

```
kalem-ide/
├── src/                     all source code (one Cargo crate per folder)
│   ├── kalem/               CLI entry point (lib + bin)
│   ├── kalem-core/          command registry, text buffer, project explorer tree
│   ├── kalem-lang/          language packs; languages/*.toml live here
│   ├── kalem-vcs/           Git backend, forge detection, Git Changes overview
│   ├── kalem-solution/      .kalem, .slnx, .sln and workspace readers
│   ├── kalem-test/          test results model, JUnit reader
│   ├── kalem-lsp/           LSP client (stub)
│   └── kalem-dap/           DAP client (stub)
├── tests/                   integration / end-to-end tests and fixtures
├── docs/                    design docs (this folder)
├── deployments/             packaging scripts and release notes
├── .github/workflows/       CI and Release (manual triggers)
└── root files               Cargo.toml, README, licenses, rustfmt, .gitignore, .gitattributes, .editorconfig
```

## Rules

- **Unit tests** sit next to the code (`#[cfg(test)]` in each crate). **Integration tests** and sample data go in `tests/`.
- Dependencies point downward: `kalem` -> `kalem-*`; the `kalem-*` crates do not depend on `kalem`.
- Language support is data (`src/kalem-lang/languages/*.toml`), not code.
- Notes, plans and reference screenshots stay outside the repo (in the Vault); only technical docs are in `docs/`.
