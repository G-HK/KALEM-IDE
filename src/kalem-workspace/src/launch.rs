//! Run profiles: named ways of starting a project, kept in `.kalem-ide/launch.toml`.
//!
//! The Run button runs the selected profile. "Default" is the language pack's own run command;
//! every `[[profile]]` of the file is another entry of the dropdown next to the button.

use crate::tasks::{Config, TaskKind, TaskPlan};
use crate::Workspace;

/// Where the profiles live, relative to the workspace root.
pub const FILE: &str = ".kalem-ide/launch.toml";

/// What a new file starts with.
pub const TEMPLATE: &str = r#"# Run profiles: they appear in the list next to the Run button.
# Every [[profile]] is one way of starting a project. Only `name` is required.

# [[profile]]
# name = "Serve on 8080"
# command = "cargo run"              # default: the project's normal run command
# args = ["--", "--port", "8080"]    # added after the command
# project = "server"                 # project folder to run in (default: the startup project)
# cwd = "server"                     # working folder, relative to this folder (default: the project)
# env = { RUST_LOG = "debug" }       # environment variables

# [[profile]]
# name = "Script"
# command = "python tools/seed.py"
"#;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Profile {
    pub name: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub project: Option<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
}

impl Profile {
    /// One line for a menu: the name and what it runs.
    pub fn summary(&self) -> String {
        match &self.command {
            Some(c) => format!("{}  ({c})", self.name),
            None => self.name.clone(),
        }
    }
}

/// Split a command line into words; single and double quotes keep spaces together.
pub fn split_command(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut quote, mut any) = (Vec::new(), String::new(), None, false);
    for c in line.chars() {
        match (c, quote) {
            ('"' | '\'', None) => {
                quote = Some(c);
                any = true;
            }
            (q, Some(open)) if q == open => quote = None,
            (c, None) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (c, _) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Read the profiles of a launch file. A broken entry is an error, so a typo is not silently
/// dropped from the menu.
pub fn parse(text: &str) -> Result<Vec<Profile>, String> {
    let t: toml::Table = text.parse().map_err(|e| format!("launch.toml: {e}"))?;
    let Some(list) = t.get("profile") else {
        return Ok(Vec::new());
    };
    let list = list
        .as_array()
        .ok_or("launch.toml: `profile` must be written as [[profile]]")?;
    let mut out: Vec<Profile> = Vec::new();
    for (i, v) in list.iter().enumerate() {
        let tab = v.as_table().ok_or("launch.toml: bad [[profile]]")?;
        let text = |k: &str| tab.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let name = text("name")
            .filter(|n| !n.trim().is_empty())
            .ok_or_else(|| format!("launch.toml: profile {} has no name", i + 1))?;
        if out.iter().any(|p| p.name == name) {
            return Err(format!("launch.toml: two profiles are called \"{name}\""));
        }
        let args = tab
            .get("args")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let env = tab
            .get("env")
            .and_then(|v| v.as_table())
            .map(|e| {
                e.iter()
                    .map(|(k, v)| {
                        let val = match v {
                            toml::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        (k.clone(), val)
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(Profile {
            name,
            command: text("command").filter(|c| !c.trim().is_empty()),
            args,
            project: text("project"),
            cwd: text("cwd"),
            env,
        });
    }
    Ok(out)
}

impl Workspace {
    /// The profiles of this workspace (empty when there is no launch file) and the problem with
    /// the file, if it has one.
    pub fn profiles(&self) -> (Vec<Profile>, Option<String>) {
        match self.read_file(FILE) {
            Err(_) => (Vec::new(), None),
            Ok(text) => match parse(&text) {
                Ok(p) => (p, None),
                Err(e) => (Vec::new(), Some(e)),
            },
        }
    }

    /// Create the launch file with its explanation, unless it exists. Returns its path.
    pub fn ensure_launch_file(&self) -> Result<&'static str, String> {
        let path = self.root.join(FILE);
        if !path.exists() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{FILE}: {e}"))?;
            }
            std::fs::write(&path, TEMPLATE).map_err(|e| format!("{FILE}: {e}"))?;
        }
        Ok(FILE)
    }

    /// What the default Run command would be for a project, as one line (for the menu).
    pub fn default_run_line(&self, startup_dir: Option<&str>, config: Config) -> Option<String> {
        self.plan(TaskKind::Run, startup_dir, config)
            .ok()
            .map(|p| p.command_line())
    }

    /// The command that starts `profile`. Without its own `command` it runs the project's normal
    /// run command, with the profile's `args` added.
    pub fn profile_plan(
        &self,
        profile: &Profile,
        startup_dir: Option<&str>,
        config: Config,
    ) -> Result<TaskPlan, String> {
        let dir = profile.project.as_deref().or(startup_dir);
        let mut plan = match &profile.command {
            None => self.plan(TaskKind::Run, dir, config)?,
            Some(line) => {
                let mut words = split_command(line).into_iter();
                let program = words.next().ok_or("The profile's command is empty")?;
                let cwd = match dir {
                    Some(d) if !d.is_empty() => self.root.join(d),
                    _ => self.root.clone(),
                };
                TaskPlan {
                    title: String::new(),
                    program,
                    args: words.collect(),
                    cwd,
                    env: Vec::new(),
                }
            }
        };
        plan.args.extend(profile.args.iter().cloned());
        if let Some(c) = &profile.cwd {
            plan.cwd = self.root.join(c);
        }
        plan.env = profile.env.clone();
        plan.title = format!("Run {}", profile.name);
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn splits_quoted_words() {
        assert_eq!(
            split_command(r#"dotnet run --urls "https://a b" 'x y' """#),
            ["dotnet", "run", "--urls", "https://a b", "x y", ""]
        );
        assert!(split_command("   ").is_empty());
    }

    #[test]
    fn reads_profiles() {
        let p = parse(
            r#"
[[profile]]
name = "a"
command = "cargo run"
args = ["--", "x"]
env = { RUST_LOG = "debug", N = 3 }

[[profile]]
name = "b"
project = "srv"
"#,
        )
        .unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].command.as_deref(), Some("cargo run"));
        assert_eq!(p[0].args, ["--", "x"]);
        assert!(p[0].env.contains(&("RUST_LOG".into(), "debug".into())));
        assert!(p[0].env.contains(&("N".into(), "3".into())));
        assert_eq!(p[1].project.as_deref(), Some("srv"));
        assert!(parse("").unwrap().is_empty());
        assert!(parse(TEMPLATE).unwrap().is_empty());
    }

    #[test]
    fn rejects_bad_files() {
        assert!(parse("[[profile]]\ncommand = \"x\"").is_err());
        assert!(parse("[[profile]]\nname=\"a\"\n[[profile]]\nname=\"a\"").is_err());
        assert!(parse("profile = 3").is_err());
        assert!(parse("= =").is_err());
    }

    #[test]
    fn plans_a_profile() {
        let d = std::env::temp_dir().join(format!("kalem-launch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        let ws = Workspace::open(&d).unwrap();
        // No file: no profiles, and the file is created from the template on request.
        assert!(ws.profiles().0.is_empty());
        ws.ensure_launch_file().unwrap();
        assert!(fs::read_to_string(d.join(FILE))
            .unwrap()
            .contains("[[profile]]"));
        let with_args = Profile {
            name: "p".into(),
            args: vec!["--".into(), "-v".into()],
            env: vec![("A".into(), "1".into())],
            ..Default::default()
        };
        let plan = ws.profile_plan(&with_args, None, Config::Debug).unwrap();
        assert_eq!(plan.command_line(), "cargo run -- -v");
        assert_eq!(plan.env, [("A".to_string(), "1".to_string())]);
        assert_eq!(plan.title, "Run p");
        let custom = Profile {
            name: "q".into(),
            command: Some("python tools/seed.py".into()),
            cwd: Some("tools".into()),
            ..Default::default()
        };
        let plan = ws.profile_plan(&custom, None, Config::Debug).unwrap();
        assert_eq!(plan.command_line(), "python tools/seed.py");
        assert_eq!(plan.cwd, d.join("tools"));
        let _ = fs::remove_dir_all(&d);
    }
}
