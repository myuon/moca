// Some functions and fields are defined for future use
#![allow(dead_code)]

pub mod ast;
mod codegen;
pub mod dump;
pub mod lexer;
mod module;
mod parser;
pub mod resolver;
pub mod typechecker;
pub mod types;

pub use codegen::Codegen;
pub use lexer::Lexer;
pub use module::ModuleLoader;
pub use parser::Parser;
pub use resolver::Resolver;
pub use typechecker::TypeChecker;

/// Standard library prelude, embedded at compile time.
pub const STDLIB_PRELUDE: &str = include_str!("../../std/prelude.mc");

use crate::config::{JitMode, RuntimeConfig};
use crate::vm::VM;
use std::fs::File;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Options for dumping intermediate representations.
///
/// Each option is `Option<Option<PathBuf>>`:
/// - `None`: dump is disabled
/// - `Some(None)`: dump to stderr
/// - `Some(Some(path))`: dump to file
#[derive(Debug, Clone, Default)]
pub struct DumpOptions {
    /// Dump AST to stderr (Some(None)) or to a file (Some(Some(path)))
    pub dump_ast: Option<Option<PathBuf>>,
    /// Dump resolved program to stderr (Some(None)) or to a file (Some(Some(path)))
    pub dump_resolved: Option<Option<PathBuf>>,
    /// Dump bytecode to stderr (Some(None)) or to a file (Some(Some(path)))
    pub dump_bytecode: Option<Option<PathBuf>>,
}

impl DumpOptions {
    /// Check if any dump option is enabled.
    pub fn any_enabled(&self) -> bool {
        self.dump_ast.is_some() || self.dump_resolved.is_some() || self.dump_bytecode.is_some()
    }
}

/// Compile and run the given source code (no import support).
pub fn run(filename: &str, source: &str) -> Result<(), String> {
    // Lexing
    let mut lexer = Lexer::new(filename, source);
    let tokens = lexer.scan_tokens()?;

    // Parsing
    let mut parser = Parser::new(filename, tokens);
    let program = parser.parse()?;

    // Type checking
    let mut typechecker = TypeChecker::new(filename);
    typechecker
        .check_program(&program)
        .map_err(|errors| format_type_errors(filename, &errors))?;

    // Name resolution
    let mut resolver = Resolver::new(filename);
    let resolved = resolver.resolve(program)?;

    // Code generation
    let mut codegen = Codegen::new();
    let chunk = codegen.compile(resolved)?;

    // Execution
    let mut vm = VM::new();
    vm.run(&chunk)?;

    Ok(())
}

/// Compile and run a file with import support.
pub fn run_file(path: &Path) -> Result<(), String> {
    run_file_with_config(path, &RuntimeConfig::default())
}

/// Output captured from running a file.
#[derive(Debug, Clone, Default)]
pub struct CapturedOutput {
    /// Standard output from print statements
    pub stdout: String,
}

/// Compile and run a file, capturing output for testing.
///
/// Returns (stdout_content, Ok(())) on success, or (stdout_content, Err(msg)) on error.
/// This allows tests to check both the output and any error messages.
pub fn run_file_capturing_output(
    path: &Path,
    config: &RuntimeConfig,
) -> (CapturedOutput, Result<(), String>) {
    // Use Arc<Mutex<Cursor>> to allow shared ownership of the buffer
    let stdout_buffer = Arc::new(Mutex::new(Cursor::new(Vec::new())));
    let buffer_clone = Arc::clone(&stdout_buffer);

    let result = (|| {
        let root_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut loader = ModuleLoader::new(root_dir);

        // Load main file with all imports
        let program = loader.load_with_imports(path)?;

        let filename = path.to_string_lossy().to_string();

        // Type checking
        let mut typechecker = TypeChecker::new(&filename);
        typechecker
            .check_program(&program)
            .map_err(|errors| format_type_errors(&filename, &errors))?;

        // Name resolution
        let mut resolver = Resolver::new(&filename);
        let resolved = resolver.resolve(program)?;

        // Code generation
        let mut codegen = Codegen::new();
        let mut chunk = codegen.compile(resolved)?;

        // Execution with output capture using a wrapper that writes to the shared buffer
        let mut vm = VM::new_with_config(
            config.heap_limit,
            config.gc_enabled,
            Box::new(SharedWriter(buffer_clone)),
        );
        vm.set_jit_config(config.jit_threshold, config.trace_jit);

        match config.jit_mode {
            JitMode::Off => {
                vm.run(&chunk)?;
            }
            JitMode::On | JitMode::Auto => {
                vm.run_with_quickening(&mut chunk)?;
            }
        }

        Ok(())
    })();

    // Extract the output from the buffer
    let output = {
        let buffer = stdout_buffer.lock().unwrap();
        CapturedOutput {
            stdout: String::from_utf8_lossy(buffer.get_ref()).to_string(),
        }
    };

    (output, result)
}

/// A Write wrapper that writes to a shared buffer.
struct SharedWriter(Arc<Mutex<Cursor<Vec<u8>>>>);

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

/// Compile and run a file with import support and runtime configuration.
pub fn run_file_with_config(path: &Path, config: &RuntimeConfig) -> Result<(), String> {
    let root_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut loader = ModuleLoader::new(root_dir);

    // Load main file with all imports
    let program = loader.load_with_imports(path)?;

    let filename = path.to_string_lossy().to_string();

    // Type checking
    let mut typechecker = TypeChecker::new(&filename);
    typechecker
        .check_program(&program)
        .map_err(|errors| format_type_errors(&filename, &errors))?;

    // Name resolution
    let mut resolver = Resolver::new(&filename);
    let resolved = resolver.resolve(program)?;

    // Code generation
    let mut codegen = Codegen::new();
    let mut chunk = codegen.compile(resolved)?;

    // Log JIT settings if tracing is enabled
    if config.trace_jit {
        eprintln!(
            "[JIT] Mode: {:?}, Threshold: {}",
            config.jit_mode, config.jit_threshold
        );
    }

    // Execution with runtime configuration
    let mut vm = VM::new_with_heap_config(config.heap_limit, config.gc_enabled);
    vm.set_jit_config(config.jit_threshold, config.trace_jit);

    // Use quickening mode for better performance
    match config.jit_mode {
        JitMode::Off => {
            vm.run(&chunk)?;
        }
        JitMode::On | JitMode::Auto => {
            // Run with quickening and JIT compilation
            vm.run_with_quickening(&mut chunk)?;
        }
    }

    // Print GC stats if requested
    if config.gc_stats {
        let stats = vm.gc_stats();
        eprintln!(
            "[GC] Collections: {}, Total pause: {}us, Max pause: {}us",
            stats.cycles, stats.total_pause_us, stats.max_pause_us
        );
    }

    Ok(())
}

/// Compile and run a file with dump options.
pub fn run_file_with_dump(
    path: &Path,
    config: &RuntimeConfig,
    dump_opts: &DumpOptions,
) -> Result<(), String> {
    let root_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut loader = ModuleLoader::new(root_dir);

    // Load main file with all imports
    let program = loader.load_with_imports(path)?;

    let filename = path.to_string_lossy().to_string();

    // Dump AST if requested (before type checking, so we can dump even if type check fails)
    if let Some(ref output_path) = dump_opts.dump_ast {
        let ast_str = dump::format_ast(&program);
        write_dump(&ast_str, output_path.as_ref(), "AST")?;
    }

    // Type checking
    let mut typechecker = TypeChecker::new(&filename);
    typechecker
        .check_program(&program)
        .map_err(|errors| format_type_errors(&filename, &errors))?;

    // Name resolution
    let mut resolver = Resolver::new(&filename);
    let resolved = resolver.resolve(program)?;

    // Dump resolved program if requested
    if let Some(ref output_path) = dump_opts.dump_resolved {
        let resolved_str = dump::format_resolved(&resolved);
        write_dump(&resolved_str, output_path.as_ref(), "Resolved")?;
    }

    // Code generation
    let mut codegen = Codegen::new();
    let mut chunk = codegen.compile(resolved)?;

    // Dump bytecode if requested
    if let Some(ref output_path) = dump_opts.dump_bytecode {
        let bytecode_str = dump::format_bytecode(&chunk);
        write_dump(&bytecode_str, output_path.as_ref(), "Bytecode")?;
    }

    // Log JIT settings if tracing is enabled
    if config.trace_jit {
        eprintln!(
            "[JIT] Mode: {:?}, Threshold: {}",
            config.jit_mode, config.jit_threshold
        );
    }

    // Execution with runtime configuration
    let mut vm = VM::new_with_heap_config(config.heap_limit, config.gc_enabled);
    vm.set_jit_config(config.jit_threshold, config.trace_jit);

    // Use quickening mode for better performance
    match config.jit_mode {
        JitMode::Off => {
            vm.run(&chunk)?;
        }
        JitMode::On | JitMode::Auto => {
            // Run with quickening and JIT compilation
            vm.run_with_quickening(&mut chunk)?;
        }
    }

    // Print GC stats if requested
    if config.gc_stats {
        let stats = vm.gc_stats();
        eprintln!(
            "[GC] Collections: {}, Total pause: {}us, Max pause: {}us",
            stats.cycles, stats.total_pause_us, stats.max_pause_us
        );
    }

    Ok(())
}

/// Write dump output to stderr or a file.
/// - `None`: dump to stderr
/// - `Some(path)`: dump to file
fn write_dump(content: &str, output_path: Option<&PathBuf>, label: &str) -> Result<(), String> {
    match output_path {
        Some(path) => {
            let mut file = File::create(path)
                .map_err(|e| format!("failed to create dump file '{}': {}", path.display(), e))?;
            file.write_all(content.as_bytes())
                .map_err(|e| format!("failed to write dump file '{}': {}", path.display(), e))?;
            eprintln!("[{}] Dumped to: {}", label, path.display());
        }
        None => {
            eprintln!("== {} ==", label);
            eprintln!("{}", content);
        }
    }
    Ok(())
}

/// Type check a file without running it.
pub fn check_file(path: &Path) -> Result<(), String> {
    let root_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut loader = ModuleLoader::new(root_dir);

    // Load main file with all imports
    let program = loader.load_with_imports(path)?;

    let filename = path.to_string_lossy().to_string();

    // Type checking only
    let mut typechecker = TypeChecker::new(&filename);
    typechecker
        .check_program(&program)
        .map_err(|errors| format_type_errors(&filename, &errors))?;

    Ok(())
}

/// Compile a file and return the AST dump as a string.
pub fn dump_ast(path: &Path) -> Result<String, String> {
    let root_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut loader = ModuleLoader::new(root_dir);

    // Load main file with all imports
    let program = loader.load_with_imports(path)?;

    Ok(dump::format_ast(&program))
}

/// Compile a file and return the bytecode dump as a string.
pub fn dump_bytecode(path: &Path) -> Result<String, String> {
    let root_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut loader = ModuleLoader::new(root_dir);

    // Load main file with all imports
    let program = loader.load_with_imports(path)?;

    let filename = path.to_string_lossy().to_string();

    // Type checking
    let mut typechecker = TypeChecker::new(&filename);
    typechecker
        .check_program(&program)
        .map_err(|errors| format_type_errors(&filename, &errors))?;

    // Name resolution
    let mut resolver = Resolver::new(&filename);
    let resolved = resolver.resolve(program)?;

    // Code generation
    let mut codegen = Codegen::new();
    let chunk = codegen.compile(resolved)?;

    Ok(dump::format_bytecode(&chunk))
}

/// Format type errors for display.
fn format_type_errors(filename: &str, errors: &[typechecker::TypeError]) -> String {
    let mut output = String::new();

    for error in errors {
        output.push_str(&format!(
            "error: type error: {}\n  --> {}:{}:{}\n",
            error.message, filename, error.span.line, error.span.column
        ));

        if let (Some(expected), Some(found)) = (&error.expected, &error.found) {
            output.push_str(&format!(
                "   = expected `{}`, found `{}`\n",
                expected, found
            ));
        }
    }

    output
}
