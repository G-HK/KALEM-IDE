//! Language Server Protocol client.
//!
//! Kalem talks to the language server named in a language pack (`[lsp] command`) over its
//! stdin/stdout. Like `kalem-dap` this crate has no UI and no async runtime: a reader thread
//! routes server messages, requests block with a timeout, notifications arrive on a channel,
//! and the IDE drives it from background threads.
//!
//! * [`Client`]: JSON-RPC with `Content-Length` framing.
//! * [`Server`]: the handshake, document sync and the requests the editor needs
//!   (completion, hover, go to definition, diagnostics).

mod client;
mod edits;
mod server;
mod uri;

pub use client::{encode, read_message, Client, LspError, Notification};
pub use edits::{apply_edits, flatten};
pub use lsp_types;
pub use server::Server;
pub use uri::{path_to_uri, uri_to_path};
