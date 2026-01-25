use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

mod compiler;
mod lsp;
mod package;
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
    /// Initialize a new mica project
    Init {
        /// Project name (defaults to directory name)
        name: Option<String>,
    },
    /// Run a mica source file
    Run {
        /// The source file to run (defaults to pkg.toml entry if in a project)
        file: Option<PathBuf>,
    },
    /// Start the language server
    Lsp,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { name } => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            if let Err(e) = package::init_project(&cwd, name.as_deref()) {
                eprintln!("{}", e);
                return ExitCode::FAILURE;
            }
        }
        Commands::Run { file } => {
            let path = match file {
                Some(p) => p,
                None => {
                    // Try to find pkg.toml and use entry point
                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    match package::PackageManifest::load(&cwd) {
                        Ok(manifest) => cwd.join(&manifest.package.entry),
                        Err(_) => {
                            eprintln!("error: no file specified and no pkg.toml found");
                            eprintln!("usage: mica run <file> or run from a mica project directory");
                            return ExitCode::FAILURE;
                        }
                    }
                }
            };
            if let Err(e) = run_file(&path) {
                eprintln!("{}", e);
                return ExitCode::FAILURE;
            }
        }
        Commands::Lsp => {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(lsp::run_server());
        }
    }

    ExitCode::SUCCESS
}

fn run_file(path: &PathBuf) -> Result<(), String> {
    // Use the module-aware run_file for import support
    compiler::run_file(path)
}
