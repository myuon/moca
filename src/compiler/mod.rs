// Some functions and fields are defined for future use
#![allow(dead_code)]

pub mod ast;
mod codegen;
pub mod dump;
pub mod lexer;
mod module;
pub mod monomorphise;
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

use crate::compiler::ast::{Item, Program};
use crate::config::RuntimeConfig;
use std::collections::HashSet;

/// Parse and prepend stdlib to a user program.
/// The stdlib functions are added at the beginning so they are available globally.
/// If a user function has the same name as a stdlib function, the stdlib function is skipped.
fn prepend_stdlib(mut user_program: Program) -> Result<Program, String> {
    // Collect user-defined function names to avoid conflicts
    let user_fn_names: HashSet<String> = user_program
        .items
        .iter()
        .filter_map(|item| {
            if let Item::FnDef(fn_def) = item {
                Some(fn_def.name.clone())
            } else {
                None
            }
        })
        .collect();

    // Parse the stdlib prelude
    let mut lexer = Lexer::new("<stdlib>", STDLIB_PRELUDE);
    let tokens = lexer.scan_tokens()?;
    let mut parser = Parser::new("<stdlib>", tokens);
    let stdlib_program = parser.parse()?;

    // Filter out stdlib functions that conflict with user functions
    let filtered_stdlib_items: Vec<Item> = stdlib_program
        .items
        .into_iter()
        .filter(|item| {
            if let Item::FnDef(fn_def) = item {
                !user_fn_names.contains(&fn_def.name)
            } else {
                true
            }
        })
        .collect();

    // Prepend filtered stdlib items to user program
    let mut combined_items = filtered_stdlib_items;
    combined_items.append(&mut user_program.items);
    user_program.items = combined_items;

    Ok(user_program)
}
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
    let index_object_types = typechecker.index_object_types().clone();

    // Monomorphisation (specialize generic functions/structs)
    let program = monomorphise::monomorphise_program(program);

    // Name resolution
    let mut resolver = Resolver::new(filename);
    let resolved = resolver.resolve(program)?;

    // Code generation
    let mut codegen = Codegen::new();
    codegen.set_index_object_types(index_object_types);
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
    /// Standard error output
    pub stderr: String,
}

/// Compile and run a file, capturing output for testing.
///
/// Returns (CapturedOutput, Ok(())) on success, or (CapturedOutput, Err(msg)) on error.
/// This allows tests to check both the output and any error messages.
pub fn run_file_capturing_output(
    path: &Path,
    config: &RuntimeConfig,
) -> (CapturedOutput, Result<(), String>) {
    // Use Arc<Mutex<Cursor>> to allow shared ownership of the buffers
    let stdout_buffer = Arc::new(Mutex::new(Cursor::new(Vec::new())));
    let stderr_buffer = Arc::new(Mutex::new(Cursor::new(Vec::new())));
    let stdout_clone = Arc::clone(&stdout_buffer);
    let stderr_clone = Arc::clone(&stderr_buffer);

    let result = (|| {
        let root_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut loader = ModuleLoader::new(root_dir);

        // Load main file with all imports
        let user_program = loader.load_with_imports(path)?;

        // Prepend standard library
        let program = prepend_stdlib(user_program)?;

        let filename = path.to_string_lossy().to_string();

        // Type checking
        let mut typechecker = TypeChecker::new(&filename);
        typechecker
            .check_program(&program)
            .map_err(|errors| format_type_errors(&filename, &errors))?;
        let index_object_types = typechecker.index_object_types().clone();

        // Monomorphisation (specialize generic functions/structs)
        let program = monomorphise::monomorphise_program(program);

        // Name resolution
        let mut resolver = Resolver::new(&filename);
        let resolved = resolver.resolve(program)?;

        // Code generation
        let mut codegen = Codegen::new();
        codegen.set_index_object_types(index_object_types);
        let chunk = codegen.compile(resolved)?;

        // Execution with output capture using wrappers that write to shared buffers
        let mut vm = VM::new_with_config(
            config.heap_limit,
            config.gc_enabled,
            Box::new(SharedWriter(stdout_clone)),
            Box::new(SharedWriter(stderr_clone)),
        );
        vm.set_jit_config(config.jit_threshold, config.trace_jit);

        vm.run(&chunk)?;

        Ok(())
    })();

    // Extract the output from the buffers
    let output = {
        let stdout = stdout_buffer.lock().unwrap();
        let stderr = stderr_buffer.lock().unwrap();
        CapturedOutput {
            stdout: String::from_utf8_lossy(stdout.get_ref()).to_string(),
            stderr: String::from_utf8_lossy(stderr.get_ref()).to_string(),
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
    let user_program = loader.load_with_imports(path)?;

    // Prepend standard library
    let program = prepend_stdlib(user_program)?;

    let filename = path.to_string_lossy().to_string();

    // Type checking
    let mut typechecker = TypeChecker::new(&filename);
    typechecker
        .check_program(&program)
        .map_err(|errors| format_type_errors(&filename, &errors))?;
    let index_object_types = typechecker.index_object_types().clone();

    // Monomorphisation (specialize generic functions/structs)
    let program = monomorphise::monomorphise_program(program);

    // Name resolution
    let mut resolver = Resolver::new(&filename);
    let resolved = resolver.resolve(program)?;

    // Code generation
    let mut codegen = Codegen::new();
    codegen.set_index_object_types(index_object_types);
    let chunk = codegen.compile(resolved)?;

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

    vm.run(&chunk)?;

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
    cli_args: Vec<String>,
) -> Result<(), String> {
    let root_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut loader = ModuleLoader::new(root_dir);

    // Load main file with all imports
    let user_program = loader.load_with_imports(path)?;

    // Prepend standard library
    let program = prepend_stdlib(user_program)?;

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
    let index_object_types = typechecker.index_object_types().clone();

    // Monomorphisation (specialize generic functions/structs)
    let program = monomorphise::monomorphise_program(program);

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
    codegen.set_index_object_types(index_object_types);
    let chunk = codegen.compile(resolved)?;

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
    vm.set_cli_args(cli_args);

    vm.run(&chunk)?;

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
    let index_object_types = typechecker.index_object_types().clone();

    // Monomorphisation (specialize generic functions/structs)
    let program = monomorphise::monomorphise_program(program);

    // Name resolution
    let mut resolver = Resolver::new(&filename);
    let resolved = resolver.resolve(program)?;

    // Code generation
    let mut codegen = Codegen::new();
    codegen.set_index_object_types(index_object_types);
    let chunk = codegen.compile(resolved)?;

    Ok(dump::format_bytecode(&chunk))
}

// ============================================================================
// Test Runner API
// ============================================================================

/// Result of a single test execution.
#[derive(Debug, Clone)]
pub struct TestResult {
    /// Name of the test function (e.g., "_test_add")
    pub name: String,
    /// File path where the test is defined
    pub file: PathBuf,
    /// Whether the test passed
    pub passed: bool,
    /// Error message if the test failed
    pub error: Option<String>,
}

/// Results of running all tests.
#[derive(Debug, Clone, Default)]
pub struct TestResults {
    /// Individual test results
    pub results: Vec<TestResult>,
    /// Number of passed tests
    pub passed: usize,
    /// Number of failed tests
    pub failed: usize,
}

impl TestResults {
    /// Create a new empty TestResults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a test result.
    pub fn add(&mut self, result: TestResult) {
        if result.passed {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
        self.results.push(result);
    }

    /// Check if all tests passed.
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

/// Information about a discovered test function.
#[derive(Debug, Clone)]
pub struct TestInfo {
    /// Name of the test function
    pub name: String,
    /// File path where the test is defined
    pub file: PathBuf,
}

/// Discover all test functions in a directory.
///
/// Scans all .mc files recursively and finds functions with `_test_` prefix.
pub fn discover_tests(dir: &Path) -> Result<Vec<TestInfo>, String> {
    let mut tests = Vec::new();
    collect_test_files(dir, &mut tests)?;
    Ok(tests)
}

/// Recursively collect test functions from .mc files.
fn collect_test_files(dir: &Path, tests: &mut Vec<TestInfo>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }

    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("failed to read directory '{}': {}", dir.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read directory entry: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            collect_test_files(&path, tests)?;
        } else if path.extension().is_some_and(|ext| ext == "mc") {
            collect_tests_from_file(&path, tests)?;
        }
    }

    Ok(())
}

/// Extract test functions from a single .mc file.
fn collect_tests_from_file(path: &Path, tests: &mut Vec<TestInfo>) -> Result<(), String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read file '{}': {}", path.display(), e))?;

    let filename = path.to_string_lossy().to_string();

    // Parse the file
    let mut lexer = Lexer::new(&filename, &source);
    let tokens = match lexer.scan_tokens() {
        Ok(tokens) => tokens,
        Err(_) => return Ok(()), // Skip files with lexer errors
    };

    let mut parser = Parser::new(&filename, tokens);
    let program = match parser.parse() {
        Ok(program) => program,
        Err(_) => return Ok(()), // Skip files with parser errors
    };

    // Find functions with _test_ prefix
    for item in &program.items {
        if let Item::FnDef(fn_def) = item
            && fn_def.name.starts_with("_test_")
        {
            tests.push(TestInfo {
                name: fn_def.name.clone(),
                file: path.to_path_buf(),
            });
        }
    }

    Ok(())
}

/// Run all tests in a directory.
///
/// Returns TestResults with information about each test execution.
pub fn run_tests(dir: &Path, config: &RuntimeConfig) -> Result<TestResults, String> {
    let tests = discover_tests(dir)?;
    let mut results = TestResults::new();

    for test in tests {
        let result = run_single_test(&test, config);
        results.add(result);
    }

    Ok(results)
}

/// Run a single test function.
fn run_single_test(test: &TestInfo, config: &RuntimeConfig) -> TestResult {
    // Read the test file
    let source = match std::fs::read_to_string(&test.file) {
        Ok(s) => s,
        Err(e) => {
            return TestResult {
                name: test.name.clone(),
                file: test.file.clone(),
                passed: false,
                error: Some(format!("failed to read file: {}", e)),
            };
        }
    };

    // Append a call to the test function
    let source_with_call = format!("{}\n{}();", source, test.name);

    // Create a temporary file with the test call
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("moca_test_{}.mc", test.name));

    if let Err(e) = std::fs::write(&temp_file, &source_with_call) {
        return TestResult {
            name: test.name.clone(),
            file: test.file.clone(),
            passed: false,
            error: Some(format!("failed to write temp file: {}", e)),
        };
    }

    // Run the test
    let (_, result) = run_file_capturing_output(&temp_file, config);

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_file);

    match result {
        Ok(()) => TestResult {
            name: test.name.clone(),
            file: test.file.clone(),
            passed: true,
            error: None,
        },
        Err(e) => TestResult {
            name: test.name.clone(),
            file: test.file.clone(),
            passed: false,
            error: Some(e),
        },
    }
}

// ============================================================================
// Error Formatting
// ============================================================================

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
