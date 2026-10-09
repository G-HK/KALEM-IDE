//! Kalem IDE core.
//!
//! Design rule (borrowed from FilmCraft): every user-visible action is a
//! *command* with an id, a title and an enabled state. The UI, the CLI and the
//! future MCP server all dispatch the same commands, so a human and an AI agent
//! drive the IDE in exactly the same way.

pub mod buffer;
pub mod command;
pub mod explorer;

pub use buffer::Buffer;
pub use command::{Command, CommandError, CommandRegistry};
pub use explorer::{Explorer, RevealPlan};
