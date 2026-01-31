use crate::compiler::ast::{AsmArg, BinaryOp, UnaryOp};
use crate::compiler::lexer::Span;
use crate::compiler::resolver::{
    ResolvedAsmInstruction, ResolvedExpr, ResolvedFunction, ResolvedProgram, ResolvedStatement,
    ResolvedStruct,
};
use crate::compiler::types::Type;
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
    /// Index expression object types (from typechecker)
    index_object_types: HashMap<Span, Type>,
    /// Map function name -> function index (for calling stdlib functions)
    function_indices: HashMap<String, usize>,
}

impl Default for Codegen {
    fn default() -> Self {
        Self::new()
    }
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
            index_object_types: HashMap::new(),
            function_indices: HashMap::new(),
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
            index_object_types: HashMap::new(),
            function_indices: HashMap::new(),
        }
    }

    /// Set index object types from typechecker.
    pub fn set_index_object_types(&mut self, types: HashMap<Span, Type>) {
        self.index_object_types = types;
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

        // Build function name -> index map for stdlib function calls
        for (idx, func) in program.functions.iter().enumerate() {
            self.function_indices.insert(func.name.clone(), idx);
        }

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
                span,
            } => {
                // Check if the object is a Vector (from type info)
                let is_vector = self
                    .index_object_types
                    .get(span)
                    .map(|t| matches!(t, Type::Vector(_)))
                    .unwrap_or(false);

                if is_vector {
                    // Vector assign: call vec_set_any from stdlib
                    if let Some(&func_idx) = self.function_indices.get("vec_set_any") {
                        self.compile_expr(object, ops)?;
                        self.compile_expr(index, ops)?;
                        self.compile_expr(value, ops)?;
                        ops.push(Op::Call(func_idx, 3));
                        ops.push(Op::Pop); // Discard nil return value
                    } else {
                        return Err("vec_set_any not found in stdlib".to_string());
                    }
                } else {
                    // Array/struct assign: direct HeapStoreDyn
                    self.compile_expr(object, ops)?;
                    self.compile_expr(index, ops)?;
                    self.compile_expr(value, ops)?;
                    ops.push(Op::HeapStoreDyn);
                }
            }
            ResolvedStatement::FieldAssign {
                object,
                field,
                value,
            } => {
                // Check if this might be a struct field (structs are compiled as arrays)
                if let Some(idx) = self.get_field_index(field) {
                    // Known struct field - use heap slot assignment
                    self.compile_expr(object, ops)?;
                    ops.push(Op::PushInt(idx as i64));
                    self.compile_expr(value, ops)?;
                    ops.push(Op::HeapStoreDyn);
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
                ops.push(Op::HeapLoadDyn);
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
                // Array layout: [elem0, elem1, ...] (length is slots.len())
                for elem in elements {
                    self.compile_expr(elem, ops)?;
                }
                ops.push(Op::AllocHeap(elements.len()));
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
            ResolvedExpr::Index {
                object,
                index,
                span,
            } => {
                // Check if the object is a Vector (from type info)
                let is_vector = self
                    .index_object_types
                    .get(span)
                    .map(|t| matches!(t, Type::Vector(_)))
                    .unwrap_or(false);

                if is_vector {
                    // Vector access: call vec_get_any from stdlib
                    if let Some(&func_idx) = self.function_indices.get("vec_get_any") {
                        self.compile_expr(object, ops)?;
                        self.compile_expr(index, ops)?;
                        ops.push(Op::Call(func_idx, 2));
                    } else {
                        return Err("vec_get_any not found in stdlib".to_string());
                    }
                } else {
                    // Array/struct access: direct HeapLoadDyn
                    self.compile_expr(object, ops)?;
                    self.compile_expr(index, ops)?;
                    ops.push(Op::HeapLoadDyn);
                }
            }
            ResolvedExpr::Field { object, field } => {
                self.compile_expr(object, ops)?;
                // Check if this might be a struct field (structs are compiled as arrays)
                if let Some(idx) = self.get_field_index(field) {
                    // Known struct field - use heap slot access
                    ops.push(Op::PushInt(idx as i64));
                    ops.push(Op::HeapLoadDyn);
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
                // JmpIfFalse/JmpIfTrue pop the condition value, so we need to Dup first
                // to keep the result on stack when short-circuiting
                match op {
                    BinaryOp::And => {
                        // For &&: if left is false, skip right and keep false on stack
                        self.compile_expr(left, ops)?;
                        ops.push(Op::Dup); // Duplicate for the jump check
                        let jump_if_false = ops.len();
                        ops.push(Op::JmpIfFalse(0)); // Placeholder, consumes the dup'd value
                        ops.push(Op::Pop); // Pop the original true value
                        self.compile_expr(right, ops)?;
                        let end = ops.len();
                        ops[jump_if_false] = Op::JmpIfFalse(end);
                        // If left was false: jump taken, original false still on stack
                        // If left was true: pop it, right's value is on stack
                        return Ok(());
                    }
                    BinaryOp::Or => {
                        // For ||: if left is true, skip right and keep true on stack
                        self.compile_expr(left, ops)?;
                        ops.push(Op::Dup); // Duplicate for the jump check
                        let jump_if_true = ops.len();
                        ops.push(Op::JmpIfTrue(0)); // Placeholder, consumes the dup'd value
                        ops.push(Op::Pop); // Pop the original false value
                        self.compile_expr(right, ops)?;
                        let end = ops.len();
                        ops[jump_if_true] = Op::JmpIfTrue(end);
                        // If left was true: jump taken, original true still on stack
                        // If left was false: pop it, right's value is on stack
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
                    "print" | "print_debug" => {
                        if args.len() != 1 {
                            return Err("print/print_debug takes exactly 1 argument".to_string());
                        }
                        // If argument is a string literal, call print_str from stdlib
                        if matches!(&args[0], ResolvedExpr::Str(_)) {
                            if let Some(&func_idx) = self.function_indices.get("print_str") {
                                // Call print_str(s)
                                self.compile_expr(&args[0], ops)?;
                                ops.push(Op::Call(func_idx, 1));
                                ops.push(Op::Pop); // discard return value
                                // Call print_str("\n")
                                let newline_idx = self.add_string("\n".to_string());
                                ops.push(Op::PushString(newline_idx));
                                ops.push(Op::Call(func_idx, 1));
                                // print_str returns nil, which is what print should return
                            } else {
                                // Fallback if print_str not available
                                self.compile_expr(&args[0], ops)?;
                                ops.push(Op::PrintDebug);
                            }
                        } else {
                            self.compile_expr(&args[0], ops)?;
                            ops.push(Op::PrintDebug);
                        }
                    }
                    "__syscall" => {
                        // __syscall(num, ...args) -> result
                        // First argument must be a compile-time constant (syscall number)
                        if args.is_empty() {
                            return Err("__syscall requires at least 1 argument (syscall number)"
                                .to_string());
                        }
                        // Extract syscall number from first argument (must be integer literal)
                        let syscall_num = match &args[0] {
                            ResolvedExpr::Int(n) => *n as usize,
                            _ => {
                                return Err("__syscall first argument must be an integer literal"
                                    .to_string());
                            }
                        };
                        // Compile remaining arguments (syscall-specific args)
                        for arg in args.iter().skip(1) {
                            self.compile_expr(arg, ops)?;
                        }
                        // argc is the number of syscall-specific arguments (excluding syscall number)
                        let argc = args.len() - 1;
                        ops.push(Op::Syscall(syscall_num, argc));
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
                        // Call vec_push_any from stdlib
                        if let Some(&func_idx) = self.function_indices.get("vec_push_any") {
                            self.compile_expr(&args[0], ops)?;
                            self.compile_expr(&args[1], ops)?;
                            ops.push(Op::Call(func_idx, 2));
                            // vec_push_any returns nil implicitly
                        } else {
                            return Err("vec_push_any not found in stdlib".to_string());
                        }
                    }
                    "pop" => {
                        if args.len() != 1 {
                            return Err("pop takes exactly 1 argument".to_string());
                        }
                        // Call vec_pop_any from stdlib
                        if let Some(&func_idx) = self.function_indices.get("vec_pop_any") {
                            self.compile_expr(&args[0], ops)?;
                            ops.push(Op::Call(func_idx, 1));
                        } else {
                            return Err("vec_pop_any not found in stdlib".to_string());
                        }
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
                    // Vector builtins - delegate to stdlib functions
                    "vec_new" => {
                        if !args.is_empty() {
                            return Err("vec_new takes no arguments".to_string());
                        }
                        // Call vec_new_any from stdlib
                        if let Some(&func_idx) = self.function_indices.get("vec_new_any") {
                            ops.push(Op::Call(func_idx, 0));
                        } else {
                            return Err("vec_new_any not found in stdlib".to_string());
                        }
                    }
                    "vec_with_capacity" => {
                        if args.len() != 1 {
                            return Err(
                                "vec_with_capacity takes exactly 1 argument (capacity)".to_string()
                            );
                        }
                        // Call vec_with_capacity_any from stdlib
                        if let Some(&func_idx) = self.function_indices.get("vec_with_capacity_any")
                        {
                            self.compile_expr(&args[0], ops)?;
                            ops.push(Op::Call(func_idx, 1));
                        } else {
                            return Err("vec_with_capacity_any not found in stdlib".to_string());
                        }
                    }
                    "vec_push" => {
                        if args.len() != 2 {
                            return Err(
                                "vec_push takes exactly 2 arguments (vector, value)".to_string()
                            );
                        }
                        // Call vec_push_any from stdlib
                        if let Some(&func_idx) = self.function_indices.get("vec_push_any") {
                            self.compile_expr(&args[0], ops)?;
                            self.compile_expr(&args[1], ops)?;
                            ops.push(Op::Call(func_idx, 2));
                            // vec_push_any returns nil implicitly, no need to push
                        } else {
                            return Err("vec_push_any not found in stdlib".to_string());
                        }
                    }
                    "vec_pop" => {
                        if args.len() != 1 {
                            return Err("vec_pop takes exactly 1 argument (vector)".to_string());
                        }
                        // Call vec_pop_any from stdlib
                        if let Some(&func_idx) = self.function_indices.get("vec_pop_any") {
                            self.compile_expr(&args[0], ops)?;
                            ops.push(Op::Call(func_idx, 1));
                        } else {
                            return Err("vec_pop_any not found in stdlib".to_string());
                        }
                    }
                    "vec_len" => {
                        if args.len() != 1 {
                            return Err("vec_len takes exactly 1 argument (vector)".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::HeapLoad(1)); // slot 1 is length [ptr, len, cap]
                    }
                    "vec_capacity" => {
                        if args.len() != 1 {
                            return Err(
                                "vec_capacity takes exactly 1 argument (vector)".to_string()
                            );
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::HeapLoad(2)); // slot 2 is capacity [ptr, len, cap]
                    }
                    "vec_get" => {
                        if args.len() != 2 {
                            return Err(
                                "vec_get takes exactly 2 arguments (vector, index)".to_string()
                            );
                        }
                        // Call vec_get_any from stdlib
                        if let Some(&func_idx) = self.function_indices.get("vec_get_any") {
                            self.compile_expr(&args[0], ops)?;
                            self.compile_expr(&args[1], ops)?;
                            ops.push(Op::Call(func_idx, 2));
                        } else {
                            return Err("vec_get_any not found in stdlib".to_string());
                        }
                    }
                    "vec_set" => {
                        if args.len() != 3 {
                            return Err("vec_set takes exactly 3 arguments (vector, index, value)"
                                .to_string());
                        }
                        // Call vec_set_any from stdlib
                        if let Some(&func_idx) = self.function_indices.get("vec_set_any") {
                            self.compile_expr(&args[0], ops)?;
                            self.compile_expr(&args[1], ops)?;
                            self.compile_expr(&args[2], ops)?;
                            ops.push(Op::Call(func_idx, 3));
                            // vec_set_any returns nil implicitly
                        } else {
                            return Err("vec_set_any not found in stdlib".to_string());
                        }
                    }
                    // Low-level heap intrinsics (for stdlib implementation)
                    "__heap_load" => {
                        // __heap_load(ref, idx) -> value at ref[idx]
                        if args.len() != 2 {
                            return Err(
                                "__heap_load takes exactly 2 arguments (ref, index)".to_string()
                            );
                        }
                        self.compile_expr(&args[0], ops)?;
                        self.compile_expr(&args[1], ops)?;
                        ops.push(Op::HeapLoadDyn);
                    }
                    "__heap_store" => {
                        // __heap_store(ref, idx, val) -> nil, stores val at ref[idx]
                        if args.len() != 3 {
                            return Err(
                                "__heap_store takes exactly 3 arguments (ref, index, value)"
                                    .to_string(),
                            );
                        }
                        self.compile_expr(&args[0], ops)?;
                        self.compile_expr(&args[1], ops)?;
                        self.compile_expr(&args[2], ops)?;
                        ops.push(Op::HeapStoreDyn);
                        ops.push(Op::PushNull); // returns nil
                    }
                    "__alloc_heap" => {
                        // __alloc_heap(size) -> ref to newly allocated heap object with size slots
                        if args.len() != 1 {
                            return Err("__alloc_heap takes exactly 1 argument (size)".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::AllocHeapDynSimple);
                    }
                    // CLI argument builtins
                    "argc" => {
                        if !args.is_empty() {
                            return Err("argc takes no arguments".to_string());
                        }
                        ops.push(Op::Argc);
                    }
                    "argv" => {
                        if args.len() != 1 {
                            return Err("argv takes exactly 1 argument (index)".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::Argv);
                    }
                    "args" => {
                        if !args.is_empty() {
                            return Err("args takes no arguments".to_string());
                        }
                        ops.push(Op::Args);
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
                // Compile struct as slots with [field0, field1, ...] layout
                for value in fields {
                    self.compile_expr(value, ops)?;
                }
                ops.push(Op::AllocHeap(fields.len()));
            }
            ResolvedExpr::MethodCall {
                object,
                method: _,
                func_index,
                args,
                return_struct_name: _,
            } => {
                // Push object (self) as first argument, then other args
                self.compile_expr(object, ops)?;
                for arg in args {
                    self.compile_expr(arg, ops)?;
                }

                // Call the resolved method function (self + args)
                ops.push(Op::Call(*func_index, args.len() + 1));
            }
            ResolvedExpr::AsmBlock {
                input_slots,
                output_type: _,
                body,
            } => {
                // Push input variables onto the stack (left to right)
                for slot in input_slots {
                    ops.push(Op::GetL(*slot));
                }

                // Compile each asm instruction
                for inst in body {
                    self.compile_asm_instruction(inst, ops)?;
                }

                // If no output type, the result is whatever is on the stack
                // The caller is responsible for handling the stack state
            }
        }

        Ok(())
    }

    /// Compile a single asm instruction.
    fn compile_asm_instruction(
        &mut self,
        inst: &ResolvedAsmInstruction,
        ops: &mut Vec<Op>,
    ) -> Result<(), String> {
        match inst {
            ResolvedAsmInstruction::Emit { op_name, args } => {
                let op = self.parse_asm_op(op_name, args)?;
                ops.push(op);
            }
            ResolvedAsmInstruction::Safepoint => {
                // Safepoint: allow GC to run here
                // We emit a GcHint(0) to mark this as a safepoint
                ops.push(Op::GcHint(0));
            }
            ResolvedAsmInstruction::GcHint { size } => {
                ops.push(Op::GcHint(*size as usize));
            }
        }
        Ok(())
    }

    /// Parse an asm op name and arguments into a VM Op.
    fn parse_asm_op(&mut self, op_name: &str, args: &[AsmArg]) -> Result<Op, String> {
        match op_name {
            // Constants & Stack
            "PushInt" => {
                let value = self.expect_int_arg(args, 0, "PushInt")?;
                Ok(Op::PushInt(value))
            }
            "PushFloat" => {
                let value = self.expect_float_arg(args, 0, "PushFloat")?;
                Ok(Op::PushFloat(value))
            }
            "PushTrue" => Ok(Op::PushTrue),
            "PushFalse" => Ok(Op::PushFalse),
            "PushNull" => Ok(Op::PushNull),
            "PushString" => {
                let value = self.expect_string_arg(args, 0, "PushString")?;
                let idx = self.add_string(value);
                Ok(Op::PushString(idx))
            }
            "Pop" => Ok(Op::Pop),
            "Dup" => Ok(Op::Dup),
            "Swap" => Ok(Op::Swap),
            "Pick" => {
                let n = self.expect_int_arg(args, 0, "Pick")? as usize;
                Ok(Op::Pick(n))
            }
            "PickDyn" => Ok(Op::PickDyn),

            // Local Variables
            "GetL" => {
                let slot = self.expect_int_arg(args, 0, "GetL")? as usize;
                Ok(Op::GetL(slot))
            }
            "SetL" => {
                let slot = self.expect_int_arg(args, 0, "SetL")? as usize;
                Ok(Op::SetL(slot))
            }

            // Arithmetic
            "Add" => Ok(Op::Add),
            "Sub" => Ok(Op::Sub),
            "Mul" => Ok(Op::Mul),
            "Div" => Ok(Op::Div),
            "Mod" => Ok(Op::Mod),
            "Neg" => Ok(Op::Neg),

            // Comparison
            "Eq" => Ok(Op::Eq),
            "Ne" => Ok(Op::Ne),
            "Lt" => Ok(Op::Lt),
            "Le" => Ok(Op::Le),
            "Gt" => Ok(Op::Gt),
            "Ge" => Ok(Op::Ge),

            // Logical
            "Not" => Ok(Op::Not),

            // Control Flow - Jmp instructions (allowed within asm block)
            "Jmp" => {
                let target = self.expect_int_arg(args, 0, "Jmp")? as usize;
                Ok(Op::Jmp(target))
            }
            "JmpIfFalse" => {
                let target = self.expect_int_arg(args, 0, "JmpIfFalse")? as usize;
                Ok(Op::JmpIfFalse(target))
            }
            "JmpIfTrue" => {
                let target = self.expect_int_arg(args, 0, "JmpIfTrue")? as usize;
                Ok(Op::JmpIfTrue(target))
            }

            // Functions - FORBIDDEN
            "Call" => Err("Call instruction is forbidden in asm block".to_string()),
            "Ret" => Err("Ret instruction is forbidden in asm block".to_string()),

            // Heap & Objects
            "New" => {
                let n = self.expect_int_arg(args, 0, "New")? as usize;
                Ok(Op::New(n))
            }
            "GetF" => {
                let field = self.expect_string_arg(args, 0, "GetF")?;
                let idx = self.add_string(field);
                Ok(Op::GetF(idx))
            }
            "SetF" => {
                let field = self.expect_string_arg(args, 0, "SetF")?;
                let idx = self.add_string(field);
                Ok(Op::SetF(idx))
            }

            // Heap slot operations
            "AllocHeap" => {
                let n = self.expect_int_arg(args, 0, "AllocHeap")? as usize;
                Ok(Op::AllocHeap(n))
            }
            "AllocHeapDyn" => Ok(Op::AllocHeapDyn),
            "HeapLoad" => {
                let n = self.expect_int_arg(args, 0, "HeapLoad")? as usize;
                Ok(Op::HeapLoad(n))
            }
            "HeapStore" => {
                let n = self.expect_int_arg(args, 0, "HeapStore")? as usize;
                Ok(Op::HeapStore(n))
            }
            "HeapLoadDyn" => Ok(Op::HeapLoadDyn),
            "HeapStoreDyn" => Ok(Op::HeapStoreDyn),

            // Array operations
            "ArrayLen" => Ok(Op::ArrayLen),

            // Type operations
            "TypeOf" => Ok(Op::TypeOf),
            "ToString" => Ok(Op::ToString),
            "ParseInt" => Ok(Op::ParseInt),

            // Exception handling
            "Throw" => Ok(Op::Throw),
            "TryBegin" => {
                let target = self.expect_int_arg(args, 0, "TryBegin")? as usize;
                Ok(Op::TryBegin(target))
            }
            "TryEnd" => Ok(Op::TryEnd),

            // Builtins
            "PrintDebug" => Ok(Op::PrintDebug),

            // GC hint
            "GcHint" => {
                let size = self.expect_int_arg(args, 0, "GcHint")? as usize;
                Ok(Op::GcHint(size))
            }

            // Thread operations
            "ThreadSpawn" => {
                let func_index = self.expect_int_arg(args, 0, "ThreadSpawn")? as usize;
                Ok(Op::ThreadSpawn(func_index))
            }
            "ChannelCreate" => Ok(Op::ChannelCreate),
            "ChannelSend" => Ok(Op::ChannelSend),
            "ChannelRecv" => Ok(Op::ChannelRecv),
            "ThreadJoin" => Ok(Op::ThreadJoin),

            _ => Err(format!("unknown asm instruction '{}'", op_name)),
        }
    }

    /// Extract an integer argument from asm args.
    fn expect_int_arg(&self, args: &[AsmArg], index: usize, op_name: &str) -> Result<i64, String> {
        args.get(index)
            .and_then(|arg| match arg {
                AsmArg::Int(n) => Some(*n),
                _ => None,
            })
            .ok_or_else(|| {
                format!(
                    "{} requires an integer argument at position {}",
                    op_name, index
                )
            })
    }

    /// Extract a float argument from asm args.
    fn expect_float_arg(
        &self,
        args: &[AsmArg],
        index: usize,
        op_name: &str,
    ) -> Result<f64, String> {
        args.get(index)
            .and_then(|arg| match arg {
                AsmArg::Float(f) => Some(*f),
                AsmArg::Int(n) => Some(*n as f64), // Allow int as float
                _ => None,
            })
            .ok_or_else(|| {
                format!(
                    "{} requires a float argument at position {}",
                    op_name, index
                )
            })
    }

    /// Extract a string argument from asm args.
    fn expect_string_arg(
        &self,
        args: &[AsmArg],
        index: usize,
        op_name: &str,
    ) -> Result<String, String> {
        args.get(index)
            .and_then(|arg| match arg {
                AsmArg::String(s) => Some(s.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                format!(
                    "{} requires a string argument at position {}",
                    op_name, index
                )
            })
    }

    // compile_vector_push and compile_vector_pop have been removed
    // vec_push/vec_pop now call vec_push_any/vec_pop_any from stdlib
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
        let chunk = compile("print_debug(42);").unwrap();
        assert!(chunk.main.code.contains(&Op::PushInt(42)));
        assert!(chunk.main.code.contains(&Op::PrintDebug));
    }

    #[test]
    fn test_arithmetic() {
        let chunk = compile("print_debug(1 + 2);").unwrap();
        assert!(chunk.main.code.contains(&Op::Add));
    }

    #[test]
    fn test_function_call() {
        let chunk = compile("fun foo() { return 42; } print_debug(foo());").unwrap();
        assert_eq!(chunk.functions.len(), 1);
        assert_eq!(chunk.functions[0].name, "foo");
    }
}
