use crate::compiler::ast::{AsmArg, BinaryOp, UnaryOp};

use crate::compiler::resolver::{
    ResolvedAsmInstruction, ResolvedExpr, ResolvedFunction, ResolvedProgram, ResolvedStatement,
    ResolvedStruct,
};
use crate::compiler::types::Type;
use crate::vm::{Chunk, DebugInfo, Function, FunctionDebugInfo, Op, ValueType};
use std::collections::HashMap;

/// Maximum nesting depth for @inline expansion (prevents code explosion).
const MAX_INLINE_DEPTH: usize = 4;

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
    /// Map function name -> function index (for calling stdlib functions)
    function_indices: HashMap<String, usize>,
    /// ValueType for each local variable in the currently-being-compiled function
    current_local_types: Vec<ValueType>,
    /// Return ValueType for each function (indexed by function index)
    function_return_types: Vec<ValueType>,
    /// All resolved functions for inline expansion
    inline_functions: Vec<ResolvedFunction>,
    /// Offset applied to local variable slots during inline expansion
    local_offset: usize,
    /// Stack of return-jump patch lists for nested inline expansion.
    /// Empty when not inlining; each level pushes a new Vec.
    inline_return_patches_stack: Vec<Vec<usize>>,
    /// Tracks total locals count for the current function being compiled
    /// (grows as inline expansions add locals)
    current_locals_count: usize,
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
            function_indices: HashMap::new(),
            current_local_types: Vec::new(),
            function_return_types: Vec::new(),
            inline_functions: Vec::new(),
            local_offset: 0,
            inline_return_patches_stack: Vec::new(),
            current_locals_count: 0,
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
            function_indices: HashMap::new(),
            current_local_types: Vec::new(),
            function_return_types: Vec::new(),
            inline_functions: Vec::new(),
            local_offset: 0,
            inline_return_patches_stack: Vec::new(),
            current_locals_count: 0,
        }
    }

    /// Convert the typechecker's full Type to a simplified ValueType for the VM.
    fn type_to_value_type(ty: &Type) -> ValueType {
        match ty {
            Type::Int => ValueType::I64,
            Type::Float => ValueType::F64,
            Type::Bool => ValueType::I32,
            Type::String => ValueType::Ref,
            Type::Array(_)
            | Type::Vector(_)
            | Type::Map(_, _)
            | Type::Struct { .. }
            | Type::GenericStruct { .. }
            | Type::Object(_)
            | Type::Nullable(_)
            | Type::Ptr(_)
            | Type::Dyn => ValueType::Ref,
            Type::Nil => ValueType::Ref,
            _ => ValueType::I64, // Default to I64 for unknown types
        }
    }

    fn infer_index_element_type(object_type: &Option<Type>) -> ValueType {
        match object_type {
            Some(Type::String) => ValueType::I64, // string[i] returns char code (int)
            Some(Type::Array(elem)) | Some(Type::Vector(elem)) => Self::type_to_value_type(elem),
            Some(Type::GenericStruct {
                name, type_args, ..
            }) if name == "Vec" && !type_args.is_empty() => Self::type_to_value_type(&type_args[0]),
            Some(Type::GenericStruct {
                name, type_args, ..
            }) if name == "Map" && type_args.len() >= 2 => Self::type_to_value_type(&type_args[1]),
            Some(Type::Map(_, val)) => Self::type_to_value_type(val),
            _ => ValueType::I64, // fallback
        }
    }

    fn infer_expr_type(&self, expr: &ResolvedExpr) -> ValueType {
        match expr {
            ResolvedExpr::Int(_) => ValueType::I64,
            ResolvedExpr::Float(_) => ValueType::F64,
            ResolvedExpr::Bool(_) => ValueType::I32,
            ResolvedExpr::Str(_) => ValueType::Ref,
            ResolvedExpr::Nil => ValueType::Ref,
            ResolvedExpr::Local(slot) => self
                .current_local_types
                .get(*slot + self.local_offset)
                .copied()
                .unwrap_or(ValueType::I64),
            ResolvedExpr::Array { .. } => ValueType::Ref,
            ResolvedExpr::Index { object_type, .. } => Self::infer_index_element_type(object_type),
            ResolvedExpr::Field { .. } => ValueType::I64, // Most common; TODO: improve
            ResolvedExpr::Unary { op, operand } => match op {
                UnaryOp::Neg => self.infer_expr_type(operand),
                UnaryOp::Not => ValueType::I32, // bool result
            },
            ResolvedExpr::Binary { op, left, .. } => match op {
                BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge
                | BinaryOp::And
                | BinaryOp::Or => ValueType::I32,
                _ => self.infer_expr_type(left), // arithmetic: same type as operands
            },
            ResolvedExpr::Call { func_index, .. } => self
                .function_return_types
                .get(*func_index)
                .copied()
                .unwrap_or(ValueType::I64),
            ResolvedExpr::StructLiteral { .. } => ValueType::Ref,
            ResolvedExpr::MethodCall { .. } => ValueType::I64, // Default
            ResolvedExpr::AssociatedFunctionCall { .. } => ValueType::Ref,
            ResolvedExpr::SpawnFunc { .. } => ValueType::I64,
            ResolvedExpr::Builtin { name, .. } => match name.as_str() {
                "len" | "argc" | "parse_int" | "__umul128_hi" => ValueType::I64,
                "type_of" | "__float_to_string" | "channel" | "recv" | "argv" | "args"
                | "__alloc_heap" | "__alloc_string" | "__null_ptr" | "__ptr_offset" => {
                    ValueType::Ref
                }
                "__heap_load" => ValueType::I64, // Returns raw slot value; type unknown at compile time
                "send" | "join" | "print" | "print_debug" | "__heap_store" => ValueType::Ref, // returns null
                _ => ValueType::I64,
            },
            ResolvedExpr::AsmBlock { .. } => ValueType::I64,
            ResolvedExpr::NewLiteral { .. } => ValueType::Ref,
            ResolvedExpr::Block { expr, .. } => self.infer_expr_type(expr),
            ResolvedExpr::Closure { .. } => ValueType::Ref,
            ResolvedExpr::CallIndirect { .. } => ValueType::I64, // Default; dynamic
            ResolvedExpr::CaptureLoad { .. } => ValueType::I64,  // Default; dynamic
            ResolvedExpr::CaptureStore { .. } => ValueType::Ref, // HeapStore result
            ResolvedExpr::RefCellNew { .. } => ValueType::Ref,   // Returns a Ref
            ResolvedExpr::RefCellLoad { .. } => ValueType::I64,  // Default; dynamic
            ResolvedExpr::AsDyn { .. } => ValueType::Ref,        // Dyn values are heap-allocated
        }
    }

    /// Infer the return ValueType of a function by scanning for return statements.
    fn infer_function_return_type(&self, func: &ResolvedFunction) -> ValueType {
        for stmt in &func.body {
            if let Some(vt) = self.scan_return_type(stmt) {
                return vt;
            }
        }
        ValueType::Ref // implicit nil return
    }

    /// Recursively scan a statement for return expressions and infer their type.
    fn scan_return_type(&self, stmt: &ResolvedStatement) -> Option<ValueType> {
        match stmt {
            ResolvedStatement::Return { value: Some(expr) } => Some(self.infer_expr_type(expr)),
            ResolvedStatement::Return { value: None } => Some(ValueType::Ref),
            ResolvedStatement::If {
                then_block,
                else_block,
                ..
            } => {
                for s in then_block {
                    if let Some(vt) = self.scan_return_type(s) {
                        return Some(vt);
                    }
                }
                if let Some(else_stmts) = else_block {
                    for s in else_stmts {
                        if let Some(vt) = self.scan_return_type(s) {
                            return Some(vt);
                        }
                    }
                }
                None
            }
            ResolvedStatement::While { body, .. } | ResolvedStatement::ForIn { body, .. } => {
                for s in body {
                    if let Some(vt) = self.scan_return_type(s) {
                        return Some(vt);
                    }
                }
                None
            }
            ResolvedStatement::Try {
                try_block,
                catch_block,
                ..
            } => {
                for s in try_block {
                    if let Some(vt) = self.scan_return_type(s) {
                        return Some(vt);
                    }
                }
                for s in catch_block {
                    if let Some(vt) = self.scan_return_type(s) {
                        return Some(vt);
                    }
                }
                None
            }
            _ => None,
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

        // Build function name -> index map for stdlib function calls
        for (idx, func) in program.functions.iter().enumerate() {
            self.function_indices.insert(func.name.clone(), idx);
        }

        // Store resolved functions for inline expansion
        self.inline_functions = program.functions.clone();

        // Pre-compute function return types (first pass)
        self.function_return_types = vec![ValueType::I64; program.functions.len()];
        for (i, func) in program.functions.iter().enumerate() {
            self.current_local_types = func
                .local_types
                .iter()
                .map(Self::type_to_value_type)
                .collect();
            self.function_return_types[i] = self.infer_function_return_type(func);
        }

        // Compile all user-defined functions
        for func in &program.functions {
            // Set local types for this function before compiling
            self.current_local_types = func
                .local_types
                .iter()
                .map(Self::type_to_value_type)
                .collect();
            self.current_locals_count = func.locals_count;
            let compiled = self.compile_function(func)?;
            self.functions.push(compiled);
            // Add debug info placeholder for each function
            if self.emit_debug {
                self.debug.functions.push(FunctionDebugInfo::new());
            }
        }

        // Set local types for main body
        self.current_local_types = program
            .main_local_types
            .iter()
            .map(Self::type_to_value_type)
            .collect();

        // Compile main body
        self.current_locals_count = program.main_locals_count;
        let mut main_ops = Vec::new();
        for stmt in program.main_body {
            self.compile_statement(&stmt, &mut main_ops)?;
        }
        // End of main
        main_ops.push(Op::RefNull); // Return value for main
        main_ops.push(Op::Ret);

        let main_local_types = self.current_local_types.clone();

        let main_func = Function {
            name: "__main__".to_string(),
            arity: 0,
            locals_count: self.current_locals_count,
            code: main_ops,
            stackmap: None, // TODO: generate StackMap
            local_types: main_local_types,
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
            ops.push(Op::RefNull);
            ops.push(Op::Ret);
        }

        let local_types = func
            .local_types
            .iter()
            .map(Self::type_to_value_type)
            .collect();

        Ok(Function {
            name: func.name.clone(),
            arity: func.params.len(),
            locals_count: self.current_locals_count,
            code: ops,
            stackmap: None, // TODO: generate StackMap
            local_types,
        })
    }

    /// Inline-expand a function call. Arguments must already be on the stack.
    fn compile_inline_call(
        &mut self,
        func_index: usize,
        argc: usize,
        ops: &mut Vec<Op>,
    ) -> Result<(), String> {
        let func = self.inline_functions[func_index].clone();

        // Save codegen state
        let saved_offset = self.local_offset;
        let saved_local_types = self.current_local_types.clone();

        // Allocate local slots for the inlined function at the end of caller's locals
        let inline_offset = self.current_locals_count;
        self.current_locals_count += func.locals_count;
        self.local_offset = inline_offset;
        self.inline_return_patches_stack.push(Vec::new());

        // Extend current_local_types with the inlined function's local types
        for ty in &func.local_types {
            self.current_local_types.push(Self::type_to_value_type(ty));
        }

        // Store arguments from stack into local slots (reverse order since last arg is on top)
        for i in (0..argc).rev() {
            ops.push(Op::LocalSet(i + inline_offset));
        }

        // Compile the inlined function body
        for stmt in &func.body {
            self.compile_statement(stmt, ops)?;
        }

        // Implicit return nil (for paths that don't hit an explicit return)
        ops.push(Op::RefNull);

        // Patch all return jumps to point to the end of the inline block
        let end_pos = ops.len();
        if let Some(patches) = self.inline_return_patches_stack.pop() {
            for patch_pos in patches {
                ops[patch_pos] = Op::Jmp(end_pos);
            }
        }

        // Restore codegen state
        self.local_offset = saved_offset;
        self.current_local_types = saved_local_types;

        Ok(())
    }

    fn compile_statement(
        &mut self,
        stmt: &ResolvedStatement,
        ops: &mut Vec<Op>,
    ) -> Result<(), String> {
        match stmt {
            ResolvedStatement::Let { slot, init } => {
                self.compile_expr(init, ops)?;
                ops.push(Op::LocalSet(*slot + self.local_offset));
            }
            ResolvedStatement::Assign { slot, value } => {
                self.compile_expr(value, ops)?;
                ops.push(Op::LocalSet(*slot + self.local_offset));
            }
            ResolvedStatement::IndexAssign {
                object,
                index,
                value,
                object_type,
                ..
            } => {
                // Check if the object is a Vector, Vec<T>, or Array<T> (ptr-based layout)
                let has_ptr_layout = object_type
                    .as_ref()
                    .map(|t| {
                        matches!(t, Type::Vector(_) | Type::Array(_) | Type::String)
                            || matches!(t, Type::GenericStruct { name, .. } if name == "Vec")
                    })
                    .unwrap_or(false);

                if has_ptr_layout {
                    // Ptr-based layout: indirect store via ptr field (slot 0)
                    // HeapStore2 = heap[heap[ref][0]][idx] = val in one op
                    self.compile_expr(object, ops)?;
                    self.compile_expr(index, ops)?;
                    self.compile_expr(value, ops)?;
                    ops.push(Op::HeapStore2);
                } else {
                    // Struct/string assign: direct HeapStoreDyn
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
                    ops.push(Op::I64Const(idx as i64));
                    self.compile_expr(value, ops)?;
                    ops.push(Op::HeapStoreDyn);
                } else {
                    return Err(format!(
                        "unknown field '{}' - object type has been removed, use map functions instead",
                        field
                    ));
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
                ops.push(Op::BrIfFalse(0)); // Placeholder

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
                    ops[jump_to_else] = Op::BrIfFalse(else_start);

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
                    ops[jump_to_else] = Op::BrIfFalse(after_then);
                }
            }
            ResolvedStatement::While { condition, body } => {
                let loop_start = ops.len();

                self.compile_expr(condition, ops)?;

                let jump_to_end = ops.len();
                ops.push(Op::BrIfFalse(0)); // Placeholder

                for stmt in body {
                    self.compile_statement(stmt, ops)?;
                }

                ops.push(Op::Jmp(loop_start));

                let loop_end = ops.len();
                ops[jump_to_end] = Op::BrIfFalse(loop_end);
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

                let var_slot = *slot + self.local_offset;
                let idx_slot = slot + 1 + self.local_offset;
                let arr_slot = slot + 2 + self.local_offset;

                // Store array
                self.compile_expr(iterable, ops)?;
                ops.push(Op::LocalSet(arr_slot));

                // Initialize index to 0
                ops.push(Op::I64Const(0));
                ops.push(Op::LocalSet(idx_slot));

                let loop_start = ops.len();

                // Check: idx < arr.len (Array<T> struct: slot 1 = len)
                ops.push(Op::LocalGet(idx_slot));
                ops.push(Op::LocalGet(arr_slot));
                ops.push(Op::HeapLoad(1)); // len field of Array<T>
                ops.push(Op::I64LtS);

                let jump_to_end = ops.len();
                ops.push(Op::BrIfFalse(0)); // Placeholder

                // x = arr[idx] (Array<T> struct: slot 0 = ptr)
                ops.push(Op::LocalGet(arr_slot));
                ops.push(Op::HeapLoad(0)); // ptr field of Array<T>
                ops.push(Op::LocalGet(idx_slot));
                ops.push(Op::HeapLoadDyn);
                ops.push(Op::LocalSet(var_slot));

                // Body
                for stmt in body {
                    self.compile_statement(stmt, ops)?;
                }

                // idx = idx + 1
                ops.push(Op::LocalGet(idx_slot));
                ops.push(Op::I64Const(1));
                ops.push(Op::I64Add);
                ops.push(Op::LocalSet(idx_slot));

                // Jump back to loop start
                ops.push(Op::Jmp(loop_start));

                // End of loop
                let loop_end = ops.len();
                ops[jump_to_end] = Op::BrIfFalse(loop_end);
            }
            ResolvedStatement::Return { value } => {
                if let Some(value) = value {
                    self.compile_expr(value, ops)?;
                } else {
                    ops.push(Op::RefNull); // Return nil for void
                }
                if let Some(patches) = self.inline_return_patches_stack.last_mut() {
                    // Inside inline expansion: jump to end of inline block
                    patches.push(ops.len());
                    ops.push(Op::Jmp(0)); // Placeholder, patched later
                } else {
                    ops.push(Op::Ret);
                }
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
                ops.push(Op::LocalSet(*catch_slot + self.local_offset));

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
                ops.push(Op::Drop); // Discard result
            }
            ResolvedStatement::RefCellStore { slot, value } => {
                // Store to a promoted var variable through its RefCell (outer scope)
                // LocalGet(slot) gives the RefCell ref, then store value into RefCell[0]
                ops.push(Op::LocalGet(*slot + self.local_offset));
                self.compile_expr(value, ops)?;
                ops.push(Op::HeapStore(0));
            }
            ResolvedStatement::MatchDyn {
                dyn_slot,
                expr,
                arms,
                default_block,
            } => {
                // Compile the dyn expression and store in the dyn_slot
                self.compile_expr(expr, ops)?;
                ops.push(Op::LocalSet(*dyn_slot));

                // Compile as if-else chain on type tags
                let mut jump_to_end_patches = Vec::new();

                for arm in arms {
                    // Load dyn value and get type tag
                    ops.push(Op::LocalGet(*dyn_slot));
                    ops.push(Op::DynTypeTag);
                    ops.push(Op::I64Const(arm.type_tag as i64));
                    ops.push(Op::I64Eq);

                    // Branch to next arm if tag doesn't match
                    let jump_to_next = ops.len();
                    ops.push(Op::BrIfFalse(0)); // placeholder

                    // Unbox the value and bind to the arm's variable
                    ops.push(Op::LocalGet(*dyn_slot));
                    ops.push(Op::DynUnbox);
                    ops.push(Op::LocalSet(arm.var_slot));

                    // Compile arm body
                    for stmt in &arm.body {
                        self.compile_statement(stmt, ops)?;
                    }

                    // Jump to end of match
                    jump_to_end_patches.push(ops.len());
                    ops.push(Op::Jmp(0)); // placeholder

                    // Patch jump to next arm
                    let next_arm = ops.len();
                    ops[jump_to_next] = Op::BrIfFalse(next_arm);
                }

                // Default block
                for stmt in default_block {
                    self.compile_statement(stmt, ops)?;
                }

                // Patch all jumps to end
                let end = ops.len();
                for patch_idx in jump_to_end_patches {
                    ops[patch_idx] = Op::Jmp(end);
                }
            }
        }

        Ok(())
    }

    fn compile_expr(&mut self, expr: &ResolvedExpr, ops: &mut Vec<Op>) -> Result<(), String> {
        match expr {
            ResolvedExpr::Int(value) => {
                ops.push(Op::I64Const(*value));
            }
            ResolvedExpr::Float(value) => {
                ops.push(Op::F64Const(*value));
            }
            ResolvedExpr::Bool(value) => {
                if *value {
                    ops.push(Op::I32Const(1));
                } else {
                    ops.push(Op::I32Const(0));
                }
            }
            ResolvedExpr::Str(value) => {
                let idx = self.add_string(value.clone());
                ops.push(Op::StringConst(idx));
            }
            ResolvedExpr::Nil => {
                ops.push(Op::RefNull);
            }
            ResolvedExpr::Local(slot) => {
                ops.push(Op::LocalGet(*slot + self.local_offset));
            }
            ResolvedExpr::Array { elements } => {
                // Array<T> struct layout: [ptr, len]
                // 1. Push all elements and allocate data array
                let n = elements.len();
                for elem in elements {
                    self.compile_expr(elem, ops)?;
                }
                ops.push(Op::HeapAlloc(n)); // data array with n elements
                // Stack: [data_ptr]

                // 2. Create Array<T> struct: { ptr: data_ptr, len: n }
                ops.push(Op::I64Const(n as i64));
                ops.push(Op::HeapAllocArray(2, 2)); // Array struct with [ptr, len]
            }
            ResolvedExpr::Index {
                object,
                index,
                object_type,
                ..
            } => {
                // Check if the object is a Vector, Vec<T>, or Array<T> (ptr-based layout)
                let has_ptr_layout = object_type
                    .as_ref()
                    .map(|t| {
                        matches!(t, Type::Vector(_) | Type::Array(_) | Type::String)
                            || matches!(t, Type::GenericStruct { name, .. } if name == "Vec")
                    })
                    .unwrap_or(false);

                if has_ptr_layout {
                    // Ptr-based layout: indirect access via ptr field (slot 0)
                    // HeapLoad2 = heap[heap[ref][0]][idx] in one op
                    self.compile_expr(object, ops)?;
                    self.compile_expr(index, ops)?;
                    ops.push(Op::HeapLoad2);
                } else {
                    // Struct/string access: direct HeapLoadDyn
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
                    ops.push(Op::I64Const(idx as i64));
                    ops.push(Op::HeapLoadDyn);
                } else {
                    return Err(format!(
                        "unknown field '{}' - object type has been removed, use map functions instead",
                        field
                    ));
                }
            }
            ResolvedExpr::Unary { op, operand } => {
                self.compile_expr(operand, ops)?;
                match op {
                    UnaryOp::Neg => match self.infer_expr_type(operand) {
                        ValueType::I64 => ops.push(Op::I64Neg),
                        ValueType::F64 => ops.push(Op::F64Neg),
                        ValueType::F32 => ops.push(Op::F32Neg),
                        _ => ops.push(Op::I64Neg),
                    },
                    UnaryOp::Not => ops.push(Op::I32Eqz),
                }
            }
            ResolvedExpr::Binary { op, left, right } => {
                // Handle short-circuit evaluation for && and ||
                // BrIfFalse/BrIf pop the condition value, so we need to Dup first
                // to keep the result on stack when short-circuiting
                match op {
                    BinaryOp::And => {
                        // For &&: if left is false, skip right and keep false on stack
                        self.compile_expr(left, ops)?;
                        ops.push(Op::Dup); // Duplicate for the jump check
                        let jump_if_false = ops.len();
                        ops.push(Op::BrIfFalse(0)); // Placeholder, consumes the dup'd value
                        ops.push(Op::Drop); // Pop the original true value
                        self.compile_expr(right, ops)?;
                        let end = ops.len();
                        ops[jump_if_false] = Op::BrIfFalse(end);
                        // If left was false: jump taken, original false still on stack
                        // If left was true: pop it, right's value is on stack
                        return Ok(());
                    }
                    BinaryOp::Or => {
                        // For ||: if left is true, skip right and keep true on stack
                        self.compile_expr(left, ops)?;
                        ops.push(Op::Dup); // Duplicate for the jump check
                        let jump_if_true = ops.len();
                        ops.push(Op::BrIf(0)); // Placeholder, consumes the dup'd value
                        ops.push(Op::Drop); // Pop the original false value
                        self.compile_expr(right, ops)?;
                        let end = ops.len();
                        ops[jump_if_true] = Op::BrIf(end);
                        // If left was true: jump taken, original true still on stack
                        // If left was false: pop it, right's value is on stack
                        return Ok(());
                    }
                    _ => {}
                }

                self.compile_expr(left, ops)?;
                self.compile_expr(right, ops)?;

                match op {
                    BinaryOp::Add => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64Add),
                        ValueType::F64 => ops.push(Op::F64Add),
                        ValueType::I32 => ops.push(Op::I32Add),
                        ValueType::F32 => ops.push(Op::F32Add),
                        ValueType::Ref => {
                            // String concatenation: call string_concat(a, b)
                            if let Some(&func_idx) = self.function_indices.get("string_concat") {
                                if self
                                    .inline_functions
                                    .get(func_idx)
                                    .is_some_and(|f| f.is_inline)
                                    && self.inline_return_patches_stack.len() < MAX_INLINE_DEPTH
                                {
                                    self.compile_inline_call(func_idx, 2, ops)?;
                                } else {
                                    ops.push(Op::Call(func_idx, 2));
                                }
                            } else {
                                ops.push(Op::I64Add); // fallback if no stdlib
                            }
                        }
                    },
                    BinaryOp::Sub => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64Sub),
                        ValueType::F64 => ops.push(Op::F64Sub),
                        ValueType::I32 => ops.push(Op::I32Sub),
                        ValueType::F32 => ops.push(Op::F32Sub),
                        _ => ops.push(Op::I64Sub),
                    },
                    BinaryOp::Mul => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64Mul),
                        ValueType::F64 => ops.push(Op::F64Mul),
                        ValueType::I32 => ops.push(Op::I32Mul),
                        ValueType::F32 => ops.push(Op::F32Mul),
                        _ => ops.push(Op::I64Mul),
                    },
                    BinaryOp::Div => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64DivS),
                        ValueType::F64 => ops.push(Op::F64Div),
                        ValueType::I32 => ops.push(Op::I32DivS),
                        ValueType::F32 => ops.push(Op::F32Div),
                        _ => ops.push(Op::I64DivS),
                    },
                    BinaryOp::Mod => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64RemS),
                        ValueType::I32 => ops.push(Op::I32RemS),
                        _ => ops.push(Op::I64RemS),
                    },
                    BinaryOp::Eq => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64Eq),
                        ValueType::F64 => ops.push(Op::F64Eq),
                        ValueType::I32 => ops.push(Op::I32Eq),
                        ValueType::F32 => ops.push(Op::F32Eq),
                        ValueType::Ref => ops.push(Op::RefEq),
                    },
                    BinaryOp::Ne => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64Ne),
                        ValueType::F64 => ops.push(Op::F64Ne),
                        ValueType::I32 => ops.push(Op::I32Ne),
                        ValueType::F32 => ops.push(Op::F32Ne),
                        ValueType::Ref => {
                            ops.push(Op::RefEq);
                            ops.push(Op::I32Eqz);
                        }
                    },
                    BinaryOp::Lt => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64LtS),
                        ValueType::F64 => ops.push(Op::F64Lt),
                        ValueType::I32 => ops.push(Op::I32LtS),
                        ValueType::F32 => ops.push(Op::F32Lt),
                        _ => ops.push(Op::I64LtS),
                    },
                    BinaryOp::Le => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64LeS),
                        ValueType::F64 => ops.push(Op::F64Le),
                        ValueType::I32 => ops.push(Op::I32LeS),
                        ValueType::F32 => ops.push(Op::F32Le),
                        _ => ops.push(Op::I64LeS),
                    },
                    BinaryOp::Gt => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64GtS),
                        ValueType::F64 => ops.push(Op::F64Gt),
                        ValueType::I32 => ops.push(Op::I32GtS),
                        ValueType::F32 => ops.push(Op::F32Gt),
                        _ => ops.push(Op::I64GtS),
                    },
                    BinaryOp::Ge => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64GeS),
                        ValueType::F64 => ops.push(Op::F64Ge),
                        ValueType::I32 => ops.push(Op::I32GeS),
                        ValueType::F32 => ops.push(Op::F32Ge),
                        _ => ops.push(Op::I64GeS),
                    },
                    BinaryOp::BitwiseAnd => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64And),
                        _ => {
                            return Err(
                                "bitwise AND is only supported for integer types".to_string()
                            );
                        }
                    },
                    BinaryOp::BitwiseOr => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64Or),
                        _ => {
                            return Err(
                                "bitwise OR is only supported for integer types".to_string()
                            );
                        }
                    },
                    BinaryOp::BitwiseXor => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64Xor),
                        _ => {
                            return Err(
                                "bitwise XOR is only supported for integer types".to_string()
                            );
                        }
                    },
                    BinaryOp::Shl => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64Shl),
                        _ => {
                            return Err(
                                "left shift is only supported for integer types".to_string()
                            );
                        }
                    },
                    BinaryOp::Shr => match self.infer_expr_type(left) {
                        ValueType::I64 => ops.push(Op::I64ShrS),
                        _ => {
                            return Err(
                                "right shift is only supported for integer types".to_string()
                            );
                        }
                    },
                    BinaryOp::And | BinaryOp::Or => unreachable!(),
                }
            }
            ResolvedExpr::Call { func_index, args } => {
                if self.inline_return_patches_stack.len() < MAX_INLINE_DEPTH
                    && self
                        .inline_functions
                        .get(*func_index)
                        .is_some_and(|f| f.is_inline)
                {
                    // Push arguments
                    for arg in args {
                        self.compile_expr(arg, ops)?;
                    }
                    self.compile_inline_call(*func_index, args.len(), ops)?;
                } else {
                    // Normal call
                    for arg in args {
                        self.compile_expr(arg, ops)?;
                    }
                    ops.push(Op::Call(*func_index, args.len()));
                }
            }
            ResolvedExpr::Builtin { name, args, .. } => {
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
                                ops.push(Op::Drop); // discard return value
                                // Call print_str("\n")
                                let newline_idx = self.add_string("\n".to_string());
                                ops.push(Op::StringConst(newline_idx));
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
                        // Both Array<T> and String have [ptr, len] layout
                        ops.push(Op::HeapLoad(1));
                    }
                    "type_of" => {
                        if args.len() != 1 {
                            return Err("type_of takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::TypeOf);
                    }
                    "__float_to_string" => {
                        if args.len() != 1 {
                            return Err("__float_to_string takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::FloatToString);
                    }
                    "parse_int" => {
                        if args.len() != 1 {
                            return Err("parse_int takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::ParseInt);
                    }
                    "__umul128_hi" => {
                        if args.len() != 2 {
                            return Err("__umul128_hi takes exactly 2 arguments".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        self.compile_expr(&args[1], ops)?;
                        ops.push(Op::UMul128Hi);
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
                        ops.push(Op::RefNull);
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
                        ops.push(Op::RefNull); // returns nil
                    }
                    "__alloc_heap" => {
                        // __alloc_heap(size) -> ref to newly allocated heap object with size slots
                        if args.len() != 1 {
                            return Err("__alloc_heap takes exactly 1 argument (size)".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::HeapAllocDynSimple);
                    }
                    "__null_ptr" => {
                        // __null_ptr() -> null pointer (0 as Ref)
                        if !args.is_empty() {
                            return Err("__null_ptr takes 0 arguments".to_string());
                        }
                        ops.push(Op::RefNull);
                    }
                    "__ptr_offset" => {
                        // __ptr_offset(ptr, offset) -> ptr with slot_offset += offset
                        if args.len() != 2 {
                            return Err(
                                "__ptr_offset takes exactly 2 arguments (ptr, offset)".to_string()
                            );
                        }
                        self.compile_expr(&args[0], ops)?;
                        self.compile_expr(&args[1], ops)?;
                        ops.push(Op::HeapOffsetRef);
                    }
                    "__alloc_string" => {
                        // __alloc_string(data_ref, len) -> string object [data_ref, len]
                        if args.len() != 2 {
                            return Err("__alloc_string takes exactly 2 arguments (data_ref, len)"
                                .to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        self.compile_expr(&args[1], ops)?;
                        ops.push(Op::HeapAllocArray(2, 1)); // String struct with [ptr, len]
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
                ops.push(Op::HeapAlloc(fields.len()));
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

                let total_args = args.len() + 1; // self + args

                if self.inline_return_patches_stack.len() < MAX_INLINE_DEPTH
                    && self
                        .inline_functions
                        .get(*func_index)
                        .is_some_and(|f| f.is_inline)
                {
                    self.compile_inline_call(*func_index, total_args, ops)?;
                } else {
                    ops.push(Op::Call(*func_index, total_args));
                }
            }
            ResolvedExpr::AssociatedFunctionCall {
                func_index,
                args,
                return_struct_name: _,
            } => {
                // Push arguments (no self for associated functions)
                for arg in args {
                    self.compile_expr(arg, ops)?;
                }

                if self.inline_return_patches_stack.len() < MAX_INLINE_DEPTH
                    && self
                        .inline_functions
                        .get(*func_index)
                        .is_some_and(|f| f.is_inline)
                {
                    self.compile_inline_call(*func_index, args.len(), ops)?;
                } else {
                    ops.push(Op::Call(*func_index, args.len()));
                }
            }
            ResolvedExpr::AsmBlock {
                input_slots,
                output_type: _,
                body,
            } => {
                // Push input variables onto the stack (left to right)
                for slot in input_slots {
                    ops.push(Op::LocalGet(*slot));
                }

                // Compile each asm instruction
                for inst in body {
                    self.compile_asm_instruction(inst, ops)?;
                }

                // If no output type, the result is whatever is on the stack
                // The caller is responsible for handling the stack state
            }
            ResolvedExpr::NewLiteral { .. } => {
                // NewLiteral should have been desugared to Block before reaching codegen.
                // If we get here, it means the desugar phase didn't run correctly.
                panic!("NewLiteral should have been desugared before codegen");
            }
            ResolvedExpr::Block { statements, expr } => {
                // Compile all statements in the block
                for stmt in statements {
                    self.compile_statement(stmt, ops)?;
                }
                // Compile the final expression - its result is the block's result
                self.compile_expr(expr, ops)?;
            }
            ResolvedExpr::Closure {
                func_index,
                captures,
            } => {
                // Build closure heap object using generic heap instructions:
                // slots[0] = func_index, slots[1..] = captured values or RefCell refs
                ops.push(Op::I64Const(*func_index as i64));
                for cap in captures {
                    // For both let and var: LocalGet pushes the value.
                    // For var (mutable): the local already holds a RefCell reference,
                    // so we share it by copying the reference.
                    ops.push(Op::LocalGet(cap.outer_slot + self.local_offset));
                }
                // HeapAlloc pops (1 + n_captures) values from stack in push order
                ops.push(Op::HeapAlloc(1 + captures.len()));
            }
            ResolvedExpr::CallIndirect { callee, args } => {
                // Push the closure reference first
                self.compile_expr(callee, ops)?;
                // Then push arguments
                for arg in args {
                    self.compile_expr(arg, ops)?;
                }
                // CallIndirect pops argc args + callable ref, calls the function
                ops.push(Op::CallIndirect(args.len()));
            }
            ResolvedExpr::CaptureLoad { offset, is_ref } => {
                // Load a captured variable from the closure reference (slot 0)
                ops.push(Op::LocalGet(self.local_offset));
                ops.push(Op::HeapLoad(*offset));
                if *is_ref {
                    // Dereference the RefCell to get the actual value
                    ops.push(Op::HeapLoad(0));
                }
            }
            ResolvedExpr::CaptureStore { offset, value } => {
                // Store to a captured var variable through its RefCell
                // closure_ref -> HeapLoad(offset) -> RefCell; then store value into RefCell[0]
                ops.push(Op::LocalGet(self.local_offset));
                ops.push(Op::HeapLoad(*offset));
                self.compile_expr(value, ops)?;
                ops.push(Op::HeapStore(0));
                // Push nil so this works as an expression (Stmt::Expr will Drop it)
                ops.push(Op::RefNull);
            }
            ResolvedExpr::RefCellNew { value } => {
                // Create a 1-slot heap object (RefCell) wrapping the value
                self.compile_expr(value, ops)?;
                ops.push(Op::HeapAlloc(1));
            }
            ResolvedExpr::RefCellLoad { slot } => {
                // Load value from RefCell: LocalGet(slot) gives the RefCell ref, HeapLoad(0) reads the value
                ops.push(Op::LocalGet(*slot + self.local_offset));
                ops.push(Op::HeapLoad(0));
            }
            ResolvedExpr::AsDyn { expr, type_tag } => {
                self.compile_expr(expr, ops)?;
                ops.push(Op::DynBox(*type_tag));
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
            // ========================
            // Constants & Stack (new names)
            // ========================
            "I32Const" => {
                let value = self.expect_int_arg(args, 0, "I32Const")? as i32;
                Ok(Op::I32Const(value))
            }
            "I64Const" | "PushInt" => {
                let value = self.expect_int_arg(args, 0, op_name)?;
                Ok(Op::I64Const(value))
            }
            "F32Const" => {
                let value = self.expect_float_arg(args, 0, "F32Const")? as f32;
                Ok(Op::F32Const(value))
            }
            "F64Const" | "PushFloat" => {
                let value = self.expect_float_arg(args, 0, op_name)?;
                Ok(Op::F64Const(value))
            }
            "PushTrue" => Ok(Op::I32Const(1)),
            "PushFalse" => Ok(Op::I32Const(0)),
            "RefNull" | "PushNull" => Ok(Op::RefNull),
            "StringConst" | "PushString" => {
                let value = self.expect_string_arg(args, 0, op_name)?;
                let idx = self.add_string(value);
                Ok(Op::StringConst(idx))
            }
            "Drop" | "Pop" => Ok(Op::Drop),
            "Dup" => Ok(Op::Dup),
            "Pick" => {
                let n = self.expect_int_arg(args, 0, "Pick")? as usize;
                Ok(Op::Pick(n))
            }
            "PickDyn" => Ok(Op::PickDyn),

            // ========================
            // Local Variables (new names + aliases)
            // ========================
            "LocalGet" | "GetL" => {
                let slot = self.expect_int_arg(args, 0, op_name)? as usize;
                Ok(Op::LocalGet(slot))
            }
            "LocalSet" | "SetL" => {
                let slot = self.expect_int_arg(args, 0, op_name)? as usize;
                Ok(Op::LocalSet(slot))
            }

            // ========================
            // i32 Arithmetic
            // ========================
            "I32Add" => Ok(Op::I32Add),
            "I32Sub" => Ok(Op::I32Sub),
            "I32Mul" => Ok(Op::I32Mul),
            "I32DivS" => Ok(Op::I32DivS),
            "I32RemS" => Ok(Op::I32RemS),
            "I32Eqz" | "Not" => Ok(Op::I32Eqz),

            // ========================
            // i64 Arithmetic (+ old untyped aliases)
            // ========================
            "I64Add" | "Add" => Ok(Op::I64Add),
            "I64Sub" | "Sub" => Ok(Op::I64Sub),
            "I64Mul" | "Mul" => Ok(Op::I64Mul),
            "I64DivS" | "Div" => Ok(Op::I64DivS),
            "I64RemS" | "Mod" => Ok(Op::I64RemS),
            "I64Neg" | "Neg" => Ok(Op::I64Neg),
            "I64And" => Ok(Op::I64And),
            "I64Or" => Ok(Op::I64Or),
            "I64Xor" => Ok(Op::I64Xor),
            "I64Shl" => Ok(Op::I64Shl),
            "I64ShrS" => Ok(Op::I64ShrS),
            "I64ShrU" => Ok(Op::I64ShrU),
            "UMul128Hi" => Ok(Op::UMul128Hi),

            // ========================
            // f32 Arithmetic
            // ========================
            "F32Add" => Ok(Op::F32Add),
            "F32Sub" => Ok(Op::F32Sub),
            "F32Mul" => Ok(Op::F32Mul),
            "F32Div" => Ok(Op::F32Div),
            "F32Neg" => Ok(Op::F32Neg),

            // ========================
            // f64 Arithmetic
            // ========================
            "F64Add" => Ok(Op::F64Add),
            "F64Sub" => Ok(Op::F64Sub),
            "F64Mul" => Ok(Op::F64Mul),
            "F64Div" => Ok(Op::F64Div),
            "F64Neg" => Ok(Op::F64Neg),

            // ========================
            // i32 Comparison
            // ========================
            "I32Eq" => Ok(Op::I32Eq),
            "I32Ne" => Ok(Op::I32Ne),
            "I32LtS" => Ok(Op::I32LtS),
            "I32LeS" => Ok(Op::I32LeS),
            "I32GtS" => Ok(Op::I32GtS),
            "I32GeS" => Ok(Op::I32GeS),

            // ========================
            // i64 Comparison (+ old untyped aliases)
            // ========================
            "I64Eq" | "Eq" => Ok(Op::I64Eq),
            "I64Ne" | "Ne" => Ok(Op::I64Ne),
            "I64LtS" | "Lt" => Ok(Op::I64LtS),
            "I64LeS" | "Le" => Ok(Op::I64LeS),
            "I64GtS" | "Gt" => Ok(Op::I64GtS),
            "I64GeS" | "Ge" => Ok(Op::I64GeS),

            // ========================
            // f32 Comparison
            // ========================
            "F32Eq" => Ok(Op::F32Eq),
            "F32Ne" => Ok(Op::F32Ne),
            "F32Lt" => Ok(Op::F32Lt),
            "F32Le" => Ok(Op::F32Le),
            "F32Gt" => Ok(Op::F32Gt),
            "F32Ge" => Ok(Op::F32Ge),

            // ========================
            // f64 Comparison
            // ========================
            "F64Eq" => Ok(Op::F64Eq),
            "F64Ne" => Ok(Op::F64Ne),
            "F64Lt" => Ok(Op::F64Lt),
            "F64Le" => Ok(Op::F64Le),
            "F64Gt" => Ok(Op::F64Gt),
            "F64Ge" => Ok(Op::F64Ge),

            // ========================
            // Ref Comparison
            // ========================
            "RefEq" => Ok(Op::RefEq),
            "RefIsNull" => Ok(Op::RefIsNull),

            // ========================
            // Type Conversion
            // ========================
            "I32WrapI64" => Ok(Op::I32WrapI64),
            "I64ExtendI32S" => Ok(Op::I64ExtendI32S),
            "I64ExtendI32U" => Ok(Op::I64ExtendI32U),
            "F64ConvertI64S" => Ok(Op::F64ConvertI64S),
            "I64TruncF64S" => Ok(Op::I64TruncF64S),
            "F64ConvertI32S" => Ok(Op::F64ConvertI32S),
            "F32ConvertI32S" => Ok(Op::F32ConvertI32S),
            "F32ConvertI64S" => Ok(Op::F32ConvertI64S),
            "I32TruncF32S" => Ok(Op::I32TruncF32S),
            "I32TruncF64S" => Ok(Op::I32TruncF64S),
            "I64TruncF32S" => Ok(Op::I64TruncF32S),
            "F32DemoteF64" => Ok(Op::F32DemoteF64),
            "F64PromoteF32" => Ok(Op::F64PromoteF32),
            "F64ReinterpretAsI64" => Ok(Op::F64ReinterpretAsI64),

            // ========================
            // Control Flow
            // ========================
            "Jmp" => {
                let target = self.expect_int_arg(args, 0, "Jmp")? as usize;
                Ok(Op::Jmp(target))
            }
            "BrIfFalse" | "JmpIfFalse" => {
                let target = self.expect_int_arg(args, 0, op_name)? as usize;
                Ok(Op::BrIfFalse(target))
            }
            "BrIf" | "JmpIfTrue" => {
                let target = self.expect_int_arg(args, 0, op_name)? as usize;
                Ok(Op::BrIf(target))
            }

            // Functions - FORBIDDEN
            "Call" => Err("Call instruction is forbidden in asm block".to_string()),
            "Ret" => Err("Ret instruction is forbidden in asm block".to_string()),

            // ========================
            // Heap Operations (new names + aliases)
            // ========================
            "HeapAlloc" | "AllocHeap" => {
                let n = self.expect_int_arg(args, 0, op_name)? as usize;
                Ok(Op::HeapAlloc(n))
            }
            "HeapAllocArray" => {
                let n = self.expect_int_arg(args, 0, op_name)? as usize;
                Ok(Op::HeapAllocArray(n, 2))
            }
            "HeapAllocDyn" | "AllocHeapDyn" => Ok(Op::HeapAllocDyn),
            "HeapAllocDynSimple" => Ok(Op::HeapAllocDynSimple),
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

            // Type operations
            "TypeOf" => Ok(Op::TypeOf),
            "FloatToString" => Ok(Op::FloatToString),
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

            // CLI arguments
            "Argc" => Ok(Op::Argc),
            "Argv" => Ok(Op::Argv),
            "Args" => Ok(Op::Args),

            // Thread operations
            "ThreadSpawn" => {
                let func_index = self.expect_int_arg(args, 0, "ThreadSpawn")? as usize;
                Ok(Op::ThreadSpawn(func_index))
            }
            "ChannelCreate" => Ok(Op::ChannelCreate),
            "ChannelSend" => Ok(Op::ChannelSend),
            "ChannelRecv" => Ok(Op::ChannelRecv),
            "ThreadJoin" => Ok(Op::ThreadJoin),

            // Syscall
            "Syscall" => {
                let num = self.expect_int_arg(args, 0, "Syscall")? as usize;
                let argc = self.expect_int_arg(args, 1, "Syscall")? as usize;
                Ok(Op::Syscall(num, argc))
            }

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
        assert!(chunk.main.code.contains(&Op::I64Const(42)));
        assert!(chunk.main.code.contains(&Op::PrintDebug));
    }

    #[test]
    fn test_arithmetic() {
        let chunk = compile("print_debug(1 + 2);").unwrap();
        assert!(chunk.main.code.contains(&Op::I64Add));
    }

    #[test]
    fn test_function_call() {
        let chunk = compile("fun foo() { return 42; } print_debug(foo());").unwrap();
        assert_eq!(chunk.functions.len(), 1);
        assert_eq!(chunk.functions[0].name, "foo");
    }
}
