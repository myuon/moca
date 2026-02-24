use crate::compiler::ast::{AsmArg, BinaryOp, UnaryOp};

use crate::compiler::resolver::{
    MatchDynArmKind, ResolvedAsmInstruction, ResolvedExpr, ResolvedFunction, ResolvedProgram,
    ResolvedStatement, ResolvedStruct,
};
use crate::compiler::types::Type;
use crate::vm::{Chunk, DebugInfo, ElemKind, Function, FunctionDebugInfo, Op, ValueType};
use std::collections::HashMap;

/// Maximum nesting depth for @inline expansion (prevents code explosion).
const MAX_INLINE_DEPTH: usize = 4;

/// Determine ElemKind for a direct element type.
fn elem_kind_for_element_type(ty: &Type) -> ElemKind {
    match ty {
        Type::Int | Type::Bool => ElemKind::I64,
        Type::Float => ElemKind::F64,
        Type::String | Type::GenericStruct { .. } | Type::Nullable(_) | Type::Dyn => ElemKind::Ref,
        // Only treat Struct as Ref if it has fields (concrete struct).
        // Struct { name: "T", fields: [] } is an unresolved type parameter, not a real struct.
        Type::Struct { fields, .. } if !fields.is_empty() => ElemKind::Ref,
        _ => ElemKind::Tagged,
    }
}

/// Determine ElemKind for a collection (Array/Vec) based on its element type.
fn elem_kind_for_collection(object_type: &Option<Type>) -> ElemKind {
    let elem_type = object_type
        .as_ref()
        .and_then(|t| t.collection_element_type());
    match elem_type {
        Some(ty) => elem_kind_for_element_type(ty),
        None => ElemKind::Tagged,
    }
}

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
    /// Type descriptor table for dyn type info (tag_name -> index)
    type_descriptor_indices: HashMap<String, usize>,
    /// Type descriptors collected during compilation
    type_descriptors: Vec<crate::vm::TypeDescriptor>,
    /// Interface descriptors collected during compilation
    interface_descriptors: Vec<crate::vm::InterfaceDescriptor>,
    /// Interface name -> index in interface_descriptors
    interface_descriptor_indices: HashMap<String, usize>,
    /// Interface name -> sorted method names (from resolver)
    interface_method_names: HashMap<String, Vec<String>>,
    /// Full Type for each local variable in the currently-being-compiled function
    /// (used to derive ElemKind for __alloc_heap calls)
    current_local_full_types: Vec<Type>,
    /// Struct field types: struct_name -> Vec<(field_name, Type)>
    struct_field_type_map: HashMap<String, Vec<(String, Type)>>,
    /// ElemKind for the current collection method (Vec/Array) being compiled.
    /// Derived from the struct's `data: ptr<T>` field when compiling a method.
    current_collection_elem_kind: Option<ElemKind>,
}

impl Default for Codegen {
    fn default() -> Self {
        Self::new()
    }
}

impl Codegen {
    /// Flag bit used to mark interface descriptor indices in GlobalGet during codegen.
    /// These are resolved to final global indices in a fixup pass after all code is generated.
    const IFACE_GLOBAL_FLAG: usize = 1 << 31;

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
            type_descriptor_indices: HashMap::new(),
            type_descriptors: Vec::new(),
            interface_descriptors: Vec::new(),
            interface_descriptor_indices: HashMap::new(),
            interface_method_names: HashMap::new(),
            current_local_full_types: Vec::new(),
            struct_field_type_map: HashMap::new(),
            current_collection_elem_kind: None,
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
            type_descriptor_indices: HashMap::new(),
            type_descriptors: Vec::new(),
            interface_descriptors: Vec::new(),
            interface_descriptor_indices: HashMap::new(),
            interface_method_names: HashMap::new(),
            current_local_full_types: Vec::new(),
            struct_field_type_map: HashMap::new(),
            current_collection_elem_kind: None,
        }
    }

    /// Convert the typechecker's full Type to a simplified ValueType for the VM.
    fn type_to_value_type(ty: &Type) -> ValueType {
        match ty {
            Type::Int => ValueType::I64,
            Type::Float => ValueType::F64,
            Type::Bool => ValueType::I32,
            Type::String => ValueType::Ref,
            Type::Struct { .. }
            | Type::GenericStruct { .. }
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
            Some(t) if t.is_array() || t.is_vec() => t
                .collection_element_type()
                .map(Self::type_to_value_type)
                .unwrap_or(ValueType::I64),
            Some(t) if t.is_map() => t
                .map_key_value_types()
                .map(|(_, v)| Self::type_to_value_type(v))
                .unwrap_or(ValueType::I64),
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
                "len" | "argc" | "__umul128_hi" | "__typeof" | "__heap_size" => ValueType::I64,
                "channel" | "recv" | "argv" | "args" | "__alloc_heap" | "__alloc_string"
                | "__null_ptr" | "__ptr_offset" => ValueType::Ref,
                "__heap_load" => ValueType::I64, // Returns raw slot value; type unknown at compile time
                "send" | "join" | "print" | "__heap_store" => ValueType::Ref, // returns null
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
            ResolvedExpr::VtableMethodCall { .. } => ValueType::I64, // Default; dynamic
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
            if !s.field_types.is_empty() {
                self.struct_field_type_map
                    .insert(s.name.clone(), s.field_types.clone());
            }
        }
        self.structs = structs;
    }

    /// Look up a field index, optionally scoped to a specific struct.
    /// When struct_name is provided, only that struct's fields are checked.
    /// When None, falls back to searching all structs (ambiguous but backward-compatible).
    fn get_field_index(&self, field_name: &str, struct_name: Option<&str>) -> Option<usize> {
        if let Some(sn) = struct_name
            && let Some(field_map) = self.struct_field_indices.get(sn)
        {
            return field_map.get(field_name).copied();
        }
        // Fallback: check all structs (non-deterministic if field name is ambiguous)
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

    /// Add a type descriptor (or return existing index) for dyn type info.
    /// If the tag_name already exists but field_names/field_type_tags/aux_type_tags are more complete, update them.
    fn add_type_descriptor(
        &mut self,
        tag_name: &str,
        field_names: &[String],
        field_type_tags: &[String],
        aux_type_tags: &[String],
    ) -> usize {
        if let Some(&idx) = self.type_descriptor_indices.get(tag_name) {
            // Update field_names if the existing entry has none but new request has some
            if !field_names.is_empty() && self.type_descriptors[idx].field_names.is_empty() {
                self.type_descriptors[idx].field_names = field_names.to_vec();
            }
            // Update field_type_tags if the existing entry has none but new request has some
            if !field_type_tags.is_empty() && self.type_descriptors[idx].field_type_tags.is_empty()
            {
                self.type_descriptors[idx].field_type_tags = field_type_tags.to_vec();
            }
            // Update aux_type_tags if the existing entry has none but new request has some
            if !aux_type_tags.is_empty() && self.type_descriptors[idx].aux_type_tags.is_empty() {
                self.type_descriptors[idx].aux_type_tags = aux_type_tags.to_vec();
            }
            return idx;
        }
        let idx = self.type_descriptors.len();
        // Ensure tag_name is in string pool (needed for type_info heap object)
        self.add_string(tag_name.to_string());
        self.type_descriptors.push(crate::vm::TypeDescriptor {
            tag_name: tag_name.to_string(),
            field_names: field_names.to_vec(),
            field_type_tags: field_type_tags.to_vec(),
            aux_type_tags: aux_type_tags.to_vec(),
            vtables: vec![],
        });
        self.type_descriptor_indices
            .insert(tag_name.to_string(), idx);
        idx
    }

    /// Add an interface descriptor (or return existing index).
    fn add_interface_descriptor(&mut self, name: &str) -> usize {
        if let Some(&idx) = self.interface_descriptor_indices.get(name) {
            return idx;
        }
        let idx = self.interface_descriptors.len();
        let method_names = self
            .interface_method_names
            .get(name)
            .cloned()
            .unwrap_or_default();
        self.interface_descriptors
            .push(crate::vm::InterfaceDescriptor {
                name: name.to_string(),
                method_names,
            });
        self.interface_descriptor_indices
            .insert(name.to_string(), idx);
        idx
    }

    pub fn compile(&mut self, program: ResolvedProgram) -> Result<Chunk, String> {
        // Store interface method names for interface descriptor registration
        self.interface_method_names = program.interface_methods;

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

        // Fixup pass: resolve interface descriptor GlobalGet indices.
        // During codegen, interface descriptor loads are emitted as GlobalGet(iface_idx | IFACE_GLOBAL_FLAG).
        // Now that type_descriptors is finalized, add the offset to get the final global index.
        let td_count = self.type_descriptors.len();
        for func in &mut self.functions {
            for op in &mut func.code {
                if let Op::GlobalGet(idx) = op
                    && *idx & Self::IFACE_GLOBAL_FLAG != 0
                {
                    *idx = (*idx & !Self::IFACE_GLOBAL_FLAG) + td_count;
                }
            }
        }
        for op in &mut main_ops {
            if let Op::GlobalGet(idx) = op
                && *idx & Self::IFACE_GLOBAL_FLAG != 0
            {
                *idx = (*idx & !Self::IFACE_GLOBAL_FLAG) + td_count;
            }
        }

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
            type_descriptors: self.type_descriptors.clone(),
            interface_descriptors: self.interface_descriptors.clone(),
            debug,
        })
    }

    /// Determine the collection element ElemKind from a struct's `data` field type.
    /// For structs like `vec_int` with `data: ptr<int>`, returns `Some(ElemKind::I64)`.
    fn collection_elem_kind_from_struct(&self, struct_name: &str) -> Option<ElemKind> {
        let field_types = self.struct_field_type_map.get(struct_name)?;
        for (name, ty) in field_types {
            if name == "data"
                && let Type::Ptr(inner) = ty
            {
                let ek = elem_kind_for_element_type(inner);
                if ek != ElemKind::Tagged {
                    return Some(ek);
                }
            }
        }
        None
    }

    fn compile_function(&mut self, func: &ResolvedFunction) -> Result<Function, String> {
        self.current_local_full_types = func.local_types.clone();

        // Detect collection methods (e.g., vec_int::push) and set ElemKind
        // for __alloc_heap calls within them.
        self.current_collection_elem_kind = func
            .name
            .split_once("::")
            .and_then(|(struct_name, _)| self.collection_elem_kind_from_struct(struct_name));

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
                    .map(|t| t.is_array() || t.is_vec() || matches!(t, Type::String))
                    .unwrap_or(false);

                if has_ptr_layout {
                    // Ptr-based layout: indirect store via ptr field (slot 0)
                    // HeapStore2 = heap[heap[ref][0]][idx] = val in one op
                    let ek = elem_kind_for_collection(object_type);
                    self.compile_expr(object, ops)?;
                    self.compile_expr(index, ops)?;
                    self.compile_expr(value, ops)?;
                    ops.push(Op::HeapStore2(ek));
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
                struct_name,
            } => {
                // Check if this might be a struct field (structs are compiled as arrays)
                if let Some(idx) = self.get_field_index(field, struct_name.as_deref()) {
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

                // Compile as if-else chain on type tags using pointer comparison
                let mut jump_to_end_patches = Vec::new();

                for arm in arms {
                    let jump_to_next;

                    match &arm.kind {
                        MatchDynArmKind::TypeMatch { type_tag_name } => {
                            // Register type descriptor for this arm's type
                            let td_idx = self.add_type_descriptor(type_tag_name, &[], &[], &[]);

                            // Load dyn value → type_info ref, compare with expected type descriptor
                            ops.push(Op::LocalGet(*dyn_slot));
                            ops.push(Op::HeapLoad(0)); // dyn slot 0 → type_info ref
                            ops.push(Op::GlobalGet(td_idx)); // expected type descriptor ref
                            ops.push(Op::RefEq); // pointer comparison (O(1) via identity check)

                            // Branch to next arm if tag doesn't match
                            jump_to_next = ops.len();
                            ops.push(Op::BrIfFalse(0)); // placeholder

                            // Unbox the value via HeapLoad(1) and bind to the arm's variable
                            ops.push(Op::LocalGet(*dyn_slot));
                            ops.push(Op::HeapLoad(1));
                            ops.push(Op::LocalSet(arm.var_slot));
                        }
                        MatchDynArmKind::InterfaceMatch {
                            interface_name,
                            vtable_slot,
                        } => {
                            // Look up vtable for the interface on this dyn value's type
                            let iface_idx = self.add_interface_descriptor(interface_name);

                            // Load dyn value → type_info ref
                            ops.push(Op::LocalGet(*dyn_slot));
                            ops.push(Op::HeapLoad(0)); // type_info ref
                            // Load interface descriptor ref
                            ops.push(Op::GlobalGet(iface_idx | Self::IFACE_GLOBAL_FLAG));
                            // VtableLookup: pops (type_info, iface_desc), pushes vtable_ref or null
                            ops.push(Op::VtableLookup);

                            // Store vtable result for potential use in arm body
                            ops.push(Op::LocalSet(*vtable_slot + self.local_offset));

                            // Check if vtable was found (non-null)
                            ops.push(Op::LocalGet(*vtable_slot + self.local_offset));
                            ops.push(Op::RefIsNull);

                            // Branch to next arm if vtable is null (interface not implemented)
                            jump_to_next = ops.len();
                            ops.push(Op::BrIf(0)); // placeholder (branch if IS null)

                            // Unbox the raw value and bind to the arm's variable
                            ops.push(Op::LocalGet(*dyn_slot));
                            ops.push(Op::HeapLoad(1)); // raw value from dyn slot 1
                            ops.push(Op::LocalSet(arm.var_slot));
                        }
                    }

                    // Compile arm body
                    for stmt in &arm.body {
                        self.compile_statement(stmt, ops)?;
                    }

                    // Jump to end of match
                    jump_to_end_patches.push(ops.len());
                    ops.push(Op::Jmp(0)); // placeholder

                    // Patch jump to next arm
                    let next_arm = ops.len();
                    ops[jump_to_next] = match ops[jump_to_next] {
                        Op::BrIfFalse(_) => Op::BrIfFalse(next_arm),
                        Op::BrIf(_) => Op::BrIf(next_arm),
                        _ => unreachable!(),
                    };
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
                ops.push(Op::HeapAlloc(2)); // Array struct with [ptr, len]
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
                    .map(|t| t.is_array() || t.is_vec() || matches!(t, Type::String))
                    .unwrap_or(false);

                if has_ptr_layout {
                    // Ptr-based layout: indirect access via ptr field (slot 0)
                    // HeapLoad2 = heap[heap[ref][0]][idx] in one op
                    let ek = elem_kind_for_collection(object_type);
                    self.compile_expr(object, ops)?;
                    self.compile_expr(index, ops)?;
                    ops.push(Op::HeapLoad2(ek));
                } else {
                    // Struct/string access: direct HeapLoadDyn
                    self.compile_expr(object, ops)?;
                    self.compile_expr(index, ops)?;
                    ops.push(Op::HeapLoadDyn);
                }
            }
            ResolvedExpr::Field {
                object,
                field,
                struct_name,
            } => {
                self.compile_expr(object, ops)?;
                // Check if this might be a struct field (structs are compiled as arrays)
                if let Some(idx) = self.get_field_index(field, struct_name.as_deref()) {
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
            ResolvedExpr::Binary {
                op,
                left,
                right,
                operand_type: _,
            } => {
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
                        ValueType::Ref => {
                            ops.push(Op::RefEq);
                        }
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
                    "__typeof" => {
                        if args.len() != 1 {
                            return Err("__typeof takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::TypeOf);
                    }
                    "__heap_size" => {
                        if args.len() != 1 {
                            return Err("__heap_size takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        ops.push(Op::HeapSize);
                    }
                    "__hostcall" => {
                        // __hostcall(num, ...args) -> result
                        // First argument must be a compile-time constant (hostcall number)
                        if args.is_empty() {
                            return Err(
                                "__hostcall requires at least 1 argument (hostcall number)"
                                    .to_string(),
                            );
                        }
                        // Extract hostcall number from first argument (must be integer literal)
                        let hostcall_num = match &args[0] {
                            ResolvedExpr::Int(n) => *n as usize,
                            _ => {
                                return Err("__hostcall first argument must be an integer literal"
                                    .to_string());
                            }
                        };
                        // Compile remaining arguments (hostcall-specific args)
                        for arg in args.iter().skip(1) {
                            self.compile_expr(arg, ops)?;
                        }
                        // argc is the number of hostcall-specific arguments (excluding hostcall number)
                        let argc = args.len() - 1;
                        ops.push(Op::Hostcall(hostcall_num, argc));
                    }
                    "len" => {
                        if args.len() != 1 {
                            return Err("len takes exactly 1 argument".to_string());
                        }
                        self.compile_expr(&args[0], ops)?;
                        // Both Array<T> and String have [ptr, len] layout
                        ops.push(Op::HeapLoad(1));
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
                        // Always allocate Tagged for now: the JIT always uses Tagged stride
                        // for HeapLoad2/HeapStore2, so typed allocation would cause a
                        // stride mismatch. Typed allocation requires fixing the
                        // monomorphise pass first (see #241).
                        ops.push(Op::HeapAllocDynSimple(ElemKind::Tagged));
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
                        ops.push(Op::HeapAlloc(2)); // String struct with [ptr, len]
                    }
                    "__call_func" => {
                        // __call_func(func_idx, arg) -> result
                        // Calls function by dynamic index with one argument
                        if args.len() != 2 {
                            return Err(
                                "__call_func takes exactly 2 arguments (func_idx, arg)".to_string()
                            );
                        }
                        self.compile_expr(&args[0], ops)?; // func_idx
                        self.compile_expr(&args[1], ops)?; // arg
                        ops.push(Op::CallDynamic(1)); // 1 argument
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
            ResolvedExpr::AsDyn {
                expr,
                type_tag_name,
                field_names,
                field_type_tags,
                aux_type_tags,
                nested_type_descriptors,
                vtable_entries,
            } => {
                // Register nested type descriptors first (with full field info)
                // so that subsequent registrations with empty info don't overwrite them.
                for (ntd_tag, ntd_fields, ntd_field_types, ntd_aux) in
                    nested_type_descriptors.iter()
                {
                    self.add_type_descriptor(ntd_tag, ntd_fields, ntd_field_types, ntd_aux);
                }
                // Register type descriptor (pre-allocated at VM startup)
                let td_idx = self.add_type_descriptor(
                    type_tag_name,
                    field_names,
                    field_type_tags,
                    aux_type_tags,
                );

                // Register interface descriptors and build vtable entries
                if !vtable_entries.is_empty() {
                    let mut vtables = Vec::new();
                    for (iface_name, func_indices) in vtable_entries {
                        let iface_idx = self.add_interface_descriptor(iface_name);
                        vtables.push((iface_idx, func_indices.clone()));
                    }
                    // Merge vtables into the TypeDescriptor
                    if self.type_descriptors[td_idx].vtables.is_empty() {
                        self.type_descriptors[td_idx].vtables = vtables;
                    }
                }

                // Push type_info ref (pre-allocated) and value, then alloc dyn object inline
                ops.push(Op::GlobalGet(td_idx));
                self.compile_expr(expr, ops)?;
                // HeapAlloc(2) pops [type_info_ref, value] → dyn object [slot0=type_info, slot1=value]
                ops.push(Op::HeapAlloc(2));
            }
            ResolvedExpr::VtableMethodCall {
                object,
                vtable_slot,
                method_index,
                args,
            } => {
                // Dynamic dispatch via vtable:
                // 1. Load func_index from vtable
                ops.push(Op::LocalGet(*vtable_slot + self.local_offset));
                ops.push(Op::HeapLoad(*method_index));
                // 2. Push self (raw value, already unboxed by InterfaceMatch arm)
                self.compile_expr(object, ops)?;
                // 3. Push explicit arguments
                for arg in args {
                    self.compile_expr(arg, ops)?;
                }
                // 4. CallDynamic: pops argc args then func_index, calls function
                ops.push(Op::CallDynamic(args.len() + 1)); // self + explicit args
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
            // HeapAllocArray removed — use HeapAlloc instead
            "HeapAllocDyn" | "AllocHeapDyn" => Ok(Op::HeapAllocDyn),
            "HeapAllocDynSimple" => Ok(Op::HeapAllocDynSimple(ElemKind::Tagged)),
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
            // Exception handling
            "Throw" => Ok(Op::Throw),
            "TryBegin" => {
                let target = self.expect_int_arg(args, 0, "TryBegin")? as usize;
                Ok(Op::TryBegin(target))
            }
            "TryEnd" => Ok(Op::TryEnd),

            // Builtins
            "TypeOf" => Ok(Op::TypeOf),
            "HeapSize" => Ok(Op::HeapSize),

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

            // Hostcall
            "Hostcall" => {
                let num = self.expect_int_arg(args, 0, "Hostcall")? as usize;
                let argc = self.expect_int_arg(args, 1, "Hostcall")? as usize;
                Ok(Op::Hostcall(num, argc))
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
    fn test_simple_typeof() {
        let chunk = compile("__typeof(42);").unwrap();
        assert!(chunk.main.code.contains(&Op::I64Const(42)));
        assert!(chunk.main.code.contains(&Op::TypeOf));
    }

    #[test]
    fn test_arithmetic() {
        let chunk = compile("__typeof(1 + 2);").unwrap();
        assert!(chunk.main.code.contains(&Op::I64Add));
    }

    #[test]
    fn test_function_call() {
        let chunk = compile("fun foo() { return 42; } __typeof(foo());").unwrap();
        assert_eq!(chunk.functions.len(), 1);
        assert_eq!(chunk.functions[0].name, "foo");
    }
}
