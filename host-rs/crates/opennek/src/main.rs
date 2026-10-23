use std::process::ExitCode;

// Placeholder until the host is ported (#36); clap comes with the first real options
fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => ExitCode::SUCCESS,
        Some("--version") => {
            println!("opennek {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(arg) => {
            eprintln!("opennek: unknown argument {arg}");
            ExitCode::from(2)
        }
    }
}
