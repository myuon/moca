use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::process::ExitCode;

mod compiler;
mod debugger;
mod jit;
mod lsp;
mod package;
mod vm;

/// JIT compilation mode
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum JitMode {
    /// JIT disabled, interpreter only
    Off,
    /// JIT enabled (compile hot functions)
    On,
    /// Automatic: JIT enabled if supported on this platform
    #[default]
    Auto,
}

/// GC mode
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum GcMode {
    /// Stop-the-world GC
    #[default]
    Stw,
    /// Concurrent GC (reduced pause times)
    Concurrent,
}

/// Runtime configuration for the VM
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub jit_mode: JitMode,
    pub jit_threshold: u32,
    pub trace_jit: bool,
    pub gc_mode: GcMode,
    pub gc_stats: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            jit_mode: JitMode::Auto,
            jit_threshold: 1000,
            trace_jit: false,
            gc_mode: GcMode::Stw,
            gc_stats: false,
        }
    }
}

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

        /// JIT compilation mode (off, on, auto)
        #[arg(long, value_enum, default_value = "auto")]
        jit: JitMode,

        /// JIT compilation threshold (number of calls before JIT)
        #[arg(long, default_value = "1000")]
        jit_threshold: u32,

        /// Trace JIT compilation events
        #[arg(long)]
        trace_jit: bool,

        /// GC mode (stw, concurrent)
        #[arg(long, value_enum, default_value = "stw")]
        gc_mode: GcMode,

        /// Print GC statistics
        #[arg(long)]
        gc_stats: bool,
    },
    /// Start the language server
    Lsp,
    /// Debug a mica source file with TUI debugger
    Debug {
        /// The source file to debug
        file: PathBuf,
    },
    /// Type check a mica source file without running it
    Check {
        /// The source file to check (defaults to pkg.toml entry if in a project)
        file: Option<PathBuf>,
    },
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
        Commands::Run {
            file,
            jit,
            jit_threshold,
            trace_jit,
            gc_mode,
            gc_stats,
        } => {
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

            let config = RuntimeConfig {
                jit_mode: jit,
                jit_threshold,
                trace_jit,
                gc_mode,
                gc_stats,
            };

            if let Err(e) = run_file(&path, &config) {
                eprintln!("{}", e);
                return ExitCode::FAILURE;
            }
        }
        Commands::Lsp => {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(lsp::run_server());
        }
        Commands::Debug { file } => {
            if let Err(e) = debugger::run_debugger(&file) {
                eprintln!("{}", e);
                return ExitCode::FAILURE;
            }
        }
        Commands::Check { file } => {
            let path = match file {
                Some(p) => p,
                None => {
                    // Try to find pkg.toml and use entry point
                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    match package::PackageManifest::load(&cwd) {
                        Ok(manifest) => cwd.join(&manifest.package.entry),
                        Err(_) => {
                            eprintln!("error: no file specified and no pkg.toml found");
                            eprintln!("usage: mica check <file> or run from a mica project directory");
                            return ExitCode::FAILURE;
                        }
                    }
                }
            };

            if let Err(e) = compiler::check_file(&path) {
                eprintln!("{}", e);
                return ExitCode::FAILURE;
            }
            println!("Type check passed.");
        }
    }

    ExitCode::SUCCESS
}

fn run_file(path: &PathBuf, config: &RuntimeConfig) -> Result<(), String> {
    // Use the module-aware run_file for import support
    compiler::run_file_with_config(path, config)
}
