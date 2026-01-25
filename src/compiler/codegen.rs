use crate::compiler::ast::{BinaryOp, UnaryOp};
use crate::compiler::resolver::{ResolvedExpr, ResolvedFunction, ResolvedProgram, ResolvedStatement};
use crate::vm::{Chunk, Function, Op};

/// Code generator that compiles resolved AST to bytecode.
pub struct Codegen {
    functions: Vec<Function>,
}

impl Codegen {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
        }
    }

    pub fn compile(&mut self, program: ResolvedProgram) -> Result<Chunk, String> {
        // Compile all user-defined functions first
        for func in program.functions {
            let compiled = self.compile_function(func)?;
            self.functions.push(compiled);
        }

        // Compile main body
        let mut main_ops = Vec::new();
        for stmt in program.main_body {
            self.compile_statement(&stmt, &mut main_ops)?;
        }
        // End of main
        main_ops.push(Op::PushInt(0)); // Return value for main
        main_ops.push(Op::Ret);

        let main_func = Function {
            name: "__main__".to_string(),
            arity: 0,
            locals_count: 0, // TODO: track main locals
            code: main_ops,
        };

        Ok(Chunk {
            functions: self.functions.clone(),
            main: main_func,
        })
    }

    fn compile_function(&mut self, func: ResolvedFunction) -> Result<Function, String> {
        let mut ops = Vec::new();

        for stmt in &func.body {
            self.compile_statement(stmt, &mut ops)?;
        }

        // Implicit return nil (as 0 for v0)
        if !matches!(ops.last(), Some(Op::Ret)) {
            ops.push(Op::PushInt(0));
            ops.push(Op::Ret);
        }

        Ok(Function {
            name: func.name,
            arity: func.params.len(),
            locals_count: func.locals_count,
            code: ops,
        })
    }

    fn compile_statement(&self, stmt: &ResolvedStatement, ops: &mut Vec<Op>) -> Result<(), String> {
        match stmt {
            ResolvedStatement::Let { slot, init } => {
                self.compile_expr(init, ops)?;
                ops.push(Op::StoreLocal(*slot));
            }
            ResolvedStatement::Assign { slot, value } => {
                self.compile_expr(value, ops)?;
                ops.push(Op::StoreLocal(*slot));
            }
            ResolvedStatement::If {
                condition,
                then_block,
                else_block,
            } => {
                self.compile_expr(condition, ops)?;

                // Jump to else if false
                let jump_to_else = ops.len();
                ops.push(Op::JmpIfFalse(0)); // Placeholder

                // Then block
                for stmt in then_block {
                    self.compile_statement(stmt, ops)?;
                }

                if let Some(else_stmts) = else_block {
                    // Jump over else block
                    let jump_over_else = ops.len();
                    ops.push(Op::Jmp(0)); // Placeholder

                    // Patch jump to else
                    let else_start = ops.len();
                    ops[jump_to_else] = Op::JmpIfFalse(else_start);

                    // Else block
                    for stmt in else_stmts {
                        self.compile_statement(stmt, ops)?;
                    }

                    // Patch jump over else
                    let after_else = ops.len();
                    ops[jump_over_else] = Op::Jmp(after_else);
                } else {
                    // Patch jump to after then
                    let after_then = ops.len();
                    ops[jump_to_else] = Op::JmpIfFalse(after_then);
                }
            }
            ResolvedStatement::While { condition, body } => {
                let loop_start = ops.len();

                self.compile_expr(condition, ops)?;

                let jump_to_end = ops.len();
                ops.push(Op::JmpIfFalse(0)); // Placeholder

                for stmt in body {
                    self.compile_statement(stmt, ops)?;
                }

                ops.push(Op::Jmp(loop_start));

                let loop_end = ops.len();
                ops[jump_to_end] = Op::JmpIfFalse(loop_end);
            }
            ResolvedStatement::Return { value } => {
                if let Some(value) = value {
                    self.compile_expr(value, ops)?;
                } else {
                    ops.push(Op::PushInt(0)); // Return 0 for void
                }
                ops.push(Op::Ret);
            }
            ResolvedStatement::Expr { expr } => {
                self.compile_expr(expr, ops)?;
                ops.push(Op::Pop); // Discard result
            }
        }

        Ok(())
    }

    fn compile_expr(&self, expr: &ResolvedExpr, ops: &mut Vec<Op>) -> Result<(), String> {
        match expr {
            ResolvedExpr::Int(value) => {
                ops.push(Op::PushInt(*value));
            }
            ResolvedExpr::Bool(value) => {
                if *value {
                    ops.push(Op::PushTrue);
                } else {
                    ops.push(Op::PushFalse);
                }
            }
            ResolvedExpr::Local(slot) => {
                ops.push(Op::LoadLocal(*slot));
            }
            ResolvedExpr::Unary { op, operand } => {
                self.compile_expr(operand, ops)?;
                match op {
                    UnaryOp::Neg => ops.push(Op::Neg),
                    UnaryOp::Not => ops.push(Op::Not),
                }
            }
            ResolvedExpr::Binary { op, left, right } => {
                // Handle short-circuit evaluation for && and ||
                match op {
                    BinaryOp::And => {
                        self.compile_expr(left, ops)?;
                        let jump_if_false = ops.len();
                        ops.push(Op::JmpIfFalse(0)); // Placeholder
                        ops.push(Op::Pop); // Pop the true value
                        self.compile_expr(right, ops)?;
                        let end = ops.len();
                        ops[jump_if_false] = Op::JmpIfFalse(end);
                        // If left was false, it's still on stack
                        // If left was true, right's value is on stack
                        return Ok(());
                    }
                    BinaryOp::Or => {
                        self.compile_expr(left, ops)?;
                        let jump_if_true = ops.len();
                        ops.push(Op::JmpIfTrue(0)); // Placeholder
                        ops.push(Op::Pop); // Pop the false value
                        self.compile_expr(right, ops)?;
                        let end = ops.len();
                        ops[jump_if_true] = Op::JmpIfTrue(end);
                        return Ok(());
                    }
                    _ => {}
                }

                self.compile_expr(left, ops)?;
                self.compile_expr(right, ops)?;

                match op {
                    BinaryOp::Add => ops.push(Op::Add),
                    BinaryOp::Sub => ops.push(Op::Sub),
                    BinaryOp::Mul => ops.push(Op::Mul),
                    BinaryOp::Div => ops.push(Op::Div),
                    BinaryOp::Mod => ops.push(Op::Mod),
                    BinaryOp::Eq => ops.push(Op::Eq),
                    BinaryOp::Ne => ops.push(Op::Ne),
                    BinaryOp::Lt => ops.push(Op::Lt),
                    BinaryOp::Le => ops.push(Op::Le),
                    BinaryOp::Gt => ops.push(Op::Gt),
                    BinaryOp::Ge => ops.push(Op::Ge),
                    BinaryOp::And | BinaryOp::Or => unreachable!(),
                }
            }
            ResolvedExpr::Call { func_index, args } => {
                // Push arguments
                for arg in args {
                    self.compile_expr(arg, ops)?;
                }
                ops.push(Op::Call(*func_index, args.len()));
            }
            ResolvedExpr::Builtin { name, args } => {
                match name.as_str() {
                    "print" => {
                        if args.len() != 1 {
                            return Err("print takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::Print);
                    }
                    _ => return Err(format!("unknown builtin '{}'", name)),
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lexer::Lexer;
    use crate::compiler::parser::Parser;
    use crate::compiler::resolver::Resolver;

    fn compile(source: &str) -> Result<Chunk, String> {
        let mut lexer = Lexer::new("test.mica", source);
        let tokens = lexer.scan_tokens()?;
        let mut parser = Parser::new("test.mica", tokens);
        let program = parser.parse()?;
        let mut resolver = Resolver::new("test.mica");
        let resolved = resolver.resolve(program)?;
        let mut codegen = Codegen::new();
        codegen.compile(resolved)
    }

    #[test]
    fn test_simple_print() {
        let chunk = compile("print(42);").unwrap();
        assert!(chunk.main.code.contains(&Op::PushInt(42)));
        assert!(chunk.main.code.contains(&Op::Print));
    }

    #[test]
    fn test_arithmetic() {
        let chunk = compile("print(1 + 2);").unwrap();
        assert!(chunk.main.code.contains(&Op::Add));
    }

    #[test]
    fn test_function_call() {
        let chunk = compile("fn foo() { return 42; } print(foo());").unwrap();
        assert_eq!(chunk.functions.len(), 1);
        assert_eq!(chunk.functions[0].name, "foo");
    }
}
