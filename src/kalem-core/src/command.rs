use std::collections::BTreeMap;
use std::fmt;

/// Result of running a command: a short human/agent readable message.
pub type CommandResult = Result<String, CommandError>;

#[derive(Debug, PartialEq, Eq)]
pub enum CommandError {
    Unknown(String),
    Disabled(String),
    Failed(String),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::Unknown(id) => write!(f, "unknown command: {id}"),
            CommandError::Disabled(id) => write!(f, "command is disabled: {id}"),
            CommandError::Failed(msg) => write!(f, "command failed: {msg}"),
        }
    }
}

impl std::error::Error for CommandError {}

type Handler = Box<dyn Fn(&[String]) -> CommandResult + Send + Sync>;
type EnabledFn = Box<dyn Fn() -> bool + Send + Sync>;

/// A single IDE action.
pub struct Command {
    pub id: &'static str,
    pub title: &'static str,
    enabled: EnabledFn,
    handler: Handler,
}

impl Command {
    pub fn new(
        id: &'static str,
        title: &'static str,
        handler: impl Fn(&[String]) -> CommandResult + Send + Sync + 'static,
    ) -> Self {
        Self {
            id,
            title,
            enabled: Box::new(|| true),
            handler: Box::new(handler),
        }
    }

    /// Attach an "is enabled" predicate (e.g. "needs an open file").
    pub fn enabled_when(mut self, f: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.enabled = Box::new(f);
        self
    }

    pub fn is_enabled(&self) -> bool {
        (self.enabled)()
    }
}

/// The single place where commands live. UI, CLI and MCP all go through it.
#[derive(Default)]
pub struct CommandRegistry {
    commands: BTreeMap<&'static str, Command>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, command: Command) {
        self.commands.insert(command.id, command);
    }

    /// All commands, sorted by id (stable output for CLI and agents).
    pub fn list(&self) -> impl Iterator<Item = &Command> {
        self.commands.values()
    }

    pub fn execute(&self, id: &str, args: &[String]) -> CommandResult {
        let cmd = self
            .commands
            .get(id)
            .ok_or_else(|| CommandError::Unknown(id.to_string()))?;
        if !cmd.is_enabled() {
            return Err(CommandError::Disabled(id.to_string()));
        }
        (cmd.handler)(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_registered_command() {
        let mut reg = CommandRegistry::new();
        reg.register(Command::new("test.echo", "Echo", |a| Ok(a.join(" "))));
        let out = reg.execute("test.echo", &["hi".into(), "there".into()]);
        assert_eq!(out, Ok("hi there".to_string()));
    }

    #[test]
    fn unknown_and_disabled_are_errors() {
        let mut reg = CommandRegistry::new();
        reg.register(Command::new("test.off", "Off", |_| Ok(String::new())).enabled_when(|| false));
        assert_eq!(
            reg.execute("nope", &[]),
            Err(CommandError::Unknown("nope".into()))
        );
        assert_eq!(
            reg.execute("test.off", &[]),
            Err(CommandError::Disabled("test.off".into()))
        );
    }

    #[test]
    fn list_is_sorted() {
        let mut reg = CommandRegistry::new();
        reg.register(Command::new("b.cmd", "B", |_| Ok(String::new())));
        reg.register(Command::new("a.cmd", "A", |_| Ok(String::new())));
        let ids: Vec<_> = reg.list().map(|c| c.id).collect();
        assert_eq!(ids, vec!["a.cmd", "b.cmd"]);
    }
}
