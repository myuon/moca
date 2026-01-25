pub mod ast;
mod codegen;
pub mod lexer;
mod module;
mod parser;
mod resolver;
pub mod typechecker;
pub mod types;

pub use codegen::Codegen;
pub use lexer::Lexer;
pub use module::ModuleLoader;
pub use parser::Parser;
pub use resolver::Resolver;
pub use typechecker::TypeChecker;

use crate::vm::VM;
use crate::{JitMode, RuntimeConfig};
use std::path::Path;

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
    typechecker.check_program(&program).map_err(|errors| {
        format_type_errors(filename, &errors)
    })?;

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

/// Compile and run a file with import support and runtime configuration.
pub fn run_file_with_config(path: &Path, config: &RuntimeConfig) -> Result<(), String> {
    let root_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut loader = ModuleLoader::new(root_dir);

    // Load main file with all imports
    let program = loader.load_with_imports(path)?;

    let filename = path.to_string_lossy().to_string();

    // Type checking
    let mut typechecker = TypeChecker::new(&filename);
    typechecker.check_program(&program).map_err(|errors| {
        format_type_errors(&filename, &errors)
    })?;

    // Name resolution
    let mut resolver = Resolver::new(&filename);
    let resolved = resolver.resolve(program)?;

    // Code generation
    let mut codegen = Codegen::new();
    let mut chunk = codegen.compile(resolved)?;

    // Log JIT settings if tracing is enabled
    if config.trace_jit {
        eprintln!("[JIT] Mode: {:?}, Threshold: {}", config.jit_mode, config.jit_threshold);
    }

    // Execution with runtime configuration
    let mut vm = VM::new();
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
        eprintln!("[GC] Collections: {}, Total pause: {}us, Max pause: {}us",
            stats.cycles, stats.total_pause_us, stats.max_pause_us);
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
    typechecker.check_program(&program).map_err(|errors| {
        format_type_errors(&filename, &errors)
    })?;

    Ok(())
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
