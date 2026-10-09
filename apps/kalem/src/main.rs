use kalem_core::{Command, CommandRegistry};
use kalem_lang::{load_dir, Os};
use std::path::PathBuf;
use std::process::ExitCode;

fn languages_dir() -> PathBuf {
    std::env::var_os("KALEM_LANGUAGES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("languages"))
}

fn build_registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();
    reg.register(Command::new("app.version", "Show version", |_| {
        Ok(format!("Kalem IDE {}", env!("CARGO_PKG_VERSION")))
    }));
    reg.register(Command::new(
        "languages.list",
        "List language packs",
        |_| {
            let packs = load_dir(&languages_dir()).map_err(kalem_core::CommandError::Failed)?;
            let os = Os::current();
            let lines: Vec<String> = packs
                .iter()
                .map(|p| {
                    let dbg = p.debug.as_ref().map(|d| d.adapter_for(os)).unwrap_or("-");
                    let lsp = p.lsp.as_ref().map(|l| l.command.as_str()).unwrap_or("-");
                    format!("{:<12} lsp: {:<28} debug: {}", p.language.id, lsp, dbg)
                })
                .collect();
            Ok(lines.join("\n"))
        },
    ));
    reg
}

fn main() -> ExitCode {
    let reg = build_registry();
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None | Some("help") | Some("--help") => {
            println!("kalem <command> [args]\n\nCommands:");
            for c in reg.list() {
                println!("  {:<18} {}", c.id, c.title);
            }
            ExitCode::SUCCESS
        }
        Some(id) => match reg.execute(id, &args.collect::<Vec<_>>()) {
            Ok(out) => {
                println!("{out}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
    }
}
