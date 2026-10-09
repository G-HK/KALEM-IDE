//! Debugging a project: what to build, which program to start, which debug adapter to use.
//! No UI toolkit and no protocol code in here; `kalem-dap` does the talking.

use crate::tasks::{Config, TaskKind, TaskPlan};
use crate::Workspace;
use kalem_lang::{LanguagePack, Os};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugPlan {
    /// Builds the program with debug information.
    pub build: TaskPlan,
    /// The executable (or script) to start under the debugger.
    pub program: PathBuf,
    pub cwd: PathBuf,
    pub adapter: String,
    pub adapter_args: Vec<String>,
    /// Shown when the adapter cannot be started.
    pub install_hint: String,
}

/// Is `name` an executable found on `PATH`?
pub fn on_path(name: &str) -> bool {
    if name.contains(['/', '\\']) {
        return Path::new(name).exists();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        let base = dir.join(name);
        base.is_file() || (cfg!(windows) && base.with_extension("exe").is_file())
    })
}

/// The adapter the pack names for this OS; `gdb` (DAP mode) when that one is missing but gdb
/// is installed.
fn choose_adapter(pack: &LanguagePack) -> Option<(String, Vec<String>, String)> {
    let d = pack.debug.as_ref()?;
    let name = d.adapter_for(Os::current()).to_string();
    let args = d.args.clone();
    let hint = format!(
        "Install `{name}` and make sure it is on your PATH (it is the debug adapter for {}).",
        pack.language.name
    );
    if !on_path(&name) && name != "gdb" && on_path("gdb") {
        return Some(("gdb".into(), Vec::new(), hint));
    }
    Some((name, args, hint))
}

/// Executable of the Rust package in `project_dir`: the first `bin` target of the package whose
/// manifest sits in that folder, inside Cargo's target directory (found by `cargo metadata`).
fn rust_program(project_dir: &Path, config: Config) -> Result<PathBuf, String> {
    let out = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(project_dir)
        .output()
        .map_err(|e| format!("cargo: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let target_dir = v["target_directory"]
        .as_str()
        .ok_or("cargo metadata: no target_directory")?;
    let manifest = project_dir.join("Cargo.toml");
    let same = |p: &str| {
        let a = Path::new(p);
        a == manifest || a.canonicalize().ok() == manifest.canonicalize().ok()
    };
    let bin = v["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["manifest_path"].as_str().is_some_and(same))
        .flat_map(|p| p["targets"].as_array().into_iter().flatten())
        .find(|t| {
            t["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == "bin"))
        })
        .and_then(|t| t["name"].as_str())
        .ok_or("This Rust package has no binary to debug (no `bin` target)")?;
    let profile = if config == Config::Release {
        "release"
    } else {
        "debug"
    };
    let file = if cfg!(windows) {
        format!("{bin}.exe")
    } else {
        bin.to_string()
    };
    Ok(Path::new(target_dir).join(profile).join(file))
}

impl Workspace {
    /// How to debug the startup project (None: the first project).
    pub fn debug_plan(
        &self,
        startup_dir: Option<&str>,
        config: Config,
    ) -> Result<DebugPlan, String> {
        let projects = self.projects();
        let project = match startup_dir {
            Some(d) => projects.iter().find(|p| p.dir == d),
            None => projects.first(),
        }
        .ok_or("No project to debug")?;
        let lang = project
            .language
            .as_deref()
            .ok_or_else(|| format!("{}: cannot tell the language of this project", project.name))?;
        let pack = kalem_lang::builtin()
            .into_iter()
            .find(|p| p.language.id == lang)
            .ok_or_else(|| format!("No language pack for {lang}"))?;
        let (adapter, adapter_args, install_hint) = choose_adapter(&pack)
            .ok_or_else(|| format!("{} has no debug adapter configured", pack.language.name))?;
        let build = self.plan(TaskKind::Build, startup_dir, config)?;
        let cwd = build.cwd.clone();
        let program = match lang {
            "rust" => rust_program(&cwd, config)?,
            _ => {
                // Other languages: the pack's run command when it is a path to a program.
                let run = pack.tasks.as_ref().and_then(|t| t.run.clone());
                match run.as_deref().and_then(|r| r.split_whitespace().next()) {
                    Some(p) if p.starts_with("./") => cwd.join(&p[2..]),
                    _ => {
                        return Err(format!(
                            "Debugging {} projects is not supported yet",
                            pack.language.name
                        ))
                    }
                }
            }
        };
        Ok(DebugPlan {
            build,
            program,
            cwd,
            adapter,
            adapter_args,
            install_hint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kalem-dbg-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("src")).unwrap();
        d
    }

    #[test]
    fn plans_a_rust_project() {
        let d = scratch("rust");
        fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(d.join("src/main.rs"), "fn main() {}\n").unwrap();
        let ws = Workspace::open(&d).unwrap();
        let p = ws.debug_plan(None, Config::Debug).unwrap();
        assert_eq!(p.build.command_line(), "cargo build");
        let name = p.program.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("demo-app"), "{name}");
        assert!(p.program.to_string_lossy().contains("debug"));
        let r = ws.debug_plan(None, Config::Release).unwrap();
        assert_eq!(r.build.command_line(), "cargo build --release");
        assert!(r.program.to_string_lossy().contains("release"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_library_has_nothing_to_debug() {
        let d = scratch("lib");
        fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"just-lib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(d.join("src/lib.rs"), "").unwrap();
        let ws = Workspace::open(&d).unwrap();
        let e = ws.debug_plan(None, Config::Debug).unwrap_err();
        assert!(e.contains("no binary"), "{e}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn path_lookup() {
        assert!(on_path("cargo"));
        assert!(!on_path("definitely-not-a-program-kalem"));
    }
}
