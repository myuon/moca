use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

mod compiler;
mod vm;

#[derive(Parser)]
#[command(name = "mica")]
#[command(about = "A minimal programming language", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a mica source file
    Run {
        /// The source file to run
        file: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { file } => {
            if let Err(e) = run_file(&file) {
                eprintln!("{}", e);
                return ExitCode::FAILURE;
            }
        }
    }

    ExitCode::SUCCESS
}

fn run_file(path: &PathBuf) -> Result<(), String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("error: could not read file '{}': {}", path.display(), e))?;

    let filename = path.to_string_lossy().to_string();
    compiler::run(&filename, &source)
}
