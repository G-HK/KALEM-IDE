# tests/

Integration and end-to-end tests. Unit tests stay next to their code (`src/*/src/**`, `#[cfg(test)]`), which is the Rust convention.

| File | Covers |
|---|---|
| `cli.rs` | the command registry as the `kalem` binary uses it |
| `junit.rs` | JUnit parsing on real-world reports |
| `explorer_vcs.rs` | explorer reveal and forge detection |
| `languages.rs` | shipped language packs |
| `fixtures/` | sample inputs (JUnit XML from pytest, Maven Surefire) |

Run: `cargo test --workspace`
