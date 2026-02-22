use crate::compiler::ast::*;
use crate::compiler::lexer::Span;
use crate::compiler::types::{Type, TypeAnnotation};
use std::collections::{HashMap, HashSet};

/// A type descriptor entry: (tag_name, field_names, field_type_tags, aux_type_tags).
type TypeDescriptorEntry = (String, Vec<String>, Vec<String>, Vec<String>);

/// A resolved asm instruction.
#[derive(Debug, Clone)]
pub enum ResolvedAsmInstruction {
    Emit { op_name: String, args: Vec<AsmArg> },
    Safepoint,
    GcHint { size: i64 },
}

/// Resolved program with variable indices and function references.
#[derive(Debug, Clone)]
pub struct ResolvedProgram {
    pub functions: Vec<ResolvedFunction>,
    pub main_body: Vec<ResolvedStatement>,
    pub structs: Vec<ResolvedStruct>,
    /// Number of local variables in the main body
    pub main_locals_count: usize,
    /// Type information for main body local variables (indexed by slot)
    pub main_local_types: Vec<Type>,
    /// Interface definitions: interface_name -> method_names (sorted)
    pub interface_methods: HashMap<String, Vec<String>>,
}

/// Information about a resolved struct.
#[derive(Debug, Clone)]
pub struct ResolvedStruct {
    pub name: String,
    /// Field names in declaration order
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedFunction {
    pub name: String,
    pub params: Vec<String>,
    pub locals_count: usize,
    pub body: Vec<ResolvedStatement>,
    /// Type information for local variables (indexed by slot number)
    pub local_types: Vec<Type>,
    /// Whether this function is marked with @inline
    pub is_inline: bool,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ResolvedStatement {
    Let {
        slot: usize,
        init: ResolvedExpr,
    },
    Assign {
        slot: usize,
        value: ResolvedExpr,
    },
    IndexAssign {
        object: ResolvedExpr,
        index: ResolvedExpr,
        value: ResolvedExpr,
        span: Span,
        /// Type of the object (from typechecker, for codegen)
        object_type: Option<Type>,
    },
    FieldAssign {
        object: ResolvedExpr,
        field: String,
        value: ResolvedExpr,
    },
    If {
        condition: ResolvedExpr,
        then_block: Vec<ResolvedStatement>,
        else_block: Option<Vec<ResolvedStatement>>,
    },
    While {
        condition: ResolvedExpr,
        body: Vec<ResolvedStatement>,
    },
    ForIn {
        slot: usize,
        iterable: ResolvedExpr,
        body: Vec<ResolvedStatement>,
    },
    Return {
        value: Option<ResolvedExpr>,
    },
    Throw {
        value: ResolvedExpr,
    },
    Try {
        try_block: Vec<ResolvedStatement>,
        catch_slot: usize,
        catch_block: Vec<ResolvedStatement>,
    },
    Expr {
        expr: ResolvedExpr,
    },
    /// Store to a promoted var variable through its RefCell (outer scope).
    /// Compiles to: LocalGet(slot) + compile(value) + HeapStore(0)
    RefCellStore {
        slot: usize,
        value: ResolvedExpr,
    },
    /// Match dyn statement: runtime type dispatch on a dyn value.
    MatchDyn {
        dyn_slot: usize,
        expr: ResolvedExpr,
        arms: Vec<ResolvedMatchDynArm>,
        default_block: Vec<ResolvedStatement>,
    },
}

/// An arm in a resolved match dyn statement.
#[derive(Debug, Clone)]
pub struct ResolvedMatchDynArm {
    pub var_slot: usize,
    pub kind: MatchDynArmKind,
    pub body: Vec<ResolvedStatement>,
}

/// Kind of match dyn arm: concrete type match or interface match.
#[derive(Debug, Clone)]
pub enum MatchDynArmKind {
    /// Match by concrete type tag name (e.g., "int", "Point", "Vec_int").
    TypeMatch { type_tag_name: String },
    /// Match by interface vtable presence.
    InterfaceMatch {
        interface_name: String,
        vtable_slot: usize,
    },
}

/// Information about a captured variable in a closure.
#[derive(Debug, Clone)]
pub struct CaptureInfo {
    /// Slot index in the outer scope
    pub outer_slot: usize,
    /// Whether the captured variable is `var` (mutable) — uses reference capture via RefCell
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub enum ResolvedExpr {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Nil,
    Local(usize),
    Array {
        elements: Vec<ResolvedExpr>,
    },
    Index {
        object: Box<ResolvedExpr>,
        index: Box<ResolvedExpr>,
        span: Span,
        /// Type of the object (from typechecker, for codegen)
        object_type: Option<Type>,
    },
    Field {
        object: Box<ResolvedExpr>,
        field: String,
    },
    Unary {
        op: UnaryOp,
        operand: Box<ResolvedExpr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<ResolvedExpr>,
        right: Box<ResolvedExpr>,
        /// Type of the left operand (from typechecker, for codegen to emit type-specific ops)
        operand_type: Option<Type>,
    },
    Call {
        func_index: usize,
        args: Vec<ResolvedExpr>,
    },
    Builtin {
        name: String,
        args: Vec<ResolvedExpr>,
        span: Span,
    },
    /// Spawn a thread with a specific function
    SpawnFunc {
        func_index: usize,
    },
    /// Struct literal: `Point { x: 1, y: 2 }`
    /// Fields are resolved to expressions in declaration order (struct field order).
    StructLiteral {
        struct_index: usize,
        /// Field values in declaration order (not named anymore)
        fields: Vec<ResolvedExpr>,
    },
    /// Method call: `obj.method(args)`
    /// Statically dispatched to the resolved function.
    MethodCall {
        object: Box<ResolvedExpr>,
        method: String,
        func_index: usize,
        args: Vec<ResolvedExpr>,
        /// If the method returns a struct, the struct name
        return_struct_name: Option<String>,
    },
    /// Associated function call: `Type::func(args)`
    /// Statically dispatched to the resolved function.
    AssociatedFunctionCall {
        func_index: usize,
        args: Vec<ResolvedExpr>,
        /// If the function returns a struct, the struct name
        return_struct_name: Option<String>,
    },
    /// Inline assembly block.
    AsmBlock {
        /// Resolved input variable slots.
        input_slots: Vec<usize>,
        /// Output type name (for validation).
        output_type: Option<String>,
        /// Resolved asm instructions.
        body: Vec<ResolvedAsmInstruction>,
    },
    /// New literal: `new Vec<int> {1, 2, 3}` or `new Map<string, int> {"a": 1}`
    NewLiteral {
        type_name: String,
        type_args: Vec<crate::compiler::types::TypeAnnotation>,
        elements: Vec<ResolvedNewLiteralElement>,
    },
    /// Block expression: `{ stmt1; stmt2; expr }` - evaluates to the final expression.
    Block {
        statements: Vec<ResolvedStatement>,
        expr: Box<ResolvedExpr>,
    },
    /// Create a closure: captures local variables and associates with a lifted function.
    /// The closure is a heap object: [func_index, captured_val_or_ref_0, ...]
    Closure {
        /// Index of the lifted function in the function table
        func_index: usize,
        /// Captured variables with mutability information.
        /// For `let` variables: value is copied into the closure object.
        /// For `var` variables: RefCell reference is shared with the closure object.
        captures: Vec<CaptureInfo>,
    },
    /// Call a function value indirectly (closure, function pointer, etc.).
    /// The callee is an expression that evaluates to a callable reference.
    CallIndirect {
        callee: Box<ResolvedExpr>,
        args: Vec<ResolvedExpr>,
    },
    /// Load a captured variable from the closure reference (local slot 0).
    /// When is_ref is false: LocalGet(0) + HeapLoad(offset) (copy capture, let variable)
    /// When is_ref is true:  LocalGet(0) + HeapLoad(offset) + HeapLoad(0) (ref capture via RefCell, var variable)
    CaptureLoad {
        offset: usize,
        is_ref: bool,
    },
    /// Store to a captured var variable through its RefCell.
    /// Compiles to: LocalGet(0) + HeapLoad(offset) + compile(value) + HeapStore(0)
    CaptureStore {
        offset: usize,
        value: Box<ResolvedExpr>,
    },
    /// Create a new RefCell wrapping a value. Used for var variables that are captured by closures.
    /// Compiles to: compile(value) + HeapAlloc(1)
    RefCellNew {
        value: Box<ResolvedExpr>,
    },
    /// Load a value from a RefCell (for promoted var variables in outer scope).
    /// Compiles to: LocalGet(slot) + HeapLoad(0)
    RefCellLoad {
        slot: usize,
    },
    /// As dyn expression: boxes a value with a runtime type tag.
    AsDyn {
        expr: Box<ResolvedExpr>,
        type_tag_name: String,
        field_names: Vec<String>,
        field_type_tags: Vec<String>,
        /// Auxiliary type tags for container element types.
        /// Vec/Array → [elem_tag], Map → [key_tag, val_tag], others → [].
        aux_type_tags: Vec<String>,
        /// Recursively collected type descriptors for nested types.
        nested_type_descriptors: Vec<TypeDescriptorEntry>,
        /// Interface vtable entries: Vec<(interface_name, Vec<func_index>)>.
        /// Method indices correspond to the sorted method names of each interface.
        vtable_entries: Vec<(String, Vec<usize>)>,
    },
}

/// An element in a resolved new literal.
#[derive(Debug, Clone)]
pub enum ResolvedNewLiteralElement {
    /// Simple value: `1`, `"foo"` etc.
    Value(ResolvedExpr),
    /// Key-value pair: `"a": 1`, `key: value`
    KeyValue {
        key: ResolvedExpr,
        value: ResolvedExpr,
    },
}

/// Information about a struct during resolution.
#[derive(Debug, Clone)]
struct StructDefInfo {
    index: usize,
    /// Field names in declaration order
    fields: Vec<String>,
    /// Methods: method_name -> function_index
    methods: HashMap<String, usize>,
    /// Method return types: method_name -> struct_name (if returns a struct)
    method_return_types: HashMap<String, Option<String>>,
}

/// The resolver performs name resolution and variable slot assignment.
pub struct Resolver<'a> {
    filename: &'a str,
    functions: HashMap<String, usize>,
    builtins: Vec<String>,
    /// Struct definitions: struct_name -> info
    structs: HashMap<String, StructDefInfo>,
    /// Primitive type methods: type_name -> (method_name -> func_index)
    primitive_methods: HashMap<String, HashMap<String, usize>>,
    /// Resolved struct list (for output)
    resolved_structs: Vec<ResolvedStruct>,
    /// Lambda functions lifted during resolution
    lifted_functions: Vec<ResolvedFunction>,
    /// Counter for generating unique lambda function names
    next_lambda_id: usize,
    /// Total number of functions registered before lambda lifting
    /// (used to compute correct func_index for lifted lambdas)
    base_func_count: usize,
    /// Interface implementations: (interface_name, type_name) set
    interface_impls: HashSet<(String, String)>,
    /// Interface definitions: interface_name -> method_names (sorted)
    interface_methods: HashMap<String, Vec<String>>,
}

impl<'a> Resolver<'a> {
    pub fn new(filename: &'a str) -> Self {
        Self {
            filename,
            functions: HashMap::new(),
            interface_impls: HashSet::new(),
            interface_methods: HashMap::new(),
            builtins: vec![
                "__value_to_string".to_string(),
                "len".to_string(),
                "type_of".to_string(),
                "parse_int".to_string(),
                // Thread operations
                "spawn".to_string(),
                "channel".to_string(),
                "send".to_string(),
                "recv".to_string(),
                "join".to_string(),
                // Syscall operations (generic syscall builtin)
                "__syscall".to_string(),
                // Low-level heap intrinsics (for stdlib implementation)
                "__heap_load".to_string(),
                "__heap_store".to_string(),
                "__alloc_heap".to_string(),
                "__alloc_string".to_string(),
                "__null_ptr".to_string(),
                "__ptr_offset".to_string(),
                // 128-bit multiply high
                "__umul128_hi".to_string(),
                // Dynamic call by function index
                "__call_func".to_string(),
                // CLI argument operations
                "argc".to_string(),
                "argv".to_string(),
                "args".to_string(),
            ],
            structs: HashMap::new(),
            primitive_methods: HashMap::new(),
            resolved_structs: Vec::new(),
            lifted_functions: Vec::new(),
            next_lambda_id: 0,
            base_func_count: 0,
        }
    }

    /// Compute vtable entries for a type's interface implementations.
    /// Returns Vec<(interface_name, Vec<func_index>)> where func_indices
    /// correspond to the sorted method names of each interface.
    fn compute_vtable_entries(&self, ty: &Type) -> Vec<(String, Vec<usize>)> {
        let impl_name = type_to_impl_name(ty);
        let mut entries = Vec::new();

        // Sort interface names for deterministic ordering
        let mut iface_names: Vec<&String> = self.interface_methods.keys().collect();
        iface_names.sort();

        for iface_name in iface_names {
            if self
                .interface_impls
                .contains(&(iface_name.clone(), impl_name.clone()))
                && let Some(method_names) = self.interface_methods.get(iface_name)
            {
                let mut func_indices = Vec::new();
                for method_name in method_names {
                    // Look up func_index for Type::method_name
                    let qualified_name = format!("{}::{}", impl_name, method_name);
                    if let Some(&func_idx) = self.functions.get(&qualified_name) {
                        func_indices.push(func_idx);
                    }
                }
                if func_indices.len() == method_names.len() {
                    entries.push((iface_name.clone(), func_indices));
                }
            }
        }

        entries
    }

    /// Set interface implementation data from the type checker.
    pub fn set_interface_info(
        &mut self,
        impls: HashSet<(String, String)>,
        methods: HashMap<String, Vec<String>>,
    ) {
        self.interface_impls = impls;
        self.interface_methods = methods;
    }

    /// Collect variable types from AST Statement::Let.inferred_type fields.
    fn collect_var_types(stmts: &[Statement]) -> HashMap<String, Type> {
        let mut type_map = HashMap::new();
        Self::collect_var_types_inner(stmts, &mut type_map);
        type_map
    }

    fn collect_var_types_inner(stmts: &[Statement], type_map: &mut HashMap<String, Type>) {
        for stmt in stmts {
            match stmt {
                Statement::Let {
                    name,
                    inferred_type: Some(ty),
                    ..
                } => {
                    type_map.insert(name.clone(), ty.clone());
                }
                Statement::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    Self::collect_var_types_inner(&then_block.statements, type_map);
                    if let Some(else_block) = else_block {
                        Self::collect_var_types_inner(&else_block.statements, type_map);
                    }
                }
                Statement::While { body, .. } | Statement::ForIn { body, .. } => {
                    Self::collect_var_types_inner(&body.statements, type_map);
                }
                Statement::Try {
                    try_block,
                    catch_block,
                    ..
                } => {
                    Self::collect_var_types_inner(&try_block.statements, type_map);
                    Self::collect_var_types_inner(&catch_block.statements, type_map);
                }
                Statement::MatchDyn {
                    arms,
                    default_block,
                    ..
                } => {
                    for arm in arms {
                        Self::collect_var_types_inner(&arm.body.statements, type_map);
                    }
                    Self::collect_var_types_inner(&default_block.statements, type_map);
                }
                _ => {}
            }
        }
    }

    /// Build a Vec<Type> indexed by slot number for a function,
    /// using the slot_names from Scope and the type_map built from AST nodes.
    fn build_local_types(scope: &Scope, type_map: &HashMap<String, Type>) -> Vec<Type> {
        let mut local_types = vec![Type::Any; scope.locals_count];
        for (slot, name) in scope.slot_names.iter().enumerate() {
            if let Some(ty) = type_map.get(name) {
                local_types[slot] = ty.clone();
            }
        }
        local_types
    }

    pub fn resolve(&mut self, program: Program) -> Result<ResolvedProgram, String> {
        // First pass: collect struct definitions
        let mut struct_defs = Vec::new();
        let mut impl_blocks = Vec::new();
        let mut func_defs = Vec::new();
        let mut main_stmts = Vec::new();

        for item in program.items {
            match item {
                Item::Import(_import) => {
                    // Imports are handled in module resolution phase
                }
                Item::StructDef(struct_def) => {
                    struct_defs.push(struct_def);
                }
                Item::ImplBlock(impl_block) => {
                    impl_blocks.push(impl_block);
                }
                Item::FnDef(fn_def) => {
                    func_defs.push(fn_def);
                }
                Item::InterfaceDef(_) => {
                    // Interface definitions are handled elsewhere
                }
                Item::Statement(stmt) => {
                    main_stmts.push(stmt);
                }
            }
        }

        // Register struct definitions
        for struct_def in &struct_defs {
            if self.structs.contains_key(&struct_def.name) {
                return Err(self.error(
                    &format!("struct '{}' already defined", struct_def.name),
                    struct_def.span,
                ));
            }
            let index = self.resolved_structs.len();
            let fields: Vec<String> = struct_def.fields.iter().map(|f| f.name.clone()).collect();
            self.structs.insert(
                struct_def.name.clone(),
                StructDefInfo {
                    index,
                    fields: fields.clone(),
                    methods: HashMap::new(),
                    method_return_types: HashMap::new(),
                },
            );
            self.resolved_structs.push(ResolvedStruct {
                name: struct_def.name.clone(),
                fields,
            });
        }

        // Register top-level functions
        for fn_def in &func_defs {
            let index = func_defs
                .iter()
                .position(|f| f.name == fn_def.name)
                .unwrap();
            // Check for builtin name collision
            if self.builtins.contains(&fn_def.name) {
                return Err(self.error(
                    &format!(
                        "cannot define function '{}': name is reserved for builtin",
                        fn_def.name
                    ),
                    fn_def.span,
                ));
            }
            if self.functions.contains_key(&fn_def.name) {
                return Err(self.error(
                    &format!("function '{}' already defined", fn_def.name),
                    fn_def.span,
                ));
            }
            self.functions.insert(fn_def.name.clone(), index);
        }

        // Register impl block methods as functions
        for impl_block in &impl_blocks {
            let is_builtin_type =
                impl_block.struct_name == "vec" || impl_block.struct_name == "map";
            let is_primitive_type = matches!(
                impl_block.struct_name.as_str(),
                "int" | "float" | "bool" | "string"
            );

            if !is_builtin_type && !is_primitive_type {
                // For struct impls, verify the struct exists
                if !self.structs.contains_key(&impl_block.struct_name) {
                    return Err(self.error(
                        &format!("impl for undefined struct '{}'", impl_block.struct_name),
                        impl_block.span,
                    ));
                }
            }

            for method in &impl_block.methods {
                let has_self = method.params.iter().any(|p| p.name == "self");

                // For associated functions on builtin types, use {type}_{func} naming
                // For methods, use {Type}::{method} naming
                let func_name = if is_builtin_type && !has_self {
                    format!("{}_{}", impl_block.struct_name, method.name)
                } else {
                    format!("{}::{}", impl_block.struct_name, method.name)
                };

                // func_index is the next available index in resolved_functions
                // which will contain func_defs first, then all methods
                let func_index = self.functions.len();

                if self.functions.contains_key(&func_name) {
                    return Err(self.error(
                        &format!(
                            "method '{}' already defined for struct '{}'",
                            method.name, impl_block.struct_name
                        ),
                        method.span,
                    ));
                }
                self.functions.insert(func_name.clone(), func_index);

                // Get return type struct name if method returns a struct
                let return_struct_name = method.return_type.as_ref().and_then(|rt| {
                    if let TypeAnnotation::Named(name) = rt {
                        // Check if this name is a known struct
                        if self.structs.contains_key(name) {
                            Some(name.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });

                if is_primitive_type {
                    // Add method to primitive type's method table
                    self.primitive_methods
                        .entry(impl_block.struct_name.clone())
                        .or_default()
                        .insert(method.name.clone(), func_index);
                } else if let Some(struct_info) = self.structs.get_mut(&impl_block.struct_name) {
                    // Add method to struct's method table
                    struct_info.methods.insert(method.name.clone(), func_index);
                    struct_info
                        .method_return_types
                        .insert(method.name.clone(), return_struct_name);
                }
            }
        }

        // Record total function count before resolution (for lambda lifting indices)
        self.base_func_count = self.functions.len();

        // Second pass: resolve function bodies
        let mut resolved_functions = Vec::new();
        for fn_def in func_defs {
            let resolved = self.resolve_function(fn_def)?;
            resolved_functions.push(resolved);
        }

        // Resolve impl block methods as functions
        for impl_block in impl_blocks {
            for method in impl_block.methods {
                let resolved = self.resolve_method(method, &impl_block.struct_name)?;
                resolved_functions.push(resolved);
            }
        }

        // Resolve main body
        let mut scope = Scope::new();
        let main_type_map = Self::collect_var_types(&main_stmts);
        let resolved_main = self.resolve_statements(main_stmts, &mut scope)?;
        let main_locals_count = scope.locals_count;
        let main_local_types = Self::build_local_types(&scope, &main_type_map);

        // Append lifted lambda functions
        resolved_functions.append(&mut self.lifted_functions);

        Ok(ResolvedProgram {
            functions: resolved_functions,
            main_body: resolved_main,
            structs: self.resolved_structs.clone(),
            main_locals_count,
            main_local_types,
            interface_methods: self.interface_methods.clone(),
        })
    }

    fn resolve_method(
        &mut self,
        method: FnDef,
        struct_name: &str,
    ) -> Result<ResolvedFunction, String> {
        let mut scope = Scope::new();
        let has_self = method.params.iter().any(|p| p.name == "self");
        let is_builtin_type = struct_name == "vec" || struct_name == "map";
        let is_inline = method.attributes.iter().any(|a| a.name == "inline");

        let mut param_names: Vec<String> = Vec::new();

        if has_self {
            // Add 'self' as first parameter with struct type information
            param_names.push("self".to_string());
            scope.declare_with_type("self".to_string(), false, Some(struct_name.to_string()));
        }

        // Add other parameters
        for param in &method.params {
            if param.name != "self" {
                param_names.push(param.name.clone());
                let struct_name_for_param =
                    self.struct_name_from_type_annotation(&param.type_annotation);
                scope.declare_with_type(param.name.clone(), false, struct_name_for_param);
            }
        }

        let mut method_type_map = Self::collect_var_types(&method.body.statements);
        // Add parameter types
        for param in &method.params {
            if param.name != "self"
                && let Some(ty) = self.type_from_annotation(&param.type_annotation)
            {
                method_type_map.insert(param.name.clone(), ty);
            }
        }
        let body = self.resolve_statements(method.body.statements, &mut scope)?;

        // Function name: {Type}::{method} for methods, {type}_{func} for associated functions on builtin types
        let func_name = if is_builtin_type && !has_self {
            format!("{}_{}", struct_name, method.name)
        } else {
            format!("{}::{}", struct_name, method.name)
        };

        // Check for direct recursion in @inline methods
        if is_inline
            && let Some(&func_index) = self.functions.get(&func_name)
            && self.body_calls_function(&body, func_index)
        {
            return Err(self.error(
                &format!("@inline method '{}' cannot be recursive", func_name),
                method.span,
            ));
        }

        let local_types = Self::build_local_types(&scope, &method_type_map);

        Ok(ResolvedFunction {
            name: func_name,
            params: param_names,
            locals_count: scope.locals_count,
            body,
            local_types,
            is_inline,
        })
    }

    fn resolve_function(&mut self, fn_def: FnDef) -> Result<ResolvedFunction, String> {
        let mut scope = Scope::new();
        let is_inline = fn_def.attributes.iter().any(|a| a.name == "inline");

        // Add parameters to scope
        let param_names: Vec<String> = fn_def.params.iter().map(|p| p.name.clone()).collect();
        for param in &fn_def.params {
            let struct_name = self.struct_name_from_type_annotation(&param.type_annotation);
            scope.declare_with_type(param.name.clone(), false, struct_name);
        }

        let mut fn_type_map = Self::collect_var_types(&fn_def.body.statements);
        // Add parameter types so codegen can infer correct ValueTypes
        for param in &fn_def.params {
            if let Some(ty) = self.type_from_annotation(&param.type_annotation) {
                fn_type_map.insert(param.name.clone(), ty);
            }
        }
        let body = self.resolve_statements(fn_def.body.statements, &mut scope)?;
        let local_types = Self::build_local_types(&scope, &fn_type_map);

        // Check for direct recursion in @inline functions
        if is_inline
            && let Some(&func_index) = self.functions.get(&fn_def.name)
            && self.body_calls_function(&body, func_index)
        {
            return Err(self.error(
                &format!("@inline function '{}' cannot be recursive", fn_def.name),
                fn_def.span,
            ));
        }

        Ok(ResolvedFunction {
            name: fn_def.name,
            params: param_names,
            locals_count: scope.locals_count,
            body,
            local_types,
            is_inline,
        })
    }

    /// Convert a type annotation to a Type for use in local_types.
    fn type_from_annotation(
        &self,
        type_annotation: &Option<crate::compiler::types::TypeAnnotation>,
    ) -> Option<Type> {
        use crate::compiler::types::TypeAnnotation;
        match type_annotation {
            Some(TypeAnnotation::Named(name)) => match name.as_str() {
                "int" => Some(Type::Int),
                "float" => Some(Type::Float),
                "bool" => Some(Type::Bool),
                "string" => Some(Type::String),
                "nil" => Some(Type::Nil),
                _ => {
                    if self.structs.contains_key(name) {
                        Some(Type::Struct {
                            name: name.clone(),
                            fields: Vec::new(),
                        })
                    } else {
                        None
                    }
                }
            },
            Some(TypeAnnotation::Array(_)) => Some(Type::Array(Box::new(Type::Any))),
            Some(TypeAnnotation::Vec(_)) => Some(Type::Vector(Box::new(Type::Any))),
            Some(TypeAnnotation::Map(_, _)) => {
                Some(Type::Map(Box::new(Type::Any), Box::new(Type::Any)))
            }
            Some(TypeAnnotation::Nullable(inner)) => {
                let inner_ty = self
                    .type_from_annotation(&Some(*inner.clone()))
                    .unwrap_or(Type::Any);
                Some(Type::Nullable(Box::new(inner_ty)))
            }
            _ => None,
        }
    }

    /// Extract struct name from a type annotation (for function parameters).
    fn struct_name_from_type_annotation(
        &self,
        type_annotation: &Option<crate::compiler::types::TypeAnnotation>,
    ) -> Option<String> {
        match type_annotation {
            Some(crate::compiler::types::TypeAnnotation::Named(type_name)) => {
                if self.structs.contains_key(type_name) {
                    Some(type_name.clone())
                } else {
                    None
                }
            }
            Some(crate::compiler::types::TypeAnnotation::Array(_)) => Some("Array".to_string()),
            Some(crate::compiler::types::TypeAnnotation::Vec(_)) => Some("Vec".to_string()),
            Some(crate::compiler::types::TypeAnnotation::Map(_, _)) => Some("Map".to_string()),
            Some(crate::compiler::types::TypeAnnotation::Generic { name, .. }) => {
                if self.structs.contains_key(name) {
                    Some(name.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get struct name from a ResolvedExpr if it evaluates to a struct
    fn get_struct_name(&self, expr: &ResolvedExpr) -> Option<String> {
        match expr {
            ResolvedExpr::StructLiteral { struct_index, .. } => self
                .resolved_structs
                .get(*struct_index)
                .map(|s| s.name.clone()),
            ResolvedExpr::MethodCall {
                return_struct_name, ..
            } => return_struct_name.clone(),
            // Array literals map to Array<T> struct
            ResolvedExpr::Array { .. } => Some("Array".to_string()),
            _ => None,
        }
    }

    /// First pass: scan a block of statements to find `var` variables that are
    /// captured by any lambda within the block. These variables need to be
    /// promoted to RefCell for reference capture semantics.
    fn find_captured_mutable_vars(stmts: &[Statement]) -> HashSet<String> {
        // Collect all let declarations in this block
        let mut let_names: HashSet<String> = HashSet::new();
        Self::collect_let_decls(stmts, &mut let_names);

        // Collect which variables are reassigned anywhere
        let mut reassigned: HashSet<String> = HashSet::new();
        Self::collect_reassigned_vars(stmts, &mut reassigned);

        // Only consider variables that are both declared and reassigned
        let candidates: HashSet<String> = let_names.intersection(&reassigned).cloned().collect();

        // Find which of these are captured by lambdas
        let mut captured = HashSet::new();
        Self::scan_lambdas_for_captures(stmts, &candidates, &mut captured);
        captured
    }

    /// Collect names of let declarations at the current block level.
    fn collect_let_decls(stmts: &[Statement], let_names: &mut HashSet<String>) {
        for stmt in stmts {
            if let Statement::Let { name, .. } = stmt {
                let_names.insert(name.clone());
            }
        }
    }

    /// Collect names of variables that are reassigned (appear on LHS of assignment).
    fn collect_reassigned_vars(stmts: &[Statement], reassigned: &mut HashSet<String>) {
        for stmt in stmts {
            Self::collect_reassigned_vars_stmt(stmt, reassigned);
        }
    }

    fn collect_reassigned_vars_stmt(stmt: &Statement, reassigned: &mut HashSet<String>) {
        match stmt {
            Statement::Assign { name, .. } => {
                reassigned.insert(name.clone());
            }
            Statement::Let { init, .. } => {
                Self::collect_reassigned_vars_expr(init, reassigned);
            }
            Statement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                Self::collect_reassigned_vars_expr(condition, reassigned);
                Self::collect_reassigned_vars(&then_block.statements, reassigned);
                if let Some(else_b) = else_block {
                    Self::collect_reassigned_vars(&else_b.statements, reassigned);
                }
            }
            Statement::While {
                condition, body, ..
            } => {
                Self::collect_reassigned_vars_expr(condition, reassigned);
                Self::collect_reassigned_vars(&body.statements, reassigned);
            }
            Statement::ForIn { body, .. } => {
                Self::collect_reassigned_vars(&body.statements, reassigned);
            }
            Statement::Try {
                try_block,
                catch_block,
                ..
            } => {
                Self::collect_reassigned_vars(&try_block.statements, reassigned);
                Self::collect_reassigned_vars(&catch_block.statements, reassigned);
            }
            Statement::Expr { expr, .. } => {
                Self::collect_reassigned_vars_expr(expr, reassigned);
            }
            _ => {}
        }
    }

    fn collect_reassigned_vars_expr(expr: &Expr, reassigned: &mut HashSet<String>) {
        match expr {
            Expr::Lambda { body, .. } => {
                // Also scan inside lambda bodies for reassignments to outer vars
                Self::collect_reassigned_vars(&body.statements, reassigned);
            }
            Expr::Binary { left, right, .. } => {
                Self::collect_reassigned_vars_expr(left, reassigned);
                Self::collect_reassigned_vars_expr(right, reassigned);
            }
            Expr::Unary { operand, .. } => {
                Self::collect_reassigned_vars_expr(operand, reassigned);
            }
            Expr::Call { args, .. } => {
                for a in args {
                    Self::collect_reassigned_vars_expr(a, reassigned);
                }
            }
            Expr::CallExpr { callee, args, .. } => {
                Self::collect_reassigned_vars_expr(callee, reassigned);
                for a in args {
                    Self::collect_reassigned_vars_expr(a, reassigned);
                }
            }
            Expr::MethodCall { object, args, .. } => {
                Self::collect_reassigned_vars_expr(object, reassigned);
                for a in args {
                    Self::collect_reassigned_vars_expr(a, reassigned);
                }
            }
            Expr::Array { elements, .. } => {
                for e in elements {
                    Self::collect_reassigned_vars_expr(e, reassigned);
                }
            }
            Expr::Index { object, index, .. } => {
                Self::collect_reassigned_vars_expr(object, reassigned);
                Self::collect_reassigned_vars_expr(index, reassigned);
            }
            Expr::Field { object, .. } => {
                Self::collect_reassigned_vars_expr(object, reassigned);
            }
            _ => {}
        }
    }

    /// Scan statements for lambdas and check if they capture any of the target vars.
    fn scan_lambdas_for_captures(
        stmts: &[Statement],
        var_names: &HashSet<String>,
        captured: &mut HashSet<String>,
    ) {
        for stmt in stmts {
            Self::scan_stmt_for_lambdas(stmt, var_names, captured);
        }
    }

    fn scan_stmt_for_lambdas(
        stmt: &Statement,
        var_names: &HashSet<String>,
        captured: &mut HashSet<String>,
    ) {
        match stmt {
            Statement::Let { init, .. } => {
                Self::scan_expr_for_lambdas(init, var_names, captured);
            }
            Statement::Assign { value, .. } => {
                Self::scan_expr_for_lambdas(value, var_names, captured);
            }
            Statement::IndexAssign {
                object,
                index,
                value,
                ..
            } => {
                Self::scan_expr_for_lambdas(object, var_names, captured);
                Self::scan_expr_for_lambdas(index, var_names, captured);
                Self::scan_expr_for_lambdas(value, var_names, captured);
            }
            Statement::FieldAssign { object, value, .. } => {
                Self::scan_expr_for_lambdas(object, var_names, captured);
                Self::scan_expr_for_lambdas(value, var_names, captured);
            }
            Statement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                Self::scan_expr_for_lambdas(condition, var_names, captured);
                Self::scan_lambdas_for_captures(&then_block.statements, var_names, captured);
                if let Some(else_b) = else_block {
                    Self::scan_lambdas_for_captures(&else_b.statements, var_names, captured);
                }
            }
            Statement::While {
                condition, body, ..
            } => {
                Self::scan_expr_for_lambdas(condition, var_names, captured);
                Self::scan_lambdas_for_captures(&body.statements, var_names, captured);
            }
            Statement::ForIn { iterable, body, .. } => {
                Self::scan_expr_for_lambdas(iterable, var_names, captured);
                Self::scan_lambdas_for_captures(&body.statements, var_names, captured);
            }
            Statement::Return { value, .. } => {
                if let Some(v) = value {
                    Self::scan_expr_for_lambdas(v, var_names, captured);
                }
            }
            Statement::Throw { value, .. } => {
                Self::scan_expr_for_lambdas(value, var_names, captured);
            }
            Statement::Try {
                try_block,
                catch_block,
                ..
            } => {
                Self::scan_lambdas_for_captures(&try_block.statements, var_names, captured);
                Self::scan_lambdas_for_captures(&catch_block.statements, var_names, captured);
            }
            Statement::Expr { expr, .. } => {
                Self::scan_expr_for_lambdas(expr, var_names, captured);
            }
            Statement::ForRange { .. } => {
                unreachable!("ForRange should be desugared before resolution")
            }
            Statement::Const { .. } => {}
            Statement::MatchDyn {
                expr,
                arms,
                default_block,
                ..
            } => {
                Self::scan_expr_for_lambdas(expr, var_names, captured);
                for arm in arms {
                    Self::scan_lambdas_for_captures(&arm.body.statements, var_names, captured);
                }
                Self::scan_lambdas_for_captures(&default_block.statements, var_names, captured);
            }
        }
    }

    fn scan_expr_for_lambdas(
        expr: &Expr,
        var_names: &HashSet<String>,
        captured: &mut HashSet<String>,
    ) {
        match expr {
            Expr::Lambda { params, body, .. } => {
                // This lambda's free vars that are in var_names → captured
                let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                let free_vars = collect_free_vars_block(body, &param_names);
                for fv in &free_vars {
                    if var_names.contains(fv) {
                        captured.insert(fv.clone());
                    }
                }
                // Also scan inside the lambda body for nested lambdas
                Self::scan_lambdas_for_captures(&body.statements, var_names, captured);
            }
            Expr::Array { elements, .. } => {
                for e in elements {
                    Self::scan_expr_for_lambdas(e, var_names, captured);
                }
            }
            Expr::Index { object, index, .. } => {
                Self::scan_expr_for_lambdas(object, var_names, captured);
                Self::scan_expr_for_lambdas(index, var_names, captured);
            }
            Expr::Field { object, .. } => {
                Self::scan_expr_for_lambdas(object, var_names, captured);
            }
            Expr::Unary { operand, .. } => {
                Self::scan_expr_for_lambdas(operand, var_names, captured);
            }
            Expr::Binary { left, right, .. } => {
                Self::scan_expr_for_lambdas(left, var_names, captured);
                Self::scan_expr_for_lambdas(right, var_names, captured);
            }
            Expr::Call { args, .. } => {
                for a in args {
                    Self::scan_expr_for_lambdas(a, var_names, captured);
                }
            }
            Expr::CallExpr { callee, args, .. } => {
                Self::scan_expr_for_lambdas(callee, var_names, captured);
                for a in args {
                    Self::scan_expr_for_lambdas(a, var_names, captured);
                }
            }
            Expr::MethodCall { object, args, .. } => {
                Self::scan_expr_for_lambdas(object, var_names, captured);
                for a in args {
                    Self::scan_expr_for_lambdas(a, var_names, captured);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, e) in fields {
                    Self::scan_expr_for_lambdas(e, var_names, captured);
                }
            }
            Expr::AssociatedFunctionCall { args, .. } => {
                for a in args {
                    Self::scan_expr_for_lambdas(a, var_names, captured);
                }
            }
            Expr::NewLiteral { elements, .. } => {
                for e in elements {
                    match e {
                        NewLiteralElement::Value(v) => {
                            Self::scan_expr_for_lambdas(v, var_names, captured)
                        }
                        NewLiteralElement::KeyValue { key, value } => {
                            Self::scan_expr_for_lambdas(key, var_names, captured);
                            Self::scan_expr_for_lambdas(value, var_names, captured);
                        }
                    }
                }
            }
            Expr::Block {
                statements, expr, ..
            } => {
                Self::scan_lambdas_for_captures(statements, var_names, captured);
                Self::scan_expr_for_lambdas(expr, var_names, captured);
            }
            Expr::AsDyn { expr, .. } => {
                Self::scan_expr_for_lambdas(expr, var_names, captured);
            }
            _ => {}
        }
    }

    fn resolve_statements(
        &mut self,
        statements: Vec<Statement>,
        scope: &mut Scope,
    ) -> Result<Vec<ResolvedStatement>, String> {
        // First pass: find var variables that are captured by lambdas
        let promoted = Self::find_captured_mutable_vars(&statements);
        scope.promoted_vars.extend(promoted);

        let mut resolved = Vec::new();

        for stmt in statements {
            resolved.push(self.resolve_statement(stmt, scope)?);
        }

        Ok(resolved)
    }

    fn resolve_statement(
        &mut self,
        stmt: Statement,
        scope: &mut Scope,
    ) -> Result<ResolvedStatement, String> {
        match stmt {
            Statement::Let {
                name,
                type_annotation,
                init,
                span: _,
                ..
            } => {
                let init = self.resolve_expr(init, scope)?;
                // First try to get struct name from type annotation
                let struct_name = match type_annotation {
                    Some(crate::compiler::types::TypeAnnotation::Named(type_name)) => {
                        if self.structs.contains_key(&type_name) {
                            Some(type_name)
                        } else {
                            self.get_struct_name(&init)
                        }
                    }
                    // array<T> maps to Array<T> generic struct
                    Some(crate::compiler::types::TypeAnnotation::Array(_)) => {
                        Some("Array".to_string())
                    }
                    // vec<T> maps to Vec<T> generic struct
                    Some(crate::compiler::types::TypeAnnotation::Vec(_)) => Some("Vec".to_string()),
                    // map<K, V> maps to Map<K, V> generic struct
                    Some(crate::compiler::types::TypeAnnotation::Map(_, _)) => {
                        Some("Map".to_string())
                    }
                    // Generic type annotation: Vec<int>, Map<string, int>, etc.
                    Some(crate::compiler::types::TypeAnnotation::Generic { name, .. }) => {
                        if self.structs.contains_key(&name) {
                            Some(name)
                        } else {
                            self.get_struct_name(&init)
                        }
                    }
                    _ => self.get_struct_name(&init),
                };
                // If this let shadows a const, remove the inline value
                scope.const_values.remove(&name);
                scope.const_names.remove(&name);
                // All let variables are mutable (reassignable)
                let slot = scope.declare_with_type(name.clone(), true, struct_name);
                // If this var is promoted to RefCell, wrap the init value
                if scope.promoted_vars.contains(&name) {
                    Ok(ResolvedStatement::Let {
                        slot,
                        init: ResolvedExpr::RefCellNew {
                            value: Box::new(init),
                        },
                    })
                } else {
                    Ok(ResolvedStatement::Let { slot, init })
                }
            }
            Statement::Assign { name, value, span } => {
                // Check if trying to reassign a const
                if scope.const_names.contains(&name) {
                    return Err(self.error(&format!("cannot assign to constant '{}'", name), span));
                }

                // Check if this is a captured variable in a closure scope
                if let Some(offset) = scope.lookup_capture(&name)
                    && scope.capture_mutable.contains(&name)
                {
                    let value = self.resolve_expr(value, scope)?;
                    return Ok(ResolvedStatement::Expr {
                        expr: ResolvedExpr::CaptureStore {
                            offset,
                            value: Box::new(value),
                        },
                    });
                }

                let (slot, _mutable) = scope
                    .lookup(&name)
                    .ok_or_else(|| self.error(&format!("undefined variable '{}'", name), span))?;

                let value = self.resolve_expr(value, scope)?;
                // If this var is promoted to RefCell, use RefCellStore
                if scope.promoted_vars.contains(&name) {
                    Ok(ResolvedStatement::RefCellStore { slot, value })
                } else {
                    Ok(ResolvedStatement::Assign { slot, value })
                }
            }
            Statement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let condition = self.resolve_expr(condition, scope)?;

                scope.enter_scope();
                let then_resolved = self.resolve_statements(then_block.statements, scope)?;
                scope.exit_scope();

                let else_resolved = if let Some(else_block) = else_block {
                    scope.enter_scope();
                    let resolved = self.resolve_statements(else_block.statements, scope)?;
                    scope.exit_scope();
                    Some(resolved)
                } else {
                    None
                };

                Ok(ResolvedStatement::If {
                    condition,
                    then_block: then_resolved,
                    else_block: else_resolved,
                })
            }
            Statement::While {
                condition, body, ..
            } => {
                let condition = self.resolve_expr(condition, scope)?;

                scope.enter_scope();
                let body_resolved = self.resolve_statements(body.statements, scope)?;
                scope.exit_scope();

                Ok(ResolvedStatement::While {
                    condition,
                    body: body_resolved,
                })
            }
            Statement::Return { value, .. } => {
                let value = if let Some(v) = value {
                    Some(self.resolve_expr(v, scope)?)
                } else {
                    None
                };
                Ok(ResolvedStatement::Return { value })
            }
            Statement::Expr { expr, .. } => {
                let expr = self.resolve_expr(expr, scope)?;
                Ok(ResolvedStatement::Expr { expr })
            }
            Statement::IndexAssign {
                object,
                index,
                value,
                span,
                object_type,
            } => {
                let resolved_object = self.resolve_expr(object, scope)?;
                let resolved_index = self.resolve_expr(index, scope)?;
                let resolved_value = self.resolve_expr(value, scope)?;
                Ok(ResolvedStatement::IndexAssign {
                    object: resolved_object,
                    index: resolved_index,
                    value: resolved_value,
                    span,
                    object_type,
                })
            }
            Statement::FieldAssign {
                object,
                field,
                value,
                ..
            } => {
                let object = self.resolve_expr(object, scope)?;
                let value = self.resolve_expr(value, scope)?;
                Ok(ResolvedStatement::FieldAssign {
                    object,
                    field,
                    value,
                })
            }
            Statement::ForIn {
                var,
                iterable,
                body,
                ..
            } => {
                let iterable = self.resolve_expr(iterable, scope)?;

                scope.enter_scope();
                // Declare loop variable as mutable within the loop
                let slot = scope.declare(var, true);
                // Allocate 2 hidden slots for __idx and __arr used by codegen
                let _idx_slot = scope.declare("__for_idx".to_string(), true);
                let _arr_slot = scope.declare("__for_arr".to_string(), true);
                let body_resolved = self.resolve_statements(body.statements, scope)?;
                scope.exit_scope();

                Ok(ResolvedStatement::ForIn {
                    slot,
                    iterable,
                    body: body_resolved,
                })
            }
            Statement::ForRange { .. } => {
                unreachable!("ForRange should be desugared before resolution")
            }
            Statement::Throw { value, .. } => {
                let value = self.resolve_expr(value, scope)?;
                Ok(ResolvedStatement::Throw { value })
            }
            Statement::Const { name, init, .. } => {
                // Resolve the init expression (should be a literal)
                let resolved_init = self.resolve_expr(init, scope)?;
                // Register const name for reassignment checking
                scope.const_names.insert(name.clone());
                // Store the resolved literal for inline expansion
                scope.const_values.insert(name, resolved_init);
                // Const produces no runtime code (no slot allocation)
                Ok(ResolvedStatement::Expr {
                    expr: ResolvedExpr::Nil,
                })
            }
            Statement::Try {
                try_block,
                catch_var,
                catch_block,
                ..
            } => {
                scope.enter_scope();
                let try_resolved = self.resolve_statements(try_block.statements, scope)?;
                scope.exit_scope();

                scope.enter_scope();
                let catch_slot = scope.declare(catch_var, false);
                let catch_resolved = self.resolve_statements(catch_block.statements, scope)?;
                scope.exit_scope();

                Ok(ResolvedStatement::Try {
                    try_block: try_resolved,
                    catch_slot,
                    catch_block: catch_resolved,
                })
            }
            Statement::MatchDyn {
                expr,
                arms,
                default_block,
                ..
            } => {
                // Allocate a local slot for the dyn value
                let dyn_slot = scope.declare("__match_dyn".to_string(), false);
                let resolved_expr = self.resolve_expr(expr, scope)?;

                // Resolve each arm
                let mut resolved_arms = Vec::new();
                for arm in arms {
                    scope.enter_scope();
                    let var_slot = scope.declare(arm.var_name, false);

                    // Check if arm type is an interface name
                    let is_interface = if let TypeAnnotation::Named(name) = &arm.type_annotation {
                        self.interface_methods.contains_key(name)
                    } else {
                        false
                    };

                    let kind = if is_interface {
                        let interface_name =
                            if let TypeAnnotation::Named(name) = &arm.type_annotation {
                                name.clone()
                            } else {
                                unreachable!()
                            };
                        let vtable_slot = scope.declare("__vtable".to_string(), false);
                        MatchDynArmKind::InterfaceMatch {
                            interface_name,
                            vtable_slot,
                        }
                    } else {
                        let type_tag_name = match &arm.type_annotation {
                            TypeAnnotation::Named(name) => match arm.type_annotation.to_type() {
                                Ok(ty) => type_to_dyn_tag_name(&ty),
                                Err(_) => name.clone(),
                            },
                            TypeAnnotation::Generic { name, type_args } => {
                                let args = type_args
                                    .iter()
                                    .map(|ta| match ta.to_type() {
                                        Ok(ty) => type_to_dyn_tag_name(&ty),
                                        Err(_) => match ta {
                                            TypeAnnotation::Named(n) => n.clone(),
                                            _ => ta.to_string(),
                                        },
                                    })
                                    .collect::<Vec<_>>()
                                    .join("_");
                                format!("{}_{}", name, args)
                            }
                            _ => {
                                let ty = arm
                                    .type_annotation
                                    .to_type()
                                    .expect("MatchDyn arm type annotation must be valid");
                                type_to_dyn_tag_name(&ty)
                            }
                        };
                        MatchDynArmKind::TypeMatch { type_tag_name }
                    };

                    let body = self.resolve_statements(arm.body.statements, scope)?;
                    scope.exit_scope();
                    resolved_arms.push(ResolvedMatchDynArm {
                        var_slot,
                        kind,
                        body,
                    });
                }

                // Resolve default block
                scope.enter_scope();
                let resolved_default = self.resolve_statements(default_block.statements, scope)?;
                scope.exit_scope();

                Ok(ResolvedStatement::MatchDyn {
                    dyn_slot,
                    expr: resolved_expr,
                    arms: resolved_arms,
                    default_block: resolved_default,
                })
            }
        }
    }

    fn resolve_expr(&mut self, expr: Expr, scope: &mut Scope) -> Result<ResolvedExpr, String> {
        match expr {
            Expr::Int { value, .. } => Ok(ResolvedExpr::Int(value)),
            Expr::Float { value, .. } => Ok(ResolvedExpr::Float(value)),
            Expr::Bool { value, .. } => Ok(ResolvedExpr::Bool(value)),
            Expr::Str { value, .. } => Ok(ResolvedExpr::Str(value)),
            Expr::Nil { .. } => Ok(ResolvedExpr::Nil),
            Expr::Ident { name, span, .. } => {
                // Check if this is a const (inline expansion)
                if let Some(value) = scope.const_values.get(&name) {
                    return Ok(value.clone());
                }
                // Check if this is a captured variable (closure_ref-based)
                if let Some(offset) = scope.lookup_capture(&name) {
                    let is_ref = scope.capture_mutable.contains(&name);
                    return Ok(ResolvedExpr::CaptureLoad { offset, is_ref });
                }
                // Check if this is a promoted var (RefCell) in outer scope
                if scope.promoted_vars.contains(&name) {
                    let (slot, _) = scope.lookup(&name).ok_or_else(|| {
                        self.error(&format!("undefined variable '{}'", name), span)
                    })?;
                    return Ok(ResolvedExpr::RefCellLoad { slot });
                }
                let (slot, _) = scope
                    .lookup_or_capture(&name)
                    .ok_or_else(|| self.error(&format!("undefined variable '{}'", name), span))?;
                Ok(ResolvedExpr::Local(slot))
            }
            Expr::Array { elements, .. } => {
                let resolved: Vec<_> = elements
                    .into_iter()
                    .map(|e| self.resolve_expr(e, scope))
                    .collect::<Result<_, _>>()?;
                Ok(ResolvedExpr::Array { elements: resolved })
            }
            Expr::Index {
                object,
                index,
                span,
                object_type,
                ..
            } => {
                let resolved_object = self.resolve_expr(*object, scope)?;
                let resolved_index = self.resolve_expr(*index, scope)?;
                Ok(ResolvedExpr::Index {
                    object: Box::new(resolved_object),
                    index: Box::new(resolved_index),
                    span,
                    object_type,
                })
            }
            Expr::Field { object, field, .. } => {
                let object = self.resolve_expr(*object, scope)?;
                Ok(ResolvedExpr::Field {
                    object: Box::new(object),
                    field,
                })
            }
            Expr::Unary { op, operand, .. } => {
                let operand = self.resolve_expr(*operand, scope)?;
                Ok(ResolvedExpr::Unary {
                    op,
                    operand: Box::new(operand),
                })
            }
            Expr::Binary {
                op, left, right, ..
            } => {
                let operand_type = left.inferred_type().cloned();
                let left = self.resolve_expr(*left, scope)?;
                let right = self.resolve_expr(*right, scope)?;
                Ok(ResolvedExpr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    operand_type,
                })
            }
            Expr::Call {
                callee, args, span, ..
            } => {
                // Special handling for spawn - it takes a function name, not a value
                if callee == "spawn" {
                    if args.len() != 1 {
                        return Err(
                            self.error("spawn takes exactly 1 argument (function name)", span)
                        );
                    }

                    // Check if the argument is an identifier referring to a function
                    if let Expr::Ident {
                        name,
                        span: arg_span,
                        ..
                    } = &args[0]
                    {
                        if let Some(&func_index) = self.functions.get(name) {
                            return Ok(ResolvedExpr::SpawnFunc { func_index });
                        } else {
                            return Err(self.error(
                                &format!("spawn: '{}' is not a function", name),
                                *arg_span,
                            ));
                        }
                    } else {
                        return Err(self.error("spawn requires a function name", span));
                    }
                }

                let resolved_args: Vec<_> = args
                    .into_iter()
                    .map(|a| self.resolve_expr(a, scope))
                    .collect::<Result<_, _>>()?;

                // Check if it's a builtin
                if self.builtins.contains(&callee) {
                    return Ok(ResolvedExpr::Builtin {
                        name: callee,
                        args: resolved_args,
                        span,
                    });
                }

                // Check if it's a user-defined function
                if let Some(&func_index) = self.functions.get(&callee) {
                    return Ok(ResolvedExpr::Call {
                        func_index,
                        args: resolved_args,
                    });
                }

                // Check if it's a captured variable (closure_ref-based)
                if let Some(offset) = scope.lookup_capture(&callee) {
                    let is_ref = scope.capture_mutable.contains(&callee);
                    return Ok(ResolvedExpr::CallIndirect {
                        callee: Box::new(ResolvedExpr::CaptureLoad { offset, is_ref }),
                        args: resolved_args,
                    });
                }

                // Check if it's a local variable (possibly holding a closure)
                if let Some((slot, _)) = scope.lookup_or_capture(&callee) {
                    return Ok(ResolvedExpr::CallIndirect {
                        callee: Box::new(ResolvedExpr::Local(slot)),
                        args: resolved_args,
                    });
                }

                Err(self.error(&format!("undefined function '{}'", callee), span))
            }
            Expr::StructLiteral {
                name, fields, span, ..
            } => {
                // Look up struct definition
                let struct_info = self
                    .structs
                    .get(&name)
                    .ok_or_else(|| self.error(&format!("undefined struct '{}'", name), span))?;
                let struct_index = struct_info.index;
                let struct_fields = struct_info.fields.clone();

                // Create a map of provided field names to expressions
                let mut field_map: HashMap<String, Expr> = fields.into_iter().collect();

                // Resolve fields in declaration order
                let mut resolved_fields = Vec::new();
                for field_name in &struct_fields {
                    let expr = field_map.remove(field_name).ok_or_else(|| {
                        self.error(
                            &format!("missing field '{}' in struct '{}'", field_name, name),
                            span,
                        )
                    })?;
                    resolved_fields.push(self.resolve_expr(expr, scope)?);
                }

                // Check for extra fields
                if let Some((extra_field, _)) = field_map.into_iter().next() {
                    return Err(self.error(
                        &format!("unknown field '{}' in struct '{}'", extra_field, name),
                        span,
                    ));
                }

                Ok(ResolvedExpr::StructLiteral {
                    struct_index,
                    fields: resolved_fields,
                })
            }
            Expr::MethodCall {
                object,
                method,
                args,
                span,
                object_type,
                ..
            } => {
                // Check if object is a primitive type (set by typechecker)
                let primitive_type_name = object_type.as_ref().and_then(|t| match t {
                    Type::Int => Some("int"),
                    Type::Float => Some("float"),
                    Type::Bool => Some("bool"),
                    Type::String => Some("string"),
                    _ => None,
                });

                // Handle Type::Param — method calls on generic type parameters.
                // The generic function body is never executed directly (only
                // monomorphised versions are), so use a placeholder func_index.
                if matches!(object_type.as_ref(), Some(Type::Param { .. })) {
                    let resolved_object = self.resolve_expr(*object, scope)?;
                    let resolved_args: Vec<_> = args
                        .into_iter()
                        .map(|a| self.resolve_expr(a, scope))
                        .collect::<Result<_, _>>()?;

                    return Ok(ResolvedExpr::MethodCall {
                        object: Box::new(resolved_object),
                        method,
                        func_index: 0, // placeholder — never executed
                        args: resolved_args,
                        return_struct_name: None,
                    });
                }

                if let Some(type_name) = primitive_type_name {
                    // Resolve primitive type method call
                    let resolved_object = self.resolve_expr(*object, scope)?;
                    let resolved_args: Vec<_> = args
                        .into_iter()
                        .map(|a| self.resolve_expr(a, scope))
                        .collect::<Result<_, _>>()?;

                    let func_index = self
                        .primitive_methods
                        .get(type_name)
                        .and_then(|methods| methods.get(&method))
                        .copied()
                        .ok_or_else(|| {
                            self.error(
                                &format!(
                                    "undefined method '{}' for primitive type '{}'",
                                    method, type_name
                                ),
                                span,
                            )
                        })?;

                    return Ok(ResolvedExpr::MethodCall {
                        object: Box::new(resolved_object),
                        method,
                        func_index,
                        args: resolved_args,
                        return_struct_name: None,
                    });
                }

                // Get struct name from the object expression before resolving
                let struct_name = match &*object {
                    Expr::Ident { name, .. } => {
                        scope.lookup_with_type(name).and_then(|(_, _, sn)| sn)
                    }
                    Expr::StructLiteral { name, .. } => Some(name.clone()),
                    Expr::NewLiteral { type_name, .. } => Some(type_name.clone()),
                    _ => None,
                };

                let resolved_object = self.resolve_expr(*object, scope)?;
                let resolved_args: Vec<_> = args
                    .into_iter()
                    .map(|a| self.resolve_expr(a, scope))
                    .collect::<Result<_, _>>()?;

                // If struct_name wasn't found from the unresolved AST,
                // try to get it from the resolved object (e.g., chained method calls)
                let struct_name = struct_name.or_else(|| self.get_struct_name(&resolved_object));

                // Resolve method to function index (static dispatch)
                let (func_index, return_struct_name) = if let Some(sn) = &struct_name {
                    let struct_info = self
                        .structs
                        .get(sn)
                        .ok_or_else(|| self.error(&format!("undefined struct '{}'", sn), span))?;
                    let idx = *struct_info.methods.get(&method).ok_or_else(|| {
                        self.error(
                            &format!("undefined method '{}' for struct '{}'", method, sn),
                            span,
                        )
                    })?;
                    let ret_type = struct_info
                        .method_return_types
                        .get(&method)
                        .cloned()
                        .flatten();
                    (idx, ret_type)
                } else {
                    return Err(self.error(
                        &format!("cannot call method '{}' on non-struct value", method),
                        span,
                    ));
                };

                Ok(ResolvedExpr::MethodCall {
                    object: Box::new(resolved_object),
                    method,
                    func_index,
                    args: resolved_args,
                    return_struct_name,
                })
            }
            Expr::AssociatedFunctionCall {
                type_name,
                function,
                args,
                span,
                ..
            } => {
                // Resolve arguments
                let resolved_args: Vec<_> = args
                    .into_iter()
                    .map(|a| self.resolve_expr(a, scope))
                    .collect::<Result<_, _>>()?;

                // Look up the associated function
                // First check if it's a struct with that associated function
                if let Some(struct_info) = self.structs.get(&type_name)
                    && let Some(&func_index) = struct_info.methods.get(&function)
                {
                    let return_struct_name = struct_info
                        .method_return_types
                        .get(&function)
                        .cloned()
                        .flatten();
                    return Ok(ResolvedExpr::AssociatedFunctionCall {
                        func_index,
                        args: resolved_args,
                        return_struct_name,
                    });
                }

                // Check for builtin types (vec, map) - they use regular functions
                // Look for function name pattern: type_function (e.g., vec_new)
                let builtin_func_name = format!("{}_{}", type_name, function);
                if let Some(&func_index) = self.functions.get(&builtin_func_name) {
                    return Ok(ResolvedExpr::AssociatedFunctionCall {
                        func_index,
                        args: resolved_args,
                        return_struct_name: None,
                    });
                }

                Err(self.error(
                    &format!(
                        "no associated function '{}' found for type '{}'",
                        function, type_name
                    ),
                    span,
                ))
            }
            Expr::Asm(asm_block) => {
                // Resolve input variable names to slots
                let mut input_slots = Vec::new();
                for input_name in &asm_block.inputs {
                    let (slot, _) = scope.lookup(input_name).ok_or_else(|| {
                        self.error(
                            &format!("undefined variable '{}' in asm block", input_name),
                            asm_block.span,
                        )
                    })?;
                    input_slots.push(slot);
                }

                // Resolve asm instructions (just copy, no variable resolution needed)
                let body: Vec<ResolvedAsmInstruction> = asm_block
                    .body
                    .into_iter()
                    .map(|inst| match inst {
                        AsmInstruction::Emit { op_name, args, .. } => {
                            ResolvedAsmInstruction::Emit { op_name, args }
                        }
                        AsmInstruction::Safepoint { .. } => ResolvedAsmInstruction::Safepoint,
                        AsmInstruction::GcHint { size, .. } => {
                            ResolvedAsmInstruction::GcHint { size }
                        }
                    })
                    .collect();

                Ok(ResolvedExpr::AsmBlock {
                    input_slots,
                    output_type: asm_block.output_type,
                    body,
                })
            }
            Expr::NewLiteral {
                type_name,
                type_args,
                elements,
                ..
            } => {
                // Resolve each element
                let resolved_elements: Vec<ResolvedNewLiteralElement> = elements
                    .into_iter()
                    .map(|elem| match elem {
                        crate::compiler::ast::NewLiteralElement::Value(e) => {
                            let resolved = self.resolve_expr(e, scope)?;
                            Ok(ResolvedNewLiteralElement::Value(resolved))
                        }
                        crate::compiler::ast::NewLiteralElement::KeyValue { key, value } => {
                            let resolved_key = self.resolve_expr(key, scope)?;
                            let resolved_value = self.resolve_expr(value, scope)?;
                            Ok(ResolvedNewLiteralElement::KeyValue {
                                key: resolved_key,
                                value: resolved_value,
                            })
                        }
                    })
                    .collect::<Result<_, String>>()?;

                Ok(ResolvedExpr::NewLiteral {
                    type_name,
                    type_args,
                    elements: resolved_elements,
                })
            }

            Expr::Block {
                statements, expr, ..
            } => {
                // Create a new scope for the block
                scope.enter_scope();

                // Resolve all statements in the block
                let resolved_stmts: Vec<ResolvedStatement> = statements
                    .into_iter()
                    .map(|stmt| self.resolve_statement(stmt, scope))
                    .collect::<Result<_, _>>()?;

                // Resolve the final expression
                let resolved_expr = self.resolve_expr(*expr, scope)?;

                scope.exit_scope();

                Ok(ResolvedExpr::Block {
                    statements: resolved_stmts,
                    expr: Box::new(resolved_expr),
                })
            }

            Expr::Lambda {
                params, body, span, ..
            } => {
                // Lambda lifting with static free variable analysis.
                let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                let free_vars = collect_free_vars_block(&body, &param_names);

                // Resolve captured variable slots and mutability from outer scope
                let mut captures: Vec<(String, usize, bool)> = Vec::new(); // (name, slot, is_promoted)
                for var_name in &free_vars {
                    if let Some((slot, mutable)) = scope.lookup(var_name) {
                        // Only mark as ref-capture if the variable is actually promoted to RefCell
                        let is_promoted = mutable && scope.promoted_vars.contains(var_name);
                        captures.push((var_name.clone(), slot, is_promoted));
                    } else if scope.lookup_capture(var_name).is_some() {
                        // Variable is captured from an even outer scope — propagate
                        let is_ref = scope.capture_mutable.contains(var_name);
                        captures.push((var_name.clone(), 0, is_ref)); // slot doesn't matter for re-capture
                    } else if !self.builtins.contains(var_name)
                        && !self.functions.contains_key(var_name)
                    {
                        return Err(self.error(
                            &format!("undefined variable '{}' in lambda", var_name),
                            span,
                        ));
                    }
                }

                let capture_infos: Vec<CaptureInfo> = captures
                    .iter()
                    .map(|(_, slot, mutable)| CaptureInfo {
                        outer_slot: *slot,
                        mutable: *mutable,
                    })
                    .collect();

                // Create a fresh scope: [__closure, param_0, param_1, ...]
                // Captured variables are accessed via CaptureLoad (HeapLoad from closure_ref)
                let mut lambda_scope = Scope::new();
                // Slot 0: __closure (the closure reference itself)
                lambda_scope.declare("__closure".to_string(), false);
                // Register capture name → heap offset mappings
                for (i, (cap_name, _, mutable)) in captures.iter().enumerate() {
                    lambda_scope
                        .capture_heap_offsets
                        .insert(cap_name.clone(), i + 1); // offset 0 = func_index, so captures start at 1
                    if *mutable {
                        lambda_scope.capture_mutable.insert(cap_name.clone());
                    }
                }
                for param in &params {
                    let struct_name = self.struct_name_from_type_annotation(&param.type_annotation);
                    lambda_scope.declare_with_type(param.name.clone(), false, struct_name);
                }

                // Resolve the lambda body
                let fn_type_map = Self::collect_var_types(&body.statements);
                let resolved_body = self.resolve_statements(body.statements, &mut lambda_scope)?;
                let local_types = Self::build_local_types(&lambda_scope, &fn_type_map);

                // Generate unique name and func_index
                let lambda_id = self.next_lambda_id;
                self.next_lambda_id += 1;
                let lambda_name = format!("__lambda_{}", lambda_id);
                let func_index = self.base_func_count + self.lifted_functions.len();

                // Params: [__closure, user_params...]
                let mut all_param_names: Vec<String> = vec!["__closure".to_string()];
                all_param_names.extend(param_names);

                self.lifted_functions.push(ResolvedFunction {
                    name: lambda_name,
                    params: all_param_names,
                    locals_count: lambda_scope.locals_count,
                    body: resolved_body,
                    local_types,
                    is_inline: false,
                });

                Ok(ResolvedExpr::Closure {
                    func_index,
                    captures: capture_infos,
                })
            }

            Expr::CallExpr { callee, args, .. } => {
                let resolved_callee = self.resolve_expr(*callee, scope)?;
                let resolved_args: Vec<_> = args
                    .into_iter()
                    .map(|a| self.resolve_expr(a, scope))
                    .collect::<Result<_, _>>()?;

                Ok(ResolvedExpr::CallIndirect {
                    callee: Box::new(resolved_callee),
                    args: resolved_args,
                })
            }
            Expr::StringInterpolation { .. } => {
                unreachable!("StringInterpolation should be desugared before resolution")
            }

            Expr::AsDyn { expr, .. } => {
                // Get the inner expression's inferred type for the type tag
                let inner_type = expr
                    .inferred_type()
                    .cloned()
                    .expect("AsDyn inner expr must have inferred_type");
                let type_tag_name = type_to_dyn_tag_name(&inner_type);
                // Extract field names and field type tags from the inferred type.
                // Using the Type directly (instead of resolved_structs) ensures
                // generic structs like Container<int> get correct field info.
                let (field_names, field_type_tags) = match &inner_type {
                    Type::Struct { fields, .. } | Type::GenericStruct { fields, .. } => {
                        let names = fields.iter().map(|(n, _)| n.clone()).collect();
                        let type_tags = fields
                            .iter()
                            .map(|(_, ty)| type_to_dyn_tag_name(ty))
                            .collect();
                        (names, type_tags)
                    }
                    Type::Array(_) => {
                        // Array<T> has runtime layout [data_ptr, len]
                        (vec!["data".to_string(), "len".to_string()], vec![])
                    }
                    _ => (vec![], vec![]),
                };
                // Compute auxiliary type tags for container element types
                let aux_type_tags = compute_aux_type_tags(&inner_type);
                // Recursively collect type descriptors for all nested types
                let nested_type_descriptors = collect_nested_type_descriptors(&inner_type);
                // Compute vtable entries for interface implementations
                let vtable_entries = self.compute_vtable_entries(&inner_type);
                let resolved_expr = self.resolve_expr(*expr, scope)?;
                Ok(ResolvedExpr::AsDyn {
                    expr: Box::new(resolved_expr),
                    type_tag_name,
                    field_names,
                    field_type_tags,
                    aux_type_tags,
                    nested_type_descriptors,
                    vtable_entries,
                })
            }
        }
    }

    /// Check if a resolved function body contains a direct call to a specific function index.
    fn body_calls_function(&self, body: &[ResolvedStatement], target_index: usize) -> bool {
        for stmt in body {
            if self.stmt_calls_function(stmt, target_index) {
                return true;
            }
        }
        false
    }

    fn stmt_calls_function(&self, stmt: &ResolvedStatement, target_index: usize) -> bool {
        match stmt {
            ResolvedStatement::Let { init, .. } => self.expr_calls_function(init, target_index),
            ResolvedStatement::Assign { value, .. } => {
                self.expr_calls_function(value, target_index)
            }
            ResolvedStatement::IndexAssign {
                object,
                index,
                value,
                ..
            } => {
                self.expr_calls_function(object, target_index)
                    || self.expr_calls_function(index, target_index)
                    || self.expr_calls_function(value, target_index)
            }
            ResolvedStatement::FieldAssign { object, value, .. } => {
                self.expr_calls_function(object, target_index)
                    || self.expr_calls_function(value, target_index)
            }
            ResolvedStatement::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expr_calls_function(condition, target_index)
                    || self.body_calls_function(then_block, target_index)
                    || else_block
                        .as_ref()
                        .is_some_and(|eb| self.body_calls_function(eb, target_index))
            }
            ResolvedStatement::While { condition, body } => {
                self.expr_calls_function(condition, target_index)
                    || self.body_calls_function(body, target_index)
            }
            ResolvedStatement::ForIn { iterable, body, .. } => {
                self.expr_calls_function(iterable, target_index)
                    || self.body_calls_function(body, target_index)
            }
            ResolvedStatement::Return { value } => value
                .as_ref()
                .is_some_and(|v| self.expr_calls_function(v, target_index)),
            ResolvedStatement::Throw { value } => self.expr_calls_function(value, target_index),
            ResolvedStatement::Try {
                try_block,
                catch_block,
                ..
            } => {
                self.body_calls_function(try_block, target_index)
                    || self.body_calls_function(catch_block, target_index)
            }
            ResolvedStatement::Expr { expr } => self.expr_calls_function(expr, target_index),
            ResolvedStatement::RefCellStore { value, .. } => {
                self.expr_calls_function(value, target_index)
            }
            ResolvedStatement::MatchDyn {
                expr,
                arms,
                default_block,
                ..
            } => {
                self.expr_calls_function(expr, target_index)
                    || arms
                        .iter()
                        .any(|arm| self.body_calls_function(&arm.body, target_index))
                    || self.body_calls_function(default_block, target_index)
            }
        }
    }

    fn expr_calls_function(&self, expr: &ResolvedExpr, target_index: usize) -> bool {
        match expr {
            ResolvedExpr::Call { func_index, args } => {
                *func_index == target_index
                    || args
                        .iter()
                        .any(|a| self.expr_calls_function(a, target_index))
            }
            ResolvedExpr::MethodCall {
                object,
                func_index,
                args,
                ..
            } => {
                *func_index == target_index
                    || self.expr_calls_function(object, target_index)
                    || args
                        .iter()
                        .any(|a| self.expr_calls_function(a, target_index))
            }
            ResolvedExpr::AssociatedFunctionCall {
                func_index, args, ..
            } => {
                *func_index == target_index
                    || args
                        .iter()
                        .any(|a| self.expr_calls_function(a, target_index))
            }
            ResolvedExpr::Array { elements } => elements
                .iter()
                .any(|e| self.expr_calls_function(e, target_index)),
            ResolvedExpr::Index { object, index, .. } => {
                self.expr_calls_function(object, target_index)
                    || self.expr_calls_function(index, target_index)
            }
            ResolvedExpr::Field { object, .. } => self.expr_calls_function(object, target_index),
            ResolvedExpr::Unary { operand, .. } => self.expr_calls_function(operand, target_index),
            ResolvedExpr::Binary { left, right, .. } => {
                self.expr_calls_function(left, target_index)
                    || self.expr_calls_function(right, target_index)
            }
            ResolvedExpr::Builtin { args, .. } => args
                .iter()
                .any(|a| self.expr_calls_function(a, target_index)),
            ResolvedExpr::StructLiteral { fields, .. } => fields
                .iter()
                .any(|f| self.expr_calls_function(f, target_index)),
            ResolvedExpr::Block {
                statements, expr, ..
            } => {
                self.body_calls_function(statements, target_index)
                    || self.expr_calls_function(expr, target_index)
            }
            ResolvedExpr::CaptureStore { value, .. } => {
                self.expr_calls_function(value, target_index)
            }
            ResolvedExpr::RefCellNew { value } => self.expr_calls_function(value, target_index),
            ResolvedExpr::AsDyn { expr, .. } => self.expr_calls_function(expr, target_index),
            _ => false,
        }
    }

    fn error(&self, message: &str, span: Span) -> String {
        format!(
            "error: {}\n  --> {}:{}:{}",
            message, self.filename, span.line, span.column
        )
    }
}

/// Convert a Type to a dyn type tag name.
/// For generic types, includes type parameters (e.g. "Container_int")
/// using the same mangling format as monomorphise.
fn type_to_dyn_tag_name(ty: &Type) -> String {
    match ty {
        Type::Int => "int".to_string(),
        Type::Float => "float".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "string".to_string(),
        Type::Nil => "nil".to_string(),
        Type::Struct { name, .. } => name.clone(),
        Type::GenericStruct {
            name, type_args, ..
        } => {
            let args = type_args
                .iter()
                .map(type_to_dyn_tag_name)
                .collect::<Vec<_>>()
                .join("_");
            format!("{}_{}", name, args)
        }
        Type::Array(elem) => format!("Array_{}", type_to_dyn_tag_name(elem)),
        Type::Vector(elem) => format!("Vec_{}", type_to_dyn_tag_name(elem)),
        Type::Map(k, v) => format!(
            "Map_{}_{}",
            type_to_dyn_tag_name(k),
            type_to_dyn_tag_name(v)
        ),
        _ => ty.to_string(),
    }
}

/// Convert a Type to the name used in interface_impls lookup.
/// This must match the typechecker's `type_to_impl_name`.
fn type_to_impl_name(ty: &Type) -> String {
    match ty {
        Type::Int => "int".to_string(),
        Type::Float => "float".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "string".to_string(),
        Type::Struct { name, .. } => name.clone(),
        Type::GenericStruct { name, .. } => name.clone(),
        Type::Vector(_) => "vec".to_string(),
        Type::Map(_, _) => "map".to_string(),
        _ => ty.to_string(),
    }
}

/// Compute auxiliary type tags for container element types.
/// Vec/Array → [elem_tag], Map → [key_tag, val_tag], Struct fields → field type tags.
fn compute_aux_type_tags(ty: &Type) -> Vec<String> {
    match ty {
        Type::Vector(elem) | Type::Array(elem) => {
            vec![type_to_dyn_tag_name(elem)]
        }
        Type::Map(k, v) => {
            vec![type_to_dyn_tag_name(k), type_to_dyn_tag_name(v)]
        }
        Type::GenericStruct {
            type_args, fields, ..
        } => {
            // Check field structure to detect Vec/Map/Array containers
            let field_names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
            if field_names == ["data", "len", "cap"] && !type_args.is_empty() {
                // Vec-like: aux = [elem_tag]
                vec![type_to_dyn_tag_name(&type_args[0])]
            } else if field_names == ["hm_buckets", "hm_size", "hm_capacity"]
                && type_args.len() >= 2
            {
                // Map-like: aux = [key_tag, val_tag]
                vec![
                    type_to_dyn_tag_name(&type_args[0]),
                    type_to_dyn_tag_name(&type_args[1]),
                ]
            } else if field_names == ["data", "len"] && !type_args.is_empty() {
                // Array-like: aux = [elem_tag]
                vec![type_to_dyn_tag_name(&type_args[0])]
            } else {
                // Named struct with fields — use type_args if present
                let mut aux = Vec::new();
                for type_arg in type_args {
                    aux.push(type_to_dyn_tag_name(type_arg));
                }
                aux
            }
        }
        Type::Struct { name, fields } => {
            // Check if this is a monomorphized container (name like "Vec_int")
            if name.starts_with("Vec_") || name.starts_with("Array_") {
                let field_names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
                if field_names == ["data", "len", "cap"] || field_names == ["data", "len"] {
                    // Extract element type from field "data" which should be an Array
                    if let Some((_, Type::Array(elem))) = fields.first() {
                        return vec![type_to_dyn_tag_name(elem)];
                    }
                }
            }
            if name.starts_with("Map_") {
                let field_names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
                if field_names == ["hm_buckets", "hm_size", "hm_capacity"] {
                    // For monomorphized Map, extract key/val types from the bucket entry type
                    // This is harder, so we fall through to empty
                }
            }
            vec![]
        }
        _ => vec![],
    }
}

/// Recursively collect type descriptors for all nested types reachable from `ty`.
/// Returns a list of (tag_name, field_names, field_type_tags, aux_type_tags) tuples
/// that should be registered with full info in the codegen.
fn collect_nested_type_descriptors(ty: &Type) -> Vec<TypeDescriptorEntry> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    collect_nested_type_descriptors_inner(ty, &mut result, &mut visited);
    result
}

fn collect_nested_type_descriptors_inner(
    ty: &Type,
    result: &mut Vec<TypeDescriptorEntry>,
    visited: &mut HashSet<String>,
) {
    let tag = type_to_dyn_tag_name(ty);
    if visited.contains(&tag) {
        return;
    }
    visited.insert(tag.clone());

    // Extract field info for this type
    let (field_names, field_type_tags): (Vec<String>, Vec<String>) = match ty {
        Type::Struct { fields, .. } | Type::GenericStruct { fields, .. } => {
            let names = fields.iter().map(|(n, _)| n.clone()).collect();
            let tags = fields
                .iter()
                .map(|(_, t)| type_to_dyn_tag_name(t))
                .collect();
            (names, tags)
        }
        Type::Array(_) => (vec!["data".to_string(), "len".to_string()], vec![]),
        _ => (vec![], vec![]),
    };
    let aux_type_tags = compute_aux_type_tags(ty);

    result.push((tag, field_names, field_type_tags, aux_type_tags));

    // Recurse into field types
    if let Type::Struct { fields, .. } | Type::GenericStruct { fields, .. } = ty {
        for (_, field_ty) in fields {
            collect_nested_type_descriptors_inner(field_ty, result, visited);
        }
    }

    // Recurse into element/key/value types
    match ty {
        Type::Vector(elem) | Type::Array(elem) => {
            collect_nested_type_descriptors_inner(elem, result, visited);
        }
        Type::Map(key, val) => {
            collect_nested_type_descriptors_inner(key, result, visited);
            collect_nested_type_descriptors_inner(val, result, visited);
        }
        Type::GenericStruct { type_args, .. } => {
            for arg in type_args {
                collect_nested_type_descriptors_inner(arg, result, visited);
            }
        }
        _ => {}
    }
}

/// Information about a local variable: (slot, mutable, struct_name).
/// struct_name is Some if the variable holds a struct instance.
type LocalInfo = (usize, bool, Option<String>);

/// A scope for variable resolution.
struct Scope {
    locals: Vec<HashMap<String, LocalInfo>>,
    locals_count: usize,
    /// Maps slot number → variable name (accumulated across all scopes).
    slot_names: Vec<String>,
    /// For lambda scopes: outer variables available for capture.
    /// Maps outer variable name → outer slot index.
    outer_vars: HashMap<String, usize>,
    /// Variables actually captured from outer scope during resolution.
    /// Vec of (name, outer_slot) in order of capture.
    captured_vars: Vec<(String, usize)>,
    /// For closure_ref-based capture: maps capture variable name → heap offset.
    /// When set, captured variables resolve to CaptureLoad instead of Local.
    capture_heap_offsets: HashMap<String, usize>,
    /// For closure_ref-based capture: tracks which captured variables are mutable (var).
    capture_mutable: HashSet<String>,
    /// Set of var variable names that are captured by a closure and promoted to RefCell.
    /// Populated by the first pass (find_captured_mutable_vars) before resolution.
    promoted_vars: HashSet<String>,
    /// Set of const variable names in the current scope (used to prevent reassignment).
    const_names: HashSet<String>,
    /// Inline values for const variables (maps name → resolved literal expression).
    const_values: HashMap<String, ResolvedExpr>,
}

impl Scope {
    fn new() -> Self {
        Self {
            locals: vec![HashMap::new()],
            locals_count: 0,
            slot_names: Vec::new(),
            outer_vars: HashMap::new(),
            captured_vars: Vec::new(),
            capture_heap_offsets: HashMap::new(),
            capture_mutable: HashSet::new(),
            promoted_vars: HashSet::new(),
            const_names: HashSet::new(),
            const_values: HashMap::new(),
        }
    }

    fn new_lambda(outer_vars: HashMap<String, usize>) -> Self {
        Self {
            locals: vec![HashMap::new()],
            locals_count: 0,
            slot_names: Vec::new(),
            outer_vars,
            captured_vars: Vec::new(),
            capture_heap_offsets: HashMap::new(),
            capture_mutable: HashSet::new(),
            promoted_vars: HashSet::new(),
            const_names: HashSet::new(),
            const_values: HashMap::new(),
        }
    }

    fn declare(&mut self, name: String, mutable: bool) -> usize {
        self.declare_with_type(name, mutable, None)
    }

    fn declare_with_type(
        &mut self,
        name: String,
        mutable: bool,
        struct_name: Option<String>,
    ) -> usize {
        let slot = self.locals_count;
        self.locals_count += 1;
        self.slot_names.push(name.clone());
        self.locals
            .last_mut()
            .unwrap()
            .insert(name, (slot, mutable, struct_name));
        slot
    }

    fn lookup(&self, name: &str) -> Option<(usize, bool)> {
        self.lookup_with_type(name)
            .map(|(slot, mutable, _)| (slot, mutable))
    }

    /// Lookup a variable, and if not found locally but available in outer_vars,
    /// capture it (declare it as a local and record the capture).
    fn lookup_or_capture(&mut self, name: &str) -> Option<(usize, bool)> {
        // First try local lookup
        if let Some(info) = self.lookup(name) {
            return Some(info);
        }

        // If this is a lambda scope and the variable exists in the outer scope, capture it
        if let Some(&outer_slot) = self.outer_vars.get(name) {
            // Check if already captured
            for (captured_name, _) in &self.captured_vars {
                if captured_name == name {
                    // Already captured, find its local slot
                    return self.lookup(name);
                }
            }
            // Capture it: declare as a local in this scope
            let local_slot = self.declare(name.to_string(), false);
            self.captured_vars.push((name.to_string(), outer_slot));
            return Some((local_slot, false));
        }

        None
    }

    /// Look up a captured variable's heap offset (for closure_ref-based captures).
    fn lookup_capture(&self, name: &str) -> Option<usize> {
        self.capture_heap_offsets.get(name).copied()
    }

    fn lookup_with_type(&self, name: &str) -> Option<LocalInfo> {
        for scope in self.locals.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.clone());
            }
        }
        None
    }

    fn enter_scope(&mut self) {
        self.locals.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        self.locals.pop();
    }
}

/// Collect free variable names from a block that are not in `bound`.
fn collect_free_vars_block(block: &Block, bound: &[String]) -> Vec<String> {
    let mut free = Vec::new();
    let mut bound_set: std::collections::HashSet<String> = bound.iter().cloned().collect();
    for stmt in &block.statements {
        collect_free_vars_statement(stmt, &mut bound_set, &mut free);
    }
    free
}

fn collect_free_vars_statement(
    stmt: &Statement,
    bound: &mut std::collections::HashSet<String>,
    free: &mut Vec<String>,
) {
    match stmt {
        Statement::Let { name, init, .. } => {
            collect_free_vars_expr(init, bound, free);
            bound.insert(name.clone());
        }
        Statement::Assign { name, value, .. } => {
            if !bound.contains(name.as_str()) && !free.contains(name) {
                free.push(name.clone());
            }
            collect_free_vars_expr(value, bound, free);
        }
        Statement::IndexAssign {
            object,
            index,
            value,
            ..
        } => {
            collect_free_vars_expr(object, bound, free);
            collect_free_vars_expr(index, bound, free);
            collect_free_vars_expr(value, bound, free);
        }
        Statement::FieldAssign { object, value, .. } => {
            collect_free_vars_expr(object, bound, free);
            collect_free_vars_expr(value, bound, free);
        }
        Statement::If {
            condition,
            then_block,
            else_block,
            ..
        } => {
            collect_free_vars_expr(condition, bound, free);
            for s in &then_block.statements {
                collect_free_vars_statement(s, bound, free);
            }
            if let Some(else_b) = else_block {
                for s in &else_b.statements {
                    collect_free_vars_statement(s, bound, free);
                }
            }
        }
        Statement::While {
            condition, body, ..
        } => {
            collect_free_vars_expr(condition, bound, free);
            for s in &body.statements {
                collect_free_vars_statement(s, bound, free);
            }
        }
        Statement::ForIn {
            var,
            iterable,
            body,
            ..
        } => {
            collect_free_vars_expr(iterable, bound, free);
            bound.insert(var.clone());
            for s in &body.statements {
                collect_free_vars_statement(s, bound, free);
            }
        }
        Statement::ForRange { .. } => {
            // ForRange is desugared before resolution; this branch handles
            // the AST-level free-var collection which still sees ForRange.
            unreachable!("ForRange should be desugared before free-var collection")
        }
        Statement::Return { value, .. } => {
            if let Some(expr) = value {
                collect_free_vars_expr(expr, bound, free);
            }
        }
        Statement::Throw { value, .. } => {
            collect_free_vars_expr(value, bound, free);
        }
        Statement::Try {
            try_block,
            catch_var,
            catch_block,
            ..
        } => {
            for s in &try_block.statements {
                collect_free_vars_statement(s, bound, free);
            }
            bound.insert(catch_var.clone());
            for s in &catch_block.statements {
                collect_free_vars_statement(s, bound, free);
            }
        }
        Statement::Const { .. } => {
            // Const values are inlined; they don't create free variables
        }
        Statement::Expr { expr, .. } => {
            collect_free_vars_expr(expr, bound, free);
        }
        Statement::MatchDyn {
            expr,
            arms,
            default_block,
            ..
        } => {
            collect_free_vars_expr(expr, bound, free);
            for arm in arms {
                let mut arm_bound = bound.clone();
                arm_bound.insert(arm.var_name.clone());
                for s in &arm.body.statements {
                    collect_free_vars_statement(s, &mut arm_bound, free);
                }
            }
            for s in &default_block.statements {
                collect_free_vars_statement(s, bound, free);
            }
        }
    }
}

fn collect_free_vars_expr(
    expr: &Expr,
    bound: &std::collections::HashSet<String>,
    free: &mut Vec<String>,
) {
    match expr {
        Expr::Ident { name, .. } => {
            if !bound.contains(name.as_str()) && !free.contains(name) {
                free.push(name.clone());
            }
        }
        Expr::Array { elements, .. } => {
            for e in elements {
                collect_free_vars_expr(e, bound, free);
            }
        }
        Expr::Index { object, index, .. } => {
            collect_free_vars_expr(object, bound, free);
            collect_free_vars_expr(index, bound, free);
        }
        Expr::Field { object, .. } => collect_free_vars_expr(object, bound, free),
        Expr::Unary { operand, .. } => collect_free_vars_expr(operand, bound, free),
        Expr::Binary { left, right, .. } => {
            collect_free_vars_expr(left, bound, free);
            collect_free_vars_expr(right, bound, free);
        }
        Expr::Call { callee, args, .. } => {
            // The callee might be a variable holding a closure (not just a function name)
            if !bound.contains(callee.as_str()) && !free.contains(callee) {
                free.push(callee.clone());
            }
            for a in args {
                collect_free_vars_expr(a, bound, free);
            }
        }
        Expr::CallExpr { callee, args, .. } => {
            collect_free_vars_expr(callee, bound, free);
            for a in args {
                collect_free_vars_expr(a, bound, free);
            }
        }
        Expr::MethodCall { object, args, .. } => {
            collect_free_vars_expr(object, bound, free);
            for a in args {
                collect_free_vars_expr(a, bound, free);
            }
        }
        Expr::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                collect_free_vars_expr(e, bound, free);
            }
        }
        Expr::AssociatedFunctionCall { args, .. } => {
            for a in args {
                collect_free_vars_expr(a, bound, free);
            }
        }
        Expr::NewLiteral { elements, .. } => {
            for e in elements {
                match e {
                    NewLiteralElement::Value(v) => collect_free_vars_expr(v, bound, free),
                    NewLiteralElement::KeyValue { key, value } => {
                        collect_free_vars_expr(key, bound, free);
                        collect_free_vars_expr(value, bound, free);
                    }
                }
            }
        }
        Expr::Block {
            statements, expr, ..
        } => {
            let mut inner_bound = bound.clone();
            for s in statements {
                collect_free_vars_statement(s, &mut inner_bound, free);
            }
            collect_free_vars_expr(expr, &inner_bound, free);
        }
        Expr::Lambda { params, body, .. } => {
            let mut inner_bound = bound.clone();
            for p in params {
                inner_bound.insert(p.name.clone());
            }
            for s in &body.statements {
                collect_free_vars_statement(s, &mut inner_bound, free);
            }
        }
        Expr::StringInterpolation { parts, .. } => {
            for part in parts {
                if let crate::compiler::ast::StringInterpPart::Expr(e) = part {
                    collect_free_vars_expr(e, bound, free);
                }
            }
        }
        Expr::AsDyn { expr, .. } => {
            collect_free_vars_expr(expr, bound, free);
        }
        Expr::Int { .. }
        | Expr::Float { .. }
        | Expr::Bool { .. }
        | Expr::Str { .. }
        | Expr::Nil { .. }
        | Expr::Asm(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lexer::Lexer;
    use crate::compiler::parser::Parser;

    fn resolve(source: &str) -> Result<ResolvedProgram, String> {
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens()?;
        let mut parser = Parser::new("test.mc", tokens);
        let program = parser.parse()?;
        let mut resolver = Resolver::new("test.mc");
        resolver.resolve(program)
    }

    #[test]
    fn test_simple_resolution() {
        let program = resolve("let x = 42; __value_to_string(x);").unwrap();
        assert_eq!(program.main_body.len(), 2);
    }

    #[test]
    fn test_undefined_variable() {
        let result = resolve("__value_to_string(x);");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("undefined variable"));
    }

    #[test]
    fn test_undefined_function() {
        let result = resolve("foo();");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("undefined function"));
    }

    #[test]
    fn test_let_reassignment() {
        let result = resolve("let x = 1; x = 2;");
        assert!(result.is_ok());
    }

    #[test]
    fn test_const_reassignment_error() {
        let result = resolve("const x = 1; x = 2;");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot assign to constant"));
    }

    #[test]
    fn test_function_resolution() {
        let program = resolve("fun add(a, b) { return a + b; } let r = add(1, 2);").unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].name, "add");
    }
}
