use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod compiler;
mod config;
mod debugger;
mod ffi;
#[cfg(feature = "jit")]
mod jit;
mod lsp;
mod package;
mod vm;

use config::{GcMode, JitMode, RuntimeConfig};

// Wrapper types for clap ValueEnum support
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum JitModeArg {
    Off,
    On,
    #[default]
    Auto,
}

impl From<JitModeArg> for JitMode {
    fn from(arg: JitModeArg) -> Self {
        match arg {
            JitModeArg::Off => JitMode::Off,
            JitModeArg::On => JitMode::On,
            JitModeArg::Auto => JitMode::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum GcModeArg {
    #[default]
    Stw,
    Concurrent,
}

impl From<GcModeArg> for GcMode {
    fn from(arg: GcModeArg) -> Self {
        match arg {
            GcModeArg::Stw => GcMode::Stw,
            GcModeArg::Concurrent => GcMode::Concurrent,
        }
    }
}

#[derive(Parser)]
#[command(name = "moca")]
#[command(about = "A minimal programming language", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new moca project
    Init {
        /// Project name (defaults to directory name)
        name: Option<String>,
    },
    /// Run a moca source file
    Run {
        /// The source file to run (defaults to pkg.toml entry if in a project)
        file: Option<PathBuf>,

        /// JIT compilation mode (off, on, auto)
        #[arg(long, value_enum, default_value = "auto")]
        jit: JitModeArg,

        /// JIT compilation threshold (number of calls before JIT)
        #[arg(long, default_value = "1000")]
        jit_threshold: u32,

        /// Trace JIT compilation events
        #[arg(long)]
        trace_jit: bool,

        /// GC mode (stw, concurrent)
        #[arg(long, value_enum, default_value = "stw")]
        gc_mode: GcModeArg,

        /// Print GC statistics
        #[arg(long)]
        gc_stats: bool,

        /// Dump AST to stderr, or to a file with --dump-ast=path
        #[arg(long, value_name = "FILE", num_args = 0..=1)]
        dump_ast: Option<Option<PathBuf>>,

        /// Dump resolved program to stderr, or to a file with --dump-resolved=path
        #[arg(long, value_name = "FILE", num_args = 0..=1)]
        dump_resolved: Option<Option<PathBuf>>,

        /// Dump bytecode to stderr, or to a file with --dump-bytecode=path
        #[arg(long, value_name = "FILE", num_args = 0..=1)]
        dump_bytecode: Option<Option<PathBuf>>,
    },
    /// Start the language server
    Lsp,
    /// Debug a moca source file with TUI debugger
    Debug {
        /// The source file to debug
        file: PathBuf,
    },
    /// Type check a moca source file without running it
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
            dump_ast,
            dump_resolved,
            dump_bytecode,
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
                            eprintln!(
                                "usage: moca run <file> or run from a moca project directory"
                            );
                            return ExitCode::FAILURE;
                        }
                    }
                }
            };

            let config = RuntimeConfig {
                jit_mode: jit.into(),
                jit_threshold,
                trace_jit,
                gc_mode: gc_mode.into(),
                gc_stats,
            };

            let dump_opts = compiler::DumpOptions {
                dump_ast,
                dump_resolved,
                dump_bytecode,
            };

            if let Err(e) = run_file(&path, &config, &dump_opts) {
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
                            eprintln!(
                                "usage: moca check <file> or run from a moca project directory"
                            );
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

fn run_file(
    path: &Path,
    config: &RuntimeConfig,
    dump_opts: &compiler::DumpOptions,
) -> Result<(), String> {
    // Use the module-aware run_file with dump support
    compiler::run_file_with_dump(path, config, dump_opts)
}
