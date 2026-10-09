//! Build / Run / Test: turn the startup project and its language pack into a command, run it in
//! the background and stream its output. No UI toolkit in here.

use crate::Workspace;
use kalem_lang::LanguagePack;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Build,
    Run,
    Test,
}

impl TaskKind {
    pub fn title(self) -> &'static str {
        match self {
            TaskKind::Build => "Build",
            TaskKind::Run => "Run",
            TaskKind::Test => "Test",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Config {
    #[default]
    Debug,
    Release,
}

impl Config {
    pub fn name(self) -> &'static str {
        match self {
            Config::Debug => "Debug",
            Config::Release => "Release",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    pub name: String,
    /// Folder relative to the workspace root (`""` for the root itself).
    pub dir: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPlan {
    pub title: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Extra environment variables for the process.
    pub env: Vec<(String, String)>,
}

impl TaskPlan {
    pub fn command_line(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Language of a folder from the project files directly in it.
pub fn detect_language(dir: &str, files: &[String]) -> Option<&'static str> {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let names: Vec<&str> = files
        .iter()
        .filter_map(|f| f.strip_prefix(&prefix))
        .filter(|f| !f.contains('/'))
        .collect();
    let ends = |e: &str| names.iter().any(|n| n.to_lowercase().ends_with(e));
    let is = |n: &str| names.contains(&n);
    Some(if is("Cargo.toml") {
        "rust"
    } else if ends(".csproj") || ends(".fsproj") || ends(".sln") || ends(".slnx") {
        "csharp"
    } else if is("go.mod") {
        "go"
    } else if is("pom.xml") || is("build.gradle") {
        "java"
    } else if is("build.gradle.kts") {
        "kotlin"
    } else if is("package.json") || is("tsconfig.json") {
        "typescript"
    } else if is("pyproject.toml") || is("setup.py") || is("requirements.txt") {
        "python"
    } else if is("CMakeLists.txt") || is("Makefile") {
        "cpp"
    } else {
        return None;
    })
}

impl Workspace {
    /// Projects the toolbar can pick as startup project. A plain folder is one project: itself.
    pub fn projects(&self) -> Vec<ProjectInfo> {
        let dirs = self.project_dirs();
        if dirs.is_empty() {
            return vec![ProjectInfo {
                name: self.name(),
                dir: String::new(),
                language: detect_language("", &self.files).map(str::to_string),
            }];
        }
        dirs.into_iter()
            .map(|d| ProjectInfo {
                name: d.rsplit('/').next().unwrap_or(&d).to_string(),
                language: detect_language(&d, &self.files).map(str::to_string),
                dir: d,
            })
            .collect()
    }

    /// Folder of the solution's own startup project, if it names one.
    pub fn default_startup(&self) -> Option<String> {
        let sol = self.solution.as_ref()?;
        let first = sol.startup_projects().into_iter().next()?;
        let paths = vec![first.path.clone()];
        crate::kinds::project_dirs(&paths, &self.files)
            .into_iter()
            .next()
    }

    /// The command for `kind` on the project in `startup_dir` (None: the first project).
    pub fn plan(
        &self,
        kind: TaskKind,
        startup_dir: Option<&str>,
        config: Config,
    ) -> Result<TaskPlan, String> {
        let projects = self.projects();
        let project = match startup_dir {
            Some(d) => projects.iter().find(|p| p.dir == d),
            None => projects.first(),
        }
        .ok_or("No project to run a task on")?;
        let lang = project
            .language
            .as_deref()
            .ok_or_else(|| format!("{}: cannot tell the language of this project", project.name))?;
        let pack: LanguagePack = kalem_lang::builtin()
            .into_iter()
            .find(|p| p.language.id == lang)
            .ok_or_else(|| format!("No language pack for {lang}"))?;
        let tasks = pack.tasks.ok_or_else(|| format!("{lang} has no tasks"))?;
        let line = match kind {
            TaskKind::Build => tasks.build,
            TaskKind::Run => tasks.run,
            TaskKind::Test => tasks.test,
        }
        .ok_or_else(|| format!("{} has no {} command", pack.language.name, kind.title()))?;
        let mut parts = line.split_whitespace().map(str::to_string);
        let program = parts.next().ok_or("Empty task command")?;
        let mut args: Vec<String> = parts.collect();
        if config == Config::Release {
            match program.as_str() {
                "cargo" => args.push("--release".into()),
                "dotnet" => args.extend(["-c".into(), "Release".into()]),
                _ => {}
            }
        }
        let cwd = if project.dir.is_empty() {
            self.root.clone()
        } else {
            self.root.join(&project.dir)
        };
        Ok(TaskPlan {
            title: format!("{} {} ({})", kind.title(), project.name, config.name()),
            program,
            args,
            cwd,
            env: Vec::new(),
        })
    }
}

#[derive(Debug)]
pub enum TaskEvent {
    Line(String),
    /// The process ended (`None`: killed or no exit code).
    Done(Option<i32>),
    /// The process could not be started.
    Failed(String),
}

/// A running task: read events from `rx`, stop it with `cancel`.
pub struct RunningTask {
    pub rx: Receiver<TaskEvent>,
    child: Arc<Mutex<Option<Child>>>,
}

impl RunningTask {
    pub fn cancel(&self) {
        if let Some(c) = self.child.lock().ok().and_then(|mut g| g.take()) {
            let mut c = c;
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn pump<R: std::io::Read + Send + 'static>(r: R, tx: Sender<TaskEvent>) {
    std::thread::spawn(move || {
        for line in BufReader::new(r).lines().map_while(Result::ok) {
            if tx.send(TaskEvent::Line(strip_ansi(&line))).is_err() {
                break;
            }
        }
    });
}

/// Remove ANSI colour escapes (`ESC [ ... letter`).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' && it.peek() == Some(&'[') {
            it.next();
            for d in it.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn spawn(plan: &TaskPlan) -> RunningTask {
    let (tx, rx) = channel();
    let child_slot: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let mut cmd = Command::new(&plan.program);
    cmd.args(&plan.args)
        .envs(plan.env.iter().map(|(k, v)| (k, v)))
        .current_dir(&plan.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    match cmd.spawn() {
        Err(e) => {
            let _ = tx.send(TaskEvent::Failed(format!("{}: {e}", plan.program)));
        }
        Ok(mut child) => {
            if let Some(o) = child.stdout.take() {
                pump(o, tx.clone());
            }
            if let Some(e) = child.stderr.take() {
                pump(e, tx.clone());
            }
            *child_slot.lock().unwrap() = Some(child);
            let slot = child_slot.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(80));
                let mut g = slot.lock().unwrap();
                match g.as_mut() {
                    None => {
                        let _ = tx.send(TaskEvent::Done(None)); // cancelled
                        break;
                    }
                    Some(c) => {
                        if let Ok(Some(status)) = c.try_wait() {
                            // Give the pump threads a moment to flush the last lines.
                            drop(g);
                            std::thread::sleep(std::time::Duration::from_millis(80));
                            let _ = tx.send(TaskEvent::Done(status.code()));
                            break;
                        }
                    }
                }
            });
        }
    }
    RunningTask {
        rx,
        child: child_slot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kalem-task-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn plans_cargo_and_release() {
        let d = scratch("cargo");
        fs::write(d.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        let ws = Workspace::open(&d).unwrap();
        let p = ws.plan(TaskKind::Build, None, Config::Debug).unwrap();
        assert_eq!(p.command_line(), "cargo build");
        let p = ws.plan(TaskKind::Test, None, Config::Release).unwrap();
        assert_eq!(p.command_line(), "cargo test --release");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn plans_dotnet_release_in_project_folder() {
        let d = scratch("dotnet");
        fs::create_dir_all(d.join("App")).unwrap();
        fs::write(d.join("App/App.csproj"), "<Project/>").unwrap();
        fs::write(
            d.join("S.kalem"),
            "[solution]\nname = \"S\"\n\n[[project]]\nname = \"App\"\npath = \"App/App.csproj\"\n",
        )
        .unwrap();
        let ws = Workspace::open(&d).unwrap();
        let p = ws
            .plan(TaskKind::Build, Some("App"), Config::Release)
            .unwrap();
        assert_eq!(p.command_line(), "dotnet build -c Release");
        assert_eq!(p.cwd, d.join("App"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn unknown_language_is_an_error() {
        let d = scratch("none");
        fs::write(d.join("notes.txt"), "x").unwrap();
        let ws = Workspace::open(&d).unwrap();
        assert!(ws.plan(TaskKind::Build, None, Config::Debug).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn strips_ansi() {
        assert_eq!(strip_ansi("\u{1b}[32mok\u{1b}[0m done"), "ok done");
    }

    #[test]
    fn runs_a_process_and_streams_output() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let plan = TaskPlan {
            title: "t".into(),
            program: cargo,
            args: vec!["--version".into()],
            cwd: std::env::temp_dir(),
            env: Vec::new(),
        };
        let t = spawn(&plan);
        let mut lines = Vec::new();
        let mut code = None;
        while let Ok(ev) = t.rx.recv_timeout(std::time::Duration::from_secs(20)) {
            match ev {
                TaskEvent::Line(l) => lines.push(l),
                TaskEvent::Done(c) => {
                    code = c;
                    break;
                }
                TaskEvent::Failed(e) => panic!("{e}"),
            }
        }
        assert_eq!(code, Some(0));
        assert!(lines.iter().any(|l| l.starts_with("cargo ")));
    }

    #[test]
    fn missing_program_fails() {
        let plan = TaskPlan {
            title: "t".into(),
            program: "definitely-not-a-program-kalem".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            env: Vec::new(),
        };
        let t = spawn(&plan);
        assert!(matches!(
            t.rx.recv_timeout(std::time::Duration::from_secs(5)),
            Ok(TaskEvent::Failed(_))
        ));
    }
}
