use crate::compiler::ast::{BinaryOp, UnaryOp};
use crate::compiler::resolver::{
    ResolvedExpr, ResolvedFunction, ResolvedProgram, ResolvedStatement, ResolvedStruct,
};
use crate::vm::{Chunk, DebugInfo, Function, FunctionDebugInfo, Op};
use std::collections::HashMap;

/// Code generator that compiles resolved AST to bytecode.
pub struct Codegen {
    functions: Vec<Function>,
    strings: Vec<String>,
    debug: DebugInfo,
    emit_debug: bool,
    /// Struct definitions for field access resolution
    structs: Vec<ResolvedStruct>,
    /// Map struct name -> (struct_index, field_name -> field_index)
    struct_field_indices: HashMap<String, HashMap<String, usize>>,
}

impl Codegen {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            strings: Vec::new(),
            debug: DebugInfo::new(),
            emit_debug: true, // Enable debug info by default
            structs: Vec::new(),
            struct_field_indices: HashMap::new(),
        }
    }

    /// Create a codegen without debug info (for release builds).
    pub fn without_debug() -> Self {
        Self {
            functions: Vec::new(),
            strings: Vec::new(),
            debug: DebugInfo::new(),
            emit_debug: false,
            structs: Vec::new(),
            struct_field_indices: HashMap::new(),
        }
    }

    /// Initialize struct field indices from resolved program.
    fn init_structs(&mut self, structs: Vec<ResolvedStruct>) {
        for s in &structs {
            let mut field_map = HashMap::new();
            for (idx, field_name) in s.fields.iter().enumerate() {
                field_map.insert(field_name.clone(), idx);
            }
            self.struct_field_indices.insert(s.name.clone(), field_map);
        }
        self.structs = structs;
    }

    /// Look up a field index for any known struct.
    fn get_field_index(&self, field_name: &str) -> Option<usize> {
        // Check all structs for this field name
        for field_map in self.struct_field_indices.values() {
            if let Some(&idx) = field_map.get(field_name) {
                return Some(idx);
            }
        }
        None
    }

    /// Add a string to the constants pool and return its index.
    fn add_string(&mut self, s: String) -> usize {
        // Check if string already exists
        if let Some(idx) = self.strings.iter().position(|x| x == &s) {
            idx
        } else {
            let idx = self.strings.len();
            self.strings.push(s);
            idx
        }
    }

    pub fn compile(&mut self, program: ResolvedProgram) -> Result<Chunk, String> {
        // Initialize struct field indices for field access resolution
        self.init_structs(program.structs);

        // Compile all user-defined functions first
        for func in &program.functions {
            let compiled = self.compile_function(func)?;
            self.functions.push(compiled);
            // Add debug info placeholder for each function
            if self.emit_debug {
                self.debug.functions.push(FunctionDebugInfo::new());
            }
        }

        // Compile main body
        let mut main_ops = Vec::new();
        for stmt in program.main_body {
            self.compile_statement(&stmt, &mut main_ops)?;
        }
        // End of main
        main_ops.push(Op::PushNull); // Return value for main
        main_ops.push(Op::Ret);

        let main_func = Function {
            name: "__main__".to_string(),
            arity: 0,
            locals_count: 0, // TODO: track main locals
            code: main_ops,
            stackmap: None, // TODO: generate StackMap
        };

        let debug = if self.emit_debug {
            Some(self.debug.clone())
        } else {
            None
        };

        Ok(Chunk {
            functions: self.functions.clone(),
            main: main_func,
            strings: self.strings.clone(),
            debug,
        })
    }

    fn compile_function(&mut self, func: &ResolvedFunction) -> Result<Function, String> {
        let mut ops = Vec::new();

        for stmt in &func.body {
            self.compile_statement(stmt, &mut ops)?;
        }

        // Implicit return nil
        if !matches!(ops.last(), Some(Op::Ret)) {
            ops.push(Op::PushNull);
            ops.push(Op::Ret);
        }

        Ok(Function {
            name: func.name.clone(),
            arity: func.params.len(),
            locals_count: func.locals_count,
            code: ops,
            stackmap: None, // TODO: generate StackMap
        })
    }

    fn compile_statement(
        &mut self,
        stmt: &ResolvedStatement,
        ops: &mut Vec<Op>,
    ) -> Result<(), String> {
        match stmt {
            ResolvedStatement::Let { slot, init } => {
                self.compile_expr(init, ops)?;
                ops.push(Op::SetL(*slot));
            }
            ResolvedStatement::Assign { slot, value } => {
                self.compile_expr(value, ops)?;
                ops.push(Op::SetL(*slot));
            }
            ResolvedStatement::IndexAssign {
                object,
                index,
                value,
            } => {
                self.compile_expr(object, ops)?;
                self.compile_expr(index, ops)?;
                self.compile_expr(value, ops)?;
                ops.push(Op::ArraySet);
            }
            ResolvedStatement::FieldAssign {
                object,
                field,
                value,
            } => {
                // Check if this might be a struct field (structs are compiled as arrays)
                if let Some(idx) = self.get_field_index(field) {
                    // Known struct field - use array index assignment
                    self.compile_expr(object, ops)?;
                    ops.push(Op::PushInt(idx as i64));
                    self.compile_expr(value, ops)?;
                    ops.push(Op::ArraySet);
                } else {
                    // Regular object field assignment
                    self.compile_expr(object, ops)?;
                    self.compile_expr(value, ops)?;
                    let field_idx = self.add_string(field.clone());
                    ops.push(Op::SetF(field_idx));
                }
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
            ResolvedStatement::ForIn {
                slot,
                iterable,
                body,
            } => {
                // For-in loop: for x in arr { body }
                // Desugars to:
                //   let __arr = arr;     (slot = slot, reuse)
                //   let __idx = 0;       (slot + 1)
                //   while __idx < len(__arr) {
                //     x = __arr[__idx];
                //     body
                //     __idx = __idx + 1;
                //   }
                //
                // We use slot for x, slot+1 for __idx, slot+2 for __arr

                let var_slot = *slot;
                let idx_slot = slot + 1;
                let arr_slot = slot + 2;

                // Store array
                self.compile_expr(iterable, ops)?;
                ops.push(Op::SetL(arr_slot));

                // Initialize index to 0
                ops.push(Op::PushInt(0));
                ops.push(Op::SetL(idx_slot));

                let loop_start = ops.len();

                // Check: idx < arr.len()
                ops.push(Op::GetL(idx_slot));
                ops.push(Op::GetL(arr_slot));
                ops.push(Op::ArrayLen);
                ops.push(Op::Lt);

                let jump_to_end = ops.len();
                ops.push(Op::JmpIfFalse(0)); // Placeholder

                // x = arr[idx]
                ops.push(Op::GetL(arr_slot));
                ops.push(Op::GetL(idx_slot));
                ops.push(Op::ArrayGet);
                ops.push(Op::SetL(var_slot));

                // Body
                for stmt in body {
                    self.compile_statement(stmt, ops)?;
                }

                // idx = idx + 1
                ops.push(Op::GetL(idx_slot));
                ops.push(Op::PushInt(1));
                ops.push(Op::Add);
                ops.push(Op::SetL(idx_slot));

                // Jump back to loop start
                ops.push(Op::Jmp(loop_start));

                // End of loop
                let loop_end = ops.len();
                ops[jump_to_end] = Op::JmpIfFalse(loop_end);
            }
            ResolvedStatement::Return { value } => {
                if let Some(value) = value {
                    self.compile_expr(value, ops)?;
                } else {
                    ops.push(Op::PushNull); // Return nil for void
                }
                ops.push(Op::Ret);
            }
            ResolvedStatement::Throw { value } => {
                self.compile_expr(value, ops)?;
                ops.push(Op::Throw);
            }
            ResolvedStatement::Try {
                try_block,
                catch_slot,
                catch_block,
            } => {
                // TryBegin with placeholder for catch handler address
                let try_begin_idx = ops.len();
                ops.push(Op::TryBegin(0)); // Placeholder

                // Compile try block
                for stmt in try_block {
                    self.compile_statement(stmt, ops)?;
                }

                // End of try block - remove handler and jump over catch
                ops.push(Op::TryEnd);
                let jump_over_catch = ops.len();
                ops.push(Op::Jmp(0)); // Placeholder

                // Catch handler starts here
                let catch_start = ops.len();
                ops[try_begin_idx] = Op::TryBegin(catch_start);

                // Exception value is on stack, store to catch variable slot
                ops.push(Op::SetL(*catch_slot));

                // Compile catch block
                for stmt in catch_block {
                    self.compile_statement(stmt, ops)?;
                }

                // Patch jump over catch
                let after_catch = ops.len();
                ops[jump_over_catch] = Op::Jmp(after_catch);
            }
            ResolvedStatement::Expr { expr } => {
                self.compile_expr(expr, ops)?;
                ops.push(Op::Pop); // Discard result
            }
        }

        Ok(())
    }

    fn compile_expr(&mut self, expr: &ResolvedExpr, ops: &mut Vec<Op>) -> Result<(), String> {
        match expr {
            ResolvedExpr::Int(value) => {
                ops.push(Op::PushInt(*value));
            }
            ResolvedExpr::Float(value) => {
                ops.push(Op::PushFloat(*value));
            }
            ResolvedExpr::Bool(value) => {
                if *value {
                    ops.push(Op::PushTrue);
                } else {
                    ops.push(Op::PushFalse);
                }
            }
            ResolvedExpr::Str(value) => {
                let idx = self.add_string(value.clone());
                ops.push(Op::PushString(idx));
            }
            ResolvedExpr::Nil => {
                ops.push(Op::PushNull);
            }
            ResolvedExpr::Local(slot) => {
                ops.push(Op::GetL(*slot));
            }
            ResolvedExpr::Array { elements } => {
                // Push all elements, then allocate array
                for elem in elements {
                    self.compile_expr(elem, ops)?;
                }
                ops.push(Op::AllocArray(elements.len()));
            }
            ResolvedExpr::Object { fields } => {
                // Push field names and values as pairs
                for (name, value) in fields {
                    let name_idx = self.add_string(name.clone());
                    ops.push(Op::PushString(name_idx));
                    self.compile_expr(value, ops)?;
                }
                ops.push(Op::New(fields.len()));
            }
            ResolvedExpr::Index { object, index } => {
                self.compile_expr(object, ops)?;
                self.compile_expr(index, ops)?;
                ops.push(Op::ArrayGet);
            }
            ResolvedExpr::Field { object, field } => {
                self.compile_expr(object, ops)?;
                // Check if this might be a struct field (structs are compiled as arrays)
                if let Some(idx) = self.get_field_index(field) {
                    // Known struct field - use array index access
                    ops.push(Op::PushInt(idx as i64));
                    ops.push(Op::ArrayGet);
                } else {
                    // Regular object field access
                    let field_idx = self.add_string(field.clone());
                    ops.push(Op::GetF(field_idx));
                }
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
                    "len" => {
                        if args.len() != 1 {
                            return Err("len takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        // len works on both arrays and strings
                        // VM will handle type dispatch
                        ops.push(Op::ArrayLen); // This also works for strings via VM dispatch
                    }
                    "push" => {
                        if args.len() != 2 {
                            return Err("push takes exactly 2 arguments".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        self.compile_expr(&args[1], ops)?;
                        ops.push(Op::ArrayPush);
                        // push returns nil
                        ops.push(Op::PushNull);
                    }
                    "pop" => {
                        if args.len() != 1 {
                            return Err("pop takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::ArrayPop);
                    }
                    "type_of" => {
                        if args.len() != 1 {
                            return Err("type_of takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::TypeOf);
                    }
                    "to_string" => {
                        if args.len() != 1 {
                            return Err("to_string takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::ToString);
                    }
                    "parse_int" => {
                        if args.len() != 1 {
                            return Err("parse_int takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::ParseInt);
                    }
                    // Thread builtins
                    "spawn" => {
                        // spawn is handled specially in resolver as SpawnFunc
                        return Err("spawn should be resolved to SpawnFunc".to_string());
                    }
                    "channel" => {
                        if !args.is_empty() {
                            return Err("channel takes no arguments".to_string());
                        }
                        ops.push(Op::ChannelCreate);
                    }
                    "send" => {
                        if args.len() != 2 {
                            return Err(
                                "send takes exactly 2 arguments (channel_id, value)".to_string()
                            );
                        }
                        self.compile_expr(&args[0], ops)?;
                        self.compile_expr(&args[1], ops)?;
                        ops.push(Op::ChannelSend);
                        // send returns nil
                        ops.push(Op::PushNull);
                    }
                    "recv" => {
                        if args.len() != 1 {
                            return Err("recv takes exactly 1 argument (channel_id)".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::ChannelRecv);
                    }
                    "join" => {
                        if args.len() != 1 {
                            return Err("join takes exactly 1 argument (handle)".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::ThreadJoin);
                    }
                    _ => return Err(format!("unknown builtin '{}'", name)),
                }
            }
            ResolvedExpr::SpawnFunc { func_index } => {
                ops.push(Op::ThreadSpawn(*func_index));
            }
            ResolvedExpr::StructLiteral {
                struct_index: _,
                fields,
            } => {
                // Compile struct as an array (tuple) with field values in declaration order
                for value in fields {
                    self.compile_expr(value, ops)?;
                }
                ops.push(Op::AllocArray(fields.len()));
            }
            ResolvedExpr::MethodCall {
                object,
                method: _,
                args,
            } => {
                // TODO: Implement proper method dispatch
                // For now, compile as a function call with the method name
                // Push object (self) first, then args
                self.compile_expr(object, ops)?;
                for arg in args {
                    self.compile_expr(arg, ops)?;
                }

                // For now, pop arguments and return nil as we don't have method dispatch
                ops.push(Op::Pop);
                for _ in 0..args.len() {
                    ops.push(Op::Pop);
                }
                ops.push(Op::PushNull);
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
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens()?;
        let mut parser = Parser::new("test.mc", tokens);
        let program = parser.parse()?;
        let mut resolver = Resolver::new("test.mc");
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
        let chunk = compile("fun foo() { return 42; } print(foo());").unwrap();
        assert_eq!(chunk.functions.len(), 1);
        assert_eq!(chunk.functions[0].name, "foo");
    }
}
