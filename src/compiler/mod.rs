pub mod ast;
mod codegen;
pub mod lexer;
mod module;
mod parser;
mod resolver;

pub use codegen::Codegen;
pub use lexer::Lexer;
pub use module::ModuleLoader;
pub use parser::Parser;
pub use resolver::Resolver;

use crate::vm::VM;
use std::path::Path;

/// Compile and run the given source code (no import support).
pub fn run(filename: &str, source: &str) -> Result<(), String> {
    // Lexing
    let mut lexer = Lexer::new(filename, source);
    let tokens = lexer.scan_tokens()?;

    // Parsing
    let mut parser = Parser::new(filename, tokens);
    let program = parser.parse()?;

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
    let root_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut loader = ModuleLoader::new(root_dir);

    // Load main file with all imports
    let program = loader.load_with_imports(path)?;

    let filename = path.to_string_lossy().to_string();

    // Name resolution
    let mut resolver = Resolver::new(&filename);
    let resolved = resolver.resolve(program)?;

    // Code generation
    let mut codegen = Codegen::new();
    let chunk = codegen.compile(resolved)?;

    // Execution
    let mut vm = VM::new();
    vm.run(&chunk)?;

    Ok(())
}
