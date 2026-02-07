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

use config::{GcMode, JitMode, RuntimeConfig, TimingsFormat};

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

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum TimingsFormatArg {
    #[default]
    Human,
    Json,
}

impl From<TimingsFormatArg> for TimingsFormat {
    fn from(arg: TimingsFormatArg) -> Self {
        match arg {
            TimingsFormatArg::Human => TimingsFormat::Human,
            TimingsFormatArg::Json => TimingsFormat::Json,
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

        /// Arguments to pass to the script
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        script_args: Vec<String>,

        /// Execute code directly from command line
        #[arg(short = 'c', long)]
        code: Option<String>,

        /// Execution timeout in seconds (0 = no timeout)
        #[arg(long, default_value = "0")]
        timeout: u64,

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

        /// Profile opcode execution counts
        #[arg(long)]
        profile_opcodes: bool,

        /// Print compiler pipeline timings (human or json format)
        #[arg(long, value_enum, require_equals = true, num_args = 0..=1, default_missing_value = "human")]
        timings: Option<TimingsFormatArg>,
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
    /// Lint a moca source file
    Lint {
        /// The source file to lint (defaults to pkg.toml entry if in a project)
        file: Option<PathBuf>,
    },
    /// Run tests in the project
    Test {
        /// Directory to search for tests (defaults to src/ or pkg.toml entry directory)
        dir: Option<PathBuf>,
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
            script_args,
            code,
            timeout,
            jit,
            jit_threshold,
            trace_jit,
            gc_mode,
            gc_stats,
            dump_ast,
            dump_resolved,
            dump_bytecode,
            profile_opcodes,
            timings,
        } => {
            let config = RuntimeConfig {
                jit_mode: jit.into(),
                jit_threshold,
                trace_jit,
                gc_mode: gc_mode.into(),
                gc_stats,
                profile_opcodes,
                ..Default::default()
            };

            let dump_opts = compiler::DumpOptions {
                dump_ast,
                dump_resolved,
                dump_bytecode,
            };

            let timings_format: Option<TimingsFormat> = timings.map(|t| t.into());

            // Run with timeout if specified
            let run_result = if timeout > 0 {
                use std::sync::mpsc;
                use std::thread;
                use std::time::Duration;

                let (tx, rx) = mpsc::channel();
                let code_clone = code.clone();
                let config_clone = config.clone();
                let dump_opts_clone = dump_opts.clone();
                let script_args_clone = script_args.clone();
                let file_clone = file.clone();

                let handle = thread::spawn(move || {
                    let result = if let Some(source) = code_clone {
                        let mut cli_args = vec!["<code>".to_string()];
                        cli_args.extend(script_args_clone);
                        compiler::run_source(
                            &source,
                            &config_clone,
                            &dump_opts_clone,
                            cli_args,
                            timings_format,
                        )
                    } else {
                        let path = match file_clone {
                            Some(p) => p,
                            None => {
                                let cwd =
                                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                                match package::PackageManifest::load(&cwd) {
                                    Ok(manifest) => cwd.join(&manifest.package.entry),
                                    Err(_) => {
                                        return Err(
                                            "no file specified and no pkg.toml found".to_string()
                                        );
                                    }
                                }
                            }
                        };
                        let mut cli_args = vec![path.to_string_lossy().to_string()];
                        cli_args.extend(script_args_clone);
                        run_file(
                            &path,
                            &config_clone,
                            &dump_opts_clone,
                            cli_args,
                            timings_format,
                        )
                    };
                    let _ = tx.send(());
                    result
                });

                match rx.recv_timeout(Duration::from_secs(timeout)) {
                    Ok(()) => handle
                        .join()
                        .unwrap_or_else(|_| Err("thread panicked".to_string())),
                    Err(_) => {
                        eprintln!("error: execution timed out after {} seconds", timeout);
                        return ExitCode::FAILURE;
                    }
                }
            } else if let Some(source) = code {
                // Direct code execution without timeout
                let mut cli_args = vec!["<code>".to_string()];
                cli_args.extend(script_args);
                compiler::run_source(&source, &config, &dump_opts, cli_args, timings_format)
            } else {
                // File execution without timeout
                let path = match file {
                    Some(p) => p,
                    None => {
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
                let mut cli_args = vec![path.to_string_lossy().to_string()];
                cli_args.extend(script_args);
                run_file(&path, &config, &dump_opts, cli_args, timings_format)
            };

            if let Err(e) = run_result {
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
        Commands::Lint { file } => {
            let path = match file {
                Some(p) => p,
                None => {
                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    match package::PackageManifest::load(&cwd) {
                        Ok(manifest) => cwd.join(&manifest.package.entry),
                        Err(_) => {
                            eprintln!("error: no file specified and no pkg.toml found");
                            eprintln!(
                                "usage: moca lint <file> or run from a moca project directory"
                            );
                            return ExitCode::FAILURE;
                        }
                    }
                }
            };

            match compiler::lint_file(&path) {
                Ok((output, count)) => {
                    if count > 0 {
                        print!("{}", output);
                        return ExitCode::FAILURE;
                    }
                }
                Err(e) => {
                    eprintln!("{}", e);
                    return ExitCode::FAILURE;
                }
            }
        }
        Commands::Test { dir } => {
            let test_dir = match dir {
                Some(d) => d,
                None => {
                    // Try to find pkg.toml and use entry directory
                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    match package::PackageManifest::load(&cwd) {
                        Ok(manifest) => {
                            // Get directory of entry point
                            let entry_path = Path::new(&manifest.package.entry);
                            cwd.join(entry_path.parent().unwrap_or(Path::new("src")))
                        }
                        Err(_) => cwd.join("src"),
                    }
                }
            };

            let config = RuntimeConfig::default();

            match compiler::run_tests(&test_dir, &config) {
                Ok(results) => {
                    // Print individual test results
                    for result in &results.results {
                        if result.passed {
                            println!("\u{2713} {} passed", result.name);
                        } else {
                            let error_msg = result.error.as_deref().unwrap_or("unknown error");
                            println!("\u{2717} {} failed: {}", result.name, error_msg);
                        }
                    }

                    // Print summary
                    println!();
                    println!("{} passed, {} failed", results.passed, results.failed);

                    if !results.all_passed() {
                        return ExitCode::FAILURE;
                    }
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    return ExitCode::FAILURE;
                }
            }
        }
    }

    ExitCode::SUCCESS
}

fn run_file(
    path: &Path,
    config: &RuntimeConfig,
    dump_opts: &compiler::DumpOptions,
    cli_args: Vec<String>,
    timings_format: Option<TimingsFormat>,
) -> Result<(), String> {
    // Use the module-aware run_file with dump support
    compiler::run_file_with_dump(path, config, dump_opts, cli_args, timings_format)
}
