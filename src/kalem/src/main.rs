use kalem::{build_registry, default_languages_dir};
use std::process::ExitCode;

fn main() -> ExitCode {
    let reg = build_registry(default_languages_dir());
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
