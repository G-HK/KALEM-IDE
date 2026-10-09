//! Debug Adapter Protocol client.
//!
//! Kalem talks to any open debug adapter (`lldb-dap`, `gdb --interpreter=dap`, `debugpy`,
//! `dlv dap`, `netcoredbg`, ...) over its stdin/stdout. This crate has no UI and no async
//! runtime: a reader thread routes adapter messages, requests block with a timeout, and
//! events arrive on a channel, so the IDE drives it from background threads.
//!
//! * [`Client`]: the wire protocol (framing, request/response matching, events).
//! * [`Session`]: the debugging conversation (launch, breakpoints, stepping, stack, variables).

mod client;
mod session;

pub use client::{encode, read_message, Client, DapError, Event};
pub use session::{
    adapter_command, Breakpoint, DebugEvent, LaunchConfig, Scope, Session, StackFrame, Thread,
    Variable,
};
