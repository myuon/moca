mod lexer;
mod ast;
mod parser;
mod resolver;
mod codegen;

pub use lexer::Lexer;
pub use parser::Parser;
pub use resolver::Resolver;
pub use codegen::Codegen;

use crate::vm::VM;

/// Compile and run the given source code.
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
