//! Monomorphisation pass for generic functions and structs.
//!
//! This module transforms generic code into specialized (monomorphic) versions
//! by collecting all instantiations and generating concrete implementations.
//!
//! The process:
//! 1. Traverse the AST to find all call sites of generic functions/structs
//! 2. Collect the concrete type arguments used at each call site
//! 3. Generate specialized versions of the generic definitions
//! 4. Rewrite call sites to use the specialized versions

use crate::compiler::ast::{
    Block, Expr, FnDef, ImplBlock, Item, MatchDynArm, Program, Statement, StructDef,
};
use crate::compiler::types::Type;
use std::collections::{HashMap, HashSet};

/// Represents a specific instantiation of a generic function or struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instantiation {
    /// Name of the generic function/struct
    pub name: String,
    /// Concrete type arguments
    pub type_args: Vec<Type>,
}

impl std::hash::Hash for Instantiation {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        // Hash based on mangled name which is deterministic
        self.mangled_name().hash(state);
    }
}

impl Instantiation {
    /// Generate the mangled name for this instantiation.
    /// e.g., `identity` with `[int]` becomes `identity__int`
    pub fn mangled_name(&self) -> String {
        if self.type_args.is_empty() {
            self.name.clone()
        } else {
            let type_suffix = self
                .type_args
                .iter()
                .map(mangle_type)
                .collect::<Vec<_>>()
                .join("_");
            format!("{}__{}", self.name, type_suffix)
        }
    }
}

/// Mangle a type into a string suitable for use in a function name.
pub fn mangle_type(ty: &Type) -> String {
    match ty {
        Type::Int => "int".to_string(),
        Type::Float => "float".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "string".to_string(),
        Type::Nil => "nil".to_string(),
        Type::Ptr(elem) => format!("ptr_{}", mangle_type(elem)),
        Type::Any => "any".to_string(),
        Type::Dyn => "dyn".to_string(),
        Type::Nullable(inner) => format!("opt_{}", mangle_type(inner)),
        Type::Function { params, ret } => {
            let params_str = params.iter().map(mangle_type).collect::<Vec<_>>().join("_");
            format!("fn_{}_{}", params_str, mangle_type(ret))
        }
        Type::Struct { name, .. } => name.clone(),
        Type::GenericStruct {
            name, type_args, ..
        } => {
            let display_name = match name.as_str() {
                "Array" => "array",
                "Vec" => "vec",
                "Map" => "map",
                _ => name.as_str(),
            };
            let args_str = type_args
                .iter()
                .map(mangle_type)
                .collect::<Vec<_>>()
                .join("_");
            format!("{}_{}", display_name, args_str)
        }
        Type::Var(id) => format!("T{}", id),
        Type::Param { name } => name.clone(),
        Type::InterfaceBound { interface_name } => interface_name.clone(),
    }
}

/// Collects all instantiations of generic functions and structs.
pub struct InstantiationCollector {
    /// Generic function definitions: name -> FnDef
    generic_functions: HashMap<String, FnDef>,
    /// Generic struct definitions: name -> StructDef
    generic_structs: HashMap<String, StructDef>,
    /// Generic impl blocks: struct_name -> Vec<ImplBlock>
    generic_impl_blocks: HashMap<String, Vec<ImplBlock>>,
    /// Collected instantiations
    instantiations: HashSet<Instantiation>,
}

impl InstantiationCollector {
    pub fn new() -> Self {
        Self {
            generic_functions: HashMap::new(),
            generic_structs: HashMap::new(),
            generic_impl_blocks: HashMap::new(),
            instantiations: HashSet::new(),
        }
    }

    /// Collect all generic definitions and their instantiations from a program.
    pub fn collect(&mut self, program: &Program) {
        // First pass: collect generic definitions
        for item in &program.items {
            match item {
                Item::FnDef(fn_def) if !fn_def.type_params.is_empty() => {
                    self.generic_functions
                        .insert(fn_def.name.clone(), fn_def.clone());
                }
                Item::StructDef(struct_def) if !struct_def.type_params.is_empty() => {
                    self.generic_structs
                        .insert(struct_def.name.clone(), struct_def.clone());
                }
                Item::ImplBlock(impl_block) => {
                    self.collect_impl_block_definitions(impl_block);
                }
                _ => {}
            }
        }

        // Second pass: collect instantiations from call sites
        for item in &program.items {
            self.collect_item(item);
        }
    }

    fn collect_impl_block_definitions(&mut self, impl_block: &ImplBlock) {
        // Store generic impl blocks for later specialization
        if !impl_block.type_params.is_empty() {
            self.generic_impl_blocks
                .entry(impl_block.struct_name.clone())
                .or_default()
                .push(impl_block.clone());
        }

        // Collect generic methods from impl blocks
        for method in &impl_block.methods {
            if !method.type_params.is_empty() || !impl_block.type_params.is_empty() {
                // Store with qualified name: StructName::method_name
                let qualified_name = format!("{}::{}", impl_block.struct_name, method.name);
                self.generic_functions
                    .insert(qualified_name, method.clone());
            }
        }
    }

    pub fn collect_item(&mut self, item: &Item) {
        match item {
            Item::FnDef(fn_def) => {
                self.collect_block(&fn_def.body);
            }
            Item::ImplBlock(impl_block) => {
                for method in &impl_block.methods {
                    self.collect_block(&method.body);
                }
            }
            Item::Statement(stmt) => {
                self.collect_statement(stmt);
            }
            _ => {}
        }
    }

    fn collect_block(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.collect_statement(stmt);
        }
    }

    fn collect_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Let { init, .. } => {
                self.collect_expr(init);
            }
            Statement::Assign { value, .. } => {
                self.collect_expr(value);
            }
            Statement::IndexAssign {
                object,
                index,
                value,
                ..
            } => {
                self.collect_expr(object);
                self.collect_expr(index);
                self.collect_expr(value);
            }
            Statement::FieldAssign { object, value, .. } => {
                self.collect_expr(object);
                self.collect_expr(value);
            }
            Statement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                self.collect_expr(condition);
                self.collect_block(then_block);
                if let Some(else_block) = else_block {
                    self.collect_block(else_block);
                }
            }
            Statement::While {
                condition, body, ..
            } => {
                self.collect_expr(condition);
                self.collect_block(body);
            }
            Statement::ForIn { iterable, body, .. } => {
                self.collect_expr(iterable);
                self.collect_block(body);
            }
            Statement::ForRange { .. } => {
                unreachable!("ForRange should be desugared before monomorphisation")
            }
            Statement::Return { value, .. } => {
                if let Some(value) = value {
                    self.collect_expr(value);
                }
            }
            Statement::Throw { value, .. } => {
                self.collect_expr(value);
            }
            Statement::Try {
                try_block,
                catch_block,
                ..
            } => {
                self.collect_block(try_block);
                self.collect_block(catch_block);
            }
            Statement::MatchDyn {
                expr,
                arms,
                default_block,
                ..
            } => {
                self.collect_expr(expr);
                for arm in arms {
                    self.collect_block(&arm.body);
                }
                self.collect_block(default_block);
            }
            Statement::Expr { expr, .. } => {
                self.collect_expr(expr);
            }
            Statement::Const { .. } => {}
        }
    }

    fn collect_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Call {
                callee,
                type_args,
                args,
                ..
            } => {
                // Check if this is a call to a generic function
                if self.generic_functions.contains_key(callee) && !type_args.is_empty() {
                    // Convert type annotations to types for mangling
                    let concrete_types: Vec<Type> = type_args
                        .iter()
                        .filter_map(|ta| ta.to_type().ok())
                        .collect();

                    if concrete_types.len() == type_args.len() {
                        self.instantiations.insert(Instantiation {
                            name: callee.clone(),
                            type_args: concrete_types,
                        });
                    }
                }

                for arg in args {
                    self.collect_expr(arg);
                }
            }
            Expr::StructLiteral {
                name,
                type_args,
                fields,
                ..
            } => {
                // Check if this is a generic struct instantiation
                if self.generic_structs.contains_key(name) && !type_args.is_empty() {
                    let concrete_types: Vec<Type> = type_args
                        .iter()
                        .filter_map(|ta| ta.to_type().ok())
                        .collect();

                    if concrete_types.len() == type_args.len() {
                        self.instantiations.insert(Instantiation {
                            name: name.clone(),
                            type_args: concrete_types,
                        });
                    }
                }

                for (_, field_expr) in fields {
                    self.collect_expr(field_expr);
                }
            }
            Expr::MethodCall {
                object,
                type_args,
                args,
                ..
            } => {
                self.collect_expr(object);
                // TODO: Handle generic method calls
                let _ = type_args; // Suppress unused warning for now
                for arg in args {
                    self.collect_expr(arg);
                }
            }
            Expr::AssociatedFunctionCall {
                type_name,
                type_args,
                function,
                fn_type_args,
                args,
                ..
            } => {
                // Register struct-level instantiation (e.g., Vec<int> → Vec + [Int])
                if self.generic_structs.contains_key(type_name) && !type_args.is_empty() {
                    let struct_types: Vec<Type> = type_args
                        .iter()
                        .filter_map(|ta| ta.to_type().ok())
                        .collect();
                    if struct_types.len() == type_args.len() {
                        self.instantiations.insert(Instantiation {
                            name: type_name.clone(),
                            type_args: struct_types,
                        });
                    }
                }

                // If the method itself has type params, register function-level instantiation
                if !fn_type_args.is_empty() {
                    let qualified_name = format!("{}::{}", type_name, function);
                    if self.generic_functions.contains_key(&qualified_name) {
                        let fn_types: Vec<Type> = fn_type_args
                            .iter()
                            .filter_map(|ta| ta.to_type().ok())
                            .collect();
                        if fn_types.len() == fn_type_args.len() {
                            self.instantiations.insert(Instantiation {
                                name: qualified_name,
                                type_args: fn_types,
                            });
                        }
                    }
                }

                for arg in args {
                    self.collect_expr(arg);
                }
            }
            Expr::Array { elements, .. } => {
                for elem in elements {
                    self.collect_expr(elem);
                }
            }
            Expr::Index { object, index, .. } => {
                self.collect_expr(object);
                self.collect_expr(index);
            }
            Expr::Field { object, .. } => {
                self.collect_expr(object);
            }
            Expr::Unary { operand, .. } => {
                self.collect_expr(operand);
            }
            Expr::Binary { left, right, .. } => {
                self.collect_expr(left);
                self.collect_expr(right);
            }
            Expr::NewLiteral {
                type_name,
                type_args,
                elements,
                ..
            } => {
                // Check if this is a generic type instantiation
                if self.generic_structs.contains_key(type_name) && !type_args.is_empty() {
                    let concrete_types: Vec<Type> = type_args
                        .iter()
                        .filter_map(|ta| ta.to_type().ok())
                        .collect();

                    if concrete_types.len() == type_args.len() {
                        self.instantiations.insert(Instantiation {
                            name: type_name.clone(),
                            type_args: concrete_types,
                        });
                    }
                }

                for elem in elements {
                    match elem {
                        crate::compiler::ast::NewLiteralElement::Value(e) => {
                            self.collect_expr(e);
                        }
                        crate::compiler::ast::NewLiteralElement::KeyValue { key, value } => {
                            self.collect_expr(key);
                            self.collect_expr(value);
                        }
                    }
                }
            }
            Expr::Block {
                statements, expr, ..
            } => {
                // Collect from all statements and the final expression
                for stmt in statements {
                    self.collect_statement(stmt);
                }
                self.collect_expr(expr);
            }
            Expr::Lambda { body, .. } => {
                for stmt in &body.statements {
                    self.collect_statement(stmt);
                }
            }
            Expr::CallExpr { callee, args, .. } => {
                self.collect_expr(callee);
                for arg in args {
                    self.collect_expr(arg);
                }
            }
            Expr::StringInterpolation { parts, .. } => {
                for part in parts {
                    if let crate::compiler::ast::StringInterpPart::Expr(e) = part {
                        self.collect_expr(e);
                    }
                }
            }
            Expr::AsDyn { expr, .. } => {
                self.collect_expr(expr);
            }
            // Literals and asm blocks don't contain generic calls
            Expr::Int { .. }
            | Expr::Float { .. }
            | Expr::Bool { .. }
            | Expr::Str { .. }
            | Expr::Nil { .. }
            | Expr::Ident { .. }
            | Expr::Asm(_) => {}
        }
    }

    /// Get all collected instantiations.
    pub fn instantiations(&self) -> &HashSet<Instantiation> {
        &self.instantiations
    }

    /// Get generic function definitions.
    pub fn generic_functions(&self) -> &HashMap<String, FnDef> {
        &self.generic_functions
    }

    /// Get generic struct definitions.
    pub fn generic_structs(&self) -> &HashMap<String, StructDef> {
        &self.generic_structs
    }

    /// Get generic impl blocks.
    pub fn generic_impl_blocks(&self) -> &HashMap<String, Vec<ImplBlock>> {
        &self.generic_impl_blocks
    }
}

impl Default for InstantiationCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Monomorphiser generates specialized versions of generic functions and structs.
pub struct Monomorphiser {
    /// Generic function definitions
    generic_functions: HashMap<String, FnDef>,
    /// Generic struct definitions
    generic_structs: HashMap<String, StructDef>,
    /// Generic impl blocks: struct_name -> Vec<ImplBlock>
    generic_impl_blocks: HashMap<String, Vec<ImplBlock>>,
}

impl Monomorphiser {
    pub fn new(
        generic_functions: HashMap<String, FnDef>,
        generic_structs: HashMap<String, StructDef>,
        generic_impl_blocks: HashMap<String, Vec<ImplBlock>>,
    ) -> Self {
        Self {
            generic_functions,
            generic_structs,
            generic_impl_blocks,
        }
    }

    /// Create a Monomorphiser from an InstantiationCollector.
    pub fn from_collector(collector: &InstantiationCollector) -> Self {
        Self::new(
            collector.generic_functions().clone(),
            collector.generic_structs().clone(),
            collector.generic_impl_blocks().clone(),
        )
    }

    /// Generate a specialized function from a generic function and concrete type arguments.
    pub fn specialize_function(&self, instantiation: &Instantiation) -> Option<FnDef> {
        let generic_fn = self.generic_functions.get(&instantiation.name)?;

        // Check that type args match type params
        if generic_fn.type_params.len() != instantiation.type_args.len() {
            return None;
        }

        // Create type parameter to concrete type mapping
        let type_map: HashMap<String, Type> = generic_fn
            .type_params
            .iter()
            .cloned()
            .zip(instantiation.type_args.iter().cloned())
            .collect();

        // Create specialized function
        let specialized = FnDef {
            name: instantiation.mangled_name(),
            type_params: Vec::new(),       // No longer generic
            type_param_bounds: Vec::new(), // No longer generic
            params: generic_fn
                .params
                .iter()
                .map(|p| crate::compiler::ast::Param {
                    name: p.name.clone(),
                    type_annotation: p
                        .type_annotation
                        .as_ref()
                        .map(|ta| substitute_type_annotation(ta, &type_map)),
                    span: p.span,
                })
                .collect(),
            return_type: generic_fn
                .return_type
                .as_ref()
                .map(|ta| substitute_type_annotation(ta, &type_map)),
            body: substitute_block(&generic_fn.body, &type_map),
            attributes: generic_fn.attributes.clone(),
            span: generic_fn.span,
        };

        Some(specialized)
    }

    /// Generate a specialized struct from a generic struct and concrete type arguments.
    pub fn specialize_struct(&self, instantiation: &Instantiation) -> Option<StructDef> {
        let generic_struct = self.generic_structs.get(&instantiation.name)?;

        // Check that type args match type params
        if generic_struct.type_params.len() != instantiation.type_args.len() {
            return None;
        }

        // Create type parameter to concrete type mapping
        let type_map: HashMap<String, Type> = generic_struct
            .type_params
            .iter()
            .cloned()
            .zip(instantiation.type_args.iter().cloned())
            .collect();

        // Create specialized struct
        let specialized = StructDef {
            name: instantiation.mangled_name(),
            type_params: Vec::new(), // No longer generic
            fields: generic_struct
                .fields
                .iter()
                .map(|f| crate::compiler::ast::StructField {
                    name: f.name.clone(),
                    type_annotation: substitute_type_annotation(&f.type_annotation, &type_map),
                    span: f.span,
                })
                .collect(),
            span: generic_struct.span,
        };

        Some(specialized)
    }

    /// Generate specialized impl blocks from a struct instantiation.
    pub fn specialize_impl_blocks(&self, instantiation: &Instantiation) -> Vec<ImplBlock> {
        let generic_impls = match self.generic_impl_blocks.get(&instantiation.name) {
            Some(impls) => impls,
            None => return Vec::new(),
        };

        let mut results = Vec::new();
        for generic_impl in generic_impls {
            // Check that type args match type params
            if generic_impl.type_params.len() != instantiation.type_args.len() {
                continue;
            }

            // Skip interface impl blocks — monomorphising them would create
            // duplicate vtable entries that interfere with the original generic
            // interface resolution (e.g., impl<T> ToString for Vec<T>).
            if generic_impl.interface_name.is_some() {
                continue;
            }

            // Create type parameter to concrete type mapping
            let type_map: HashMap<String, Type> = generic_impl
                .type_params
                .iter()
                .cloned()
                .zip(instantiation.type_args.iter().cloned())
                .collect();

            // Create specialized impl block
            let specialized = ImplBlock {
                type_params: Vec::new(), // No longer generic
                interface_name: generic_impl.interface_name.clone(),
                struct_name: instantiation.mangled_name(),
                struct_type_args: Vec::new(), // No longer generic
                methods: generic_impl
                    .methods
                    .iter()
                    .map(|m| FnDef {
                        name: m.name.clone(),
                        type_params: m.type_params.clone(), // Keep method-level type params
                        type_param_bounds: m.type_param_bounds.clone(),
                        params: m
                            .params
                            .iter()
                            .map(|p| crate::compiler::ast::Param {
                                name: p.name.clone(),
                                type_annotation: p
                                    .type_annotation
                                    .as_ref()
                                    .map(|ann| substitute_type_annotation(ann, &type_map)),
                                span: p.span,
                            })
                            .collect(),
                        return_type: m
                            .return_type
                            .as_ref()
                            .map(|ann| substitute_type_annotation(ann, &type_map)),
                        body: substitute_block(&m.body, &type_map),
                        attributes: m.attributes.clone(),
                        span: m.span,
                    })
                    .collect(),
                span: generic_impl.span,
            };

            results.push(specialized);
        }

        results
    }

    /// Generate all specialized items from a set of instantiations.
    pub fn generate_all(&self, instantiations: &HashSet<Instantiation>) -> Vec<Item> {
        let mut items = Vec::new();

        // Sort instantiations by mangled name for deterministic output order.
        // HashSet iteration order is non-deterministic, which would cause
        // function indices to vary between runs, breaking vtable entries.
        let mut sorted_insts: Vec<&Instantiation> = instantiations.iter().collect();
        sorted_insts.sort_by_key(|inst| inst.mangled_name());

        for inst in sorted_insts {
            if self.generic_functions.contains_key(&inst.name)
                && let Some(specialized_fn) = self.specialize_function(inst)
            {
                items.push(Item::FnDef(specialized_fn));
            } else if self.generic_structs.contains_key(&inst.name) {
                // Generate both specialized struct and impl blocks
                if let Some(specialized_struct) = self.specialize_struct(inst) {
                    items.push(Item::StructDef(specialized_struct));
                }
                for specialized_impl in self.specialize_impl_blocks(inst) {
                    items.push(Item::ImplBlock(specialized_impl));
                }
            }
        }

        items
    }
}

/// Substitute type parameters in a Type value with concrete types from type_map.
fn substitute_type_in_type(ty: &Type, type_map: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Param { name } => type_map.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Function { params, ret } => Type::Function {
            params: params
                .iter()
                .map(|p| substitute_type_in_type(p, type_map))
                .collect(),
            ret: Box::new(substitute_type_in_type(ret, type_map)),
        },
        Type::GenericStruct {
            name,
            type_args,
            fields,
        } => Type::GenericStruct {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|t| substitute_type_in_type(t, type_map))
                .collect(),
            fields: fields
                .iter()
                .map(|(n, t): &(String, Type)| (n.clone(), substitute_type_in_type(t, type_map)))
                .collect(),
        },
        Type::Ptr(inner) => Type::Ptr(Box::new(substitute_type_in_type(inner, type_map))),
        _ => ty.clone(),
    }
}

/// Substitute type parameters in an optional inferred_type field.
fn substitute_inferred_type(
    inferred_type: &Option<Type>,
    type_map: &HashMap<String, Type>,
) -> Option<Type> {
    inferred_type
        .as_ref()
        .map(|t| substitute_type_in_type(t, type_map))
}

/// Substitute type parameters in a type annotation with concrete types.
fn substitute_type_annotation(
    ann: &crate::compiler::types::TypeAnnotation,
    type_map: &HashMap<String, Type>,
) -> crate::compiler::types::TypeAnnotation {
    use crate::compiler::types::TypeAnnotation;

    match ann {
        TypeAnnotation::Named(name) => {
            // Check if this is a type parameter that should be substituted
            if let Some(concrete) = type_map.get(name) {
                type_to_annotation(concrete)
            } else {
                TypeAnnotation::Named(name.clone())
            }
        }
        TypeAnnotation::Array(elem) => {
            TypeAnnotation::Array(Box::new(substitute_type_annotation(elem, type_map)))
        }
        TypeAnnotation::Vec(elem) => {
            TypeAnnotation::Vec(Box::new(substitute_type_annotation(elem, type_map)))
        }
        TypeAnnotation::Map(key, value) => TypeAnnotation::Map(
            Box::new(substitute_type_annotation(key, type_map)),
            Box::new(substitute_type_annotation(value, type_map)),
        ),
        TypeAnnotation::Nullable(inner) => {
            TypeAnnotation::Nullable(Box::new(substitute_type_annotation(inner, type_map)))
        }
        TypeAnnotation::Function { params, ret } => TypeAnnotation::Function {
            params: params
                .iter()
                .map(|p| substitute_type_annotation(p, type_map))
                .collect(),
            ret: Box::new(substitute_type_annotation(ret, type_map)),
        },
        TypeAnnotation::Generic { name, type_args } => TypeAnnotation::Generic {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|ta| substitute_type_annotation(ta, type_map))
                .collect(),
        },
    }
}

/// Convert a Type back to a TypeAnnotation.
fn type_to_annotation(ty: &Type) -> crate::compiler::types::TypeAnnotation {
    use crate::compiler::types::TypeAnnotation;

    match ty {
        Type::Int => TypeAnnotation::Named("int".to_string()),
        Type::Float => TypeAnnotation::Named("float".to_string()),
        Type::Bool => TypeAnnotation::Named("bool".to_string()),
        Type::String => TypeAnnotation::Named("string".to_string()),
        Type::Nil => TypeAnnotation::Named("nil".to_string()),
        Type::Ptr(elem) => TypeAnnotation::Generic {
            name: "ptr".to_string(),
            type_args: vec![type_to_annotation(elem)],
        },
        Type::Any => TypeAnnotation::Named("any".to_string()),
        Type::Nullable(inner) => TypeAnnotation::Nullable(Box::new(type_to_annotation(inner))),
        Type::Function { params, ret } => TypeAnnotation::Function {
            params: params.iter().map(type_to_annotation).collect(),
            ret: Box::new(type_to_annotation(ret)),
        },
        Type::Struct { name, .. } => TypeAnnotation::Named(name.clone()),
        Type::GenericStruct {
            name, type_args, ..
        } => {
            let ta: Vec<_> = type_args.iter().map(type_to_annotation).collect();
            match name.as_str() {
                "Array" if ta.len() == 1 => {
                    TypeAnnotation::Array(Box::new(ta.into_iter().next().unwrap()))
                }
                "Vec" if ta.len() == 1 => {
                    TypeAnnotation::Vec(Box::new(ta.into_iter().next().unwrap()))
                }
                "Map" if ta.len() == 2 => {
                    let mut iter = ta.into_iter();
                    TypeAnnotation::Map(
                        Box::new(iter.next().unwrap()),
                        Box::new(iter.next().unwrap()),
                    )
                }
                _ => TypeAnnotation::Generic {
                    name: name.clone(),
                    type_args: ta,
                },
            }
        }
        Type::Var(_) => TypeAnnotation::Named("any".to_string()), // Fallback for unresolved vars
        Type::Param { name } => TypeAnnotation::Named(name.clone()),
        Type::InterfaceBound { interface_name } => TypeAnnotation::Named(interface_name.clone()),
        Type::Dyn => TypeAnnotation::Named("dyn".to_string()),
    }
}

/// Substitute type parameters in a block.
fn substitute_block(block: &Block, type_map: &HashMap<String, Type>) -> Block {
    Block {
        statements: block
            .statements
            .iter()
            .map(|stmt| substitute_statement(stmt, type_map))
            .collect(),
        span: block.span,
    }
}

/// Substitute type parameters in a statement.
fn substitute_statement(stmt: &Statement, type_map: &HashMap<String, Type>) -> Statement {
    match stmt {
        Statement::Let {
            name,
            type_annotation,
            init,
            span,
            inferred_type,
        } => Statement::Let {
            name: name.clone(),
            type_annotation: type_annotation
                .as_ref()
                .map(|ta| substitute_type_annotation(ta, type_map)),
            init: substitute_expr(init, type_map),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Statement::Assign { name, value, span } => Statement::Assign {
            name: name.clone(),
            value: substitute_expr(value, type_map),
            span: *span,
        },
        Statement::IndexAssign {
            object,
            index,
            value,
            span,
            object_type,
        } => Statement::IndexAssign {
            object: substitute_expr(object, type_map),
            index: substitute_expr(index, type_map),
            value: substitute_expr(value, type_map),
            span: *span,
            object_type: substitute_inferred_type(object_type, type_map),
        },
        Statement::FieldAssign {
            object,
            field,
            value,
            span,
        } => Statement::FieldAssign {
            object: substitute_expr(object, type_map),
            field: field.clone(),
            value: substitute_expr(value, type_map),
            span: *span,
        },
        Statement::If {
            condition,
            then_block,
            else_block,
            span,
        } => Statement::If {
            condition: substitute_expr(condition, type_map),
            then_block: substitute_block(then_block, type_map),
            else_block: else_block.as_ref().map(|b| substitute_block(b, type_map)),
            span: *span,
        },
        Statement::While {
            condition,
            body,
            span,
        } => Statement::While {
            condition: substitute_expr(condition, type_map),
            body: substitute_block(body, type_map),
            span: *span,
        },
        Statement::ForIn {
            var,
            iterable,
            body,
            span,
        } => Statement::ForIn {
            var: var.clone(),
            iterable: substitute_expr(iterable, type_map),
            body: substitute_block(body, type_map),
            span: *span,
        },
        Statement::ForRange { .. } => {
            unreachable!("ForRange should be desugared before monomorphisation")
        }
        Statement::Return { value, span } => Statement::Return {
            value: value.as_ref().map(|v| substitute_expr(v, type_map)),
            span: *span,
        },
        Statement::Throw { value, span } => Statement::Throw {
            value: substitute_expr(value, type_map),
            span: *span,
        },
        Statement::Try {
            try_block,
            catch_var,
            catch_block,
            span,
        } => Statement::Try {
            try_block: substitute_block(try_block, type_map),
            catch_var: catch_var.clone(),
            catch_block: substitute_block(catch_block, type_map),
            span: *span,
        },
        Statement::MatchDyn {
            expr,
            arms,
            default_block,
            span,
        } => Statement::MatchDyn {
            expr: substitute_expr(expr, type_map),
            arms: arms
                .iter()
                .map(|arm| MatchDynArm {
                    var_name: arm.var_name.clone(),
                    type_annotation: arm.type_annotation.clone(),
                    body: substitute_block(&arm.body, type_map),
                    span: arm.span,
                })
                .collect(),
            default_block: substitute_block(default_block, type_map),
            span: *span,
        },
        Statement::Expr { expr, span } => Statement::Expr {
            expr: substitute_expr(expr, type_map),
            span: *span,
        },
        Statement::Const { name, init, span } => Statement::Const {
            name: name.clone(),
            init: init.clone(),
            span: *span,
        },
    }
}

/// Substitute type parameters in an expression.
fn substitute_expr(expr: &Expr, type_map: &HashMap<String, Type>) -> Expr {
    match expr {
        Expr::Int {
            value,
            span,
            inferred_type,
        } => Expr::Int {
            value: *value,
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Float {
            value,
            span,
            inferred_type,
        } => Expr::Float {
            value: *value,
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Bool {
            value,
            span,
            inferred_type,
        } => Expr::Bool {
            value: *value,
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Str {
            value,
            span,
            inferred_type,
        } => Expr::Str {
            value: value.clone(),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Nil {
            span,
            inferred_type,
        } => Expr::Nil {
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Ident {
            name,
            span,
            inferred_type,
        } => Expr::Ident {
            name: name.clone(),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Array {
            elements,
            span,
            inferred_type,
        } => Expr::Array {
            elements: elements
                .iter()
                .map(|e| substitute_expr(e, type_map))
                .collect(),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Index {
            object,
            index,
            span,
            object_type,
            inferred_type,
        } => Expr::Index {
            object: Box::new(substitute_expr(object, type_map)),
            index: Box::new(substitute_expr(index, type_map)),
            span: *span,
            object_type: substitute_inferred_type(object_type, type_map),
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Field {
            object,
            field,
            span,
            inferred_type,
        } => Expr::Field {
            object: Box::new(substitute_expr(object, type_map)),
            field: field.clone(),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Unary {
            op,
            operand,
            span,
            inferred_type,
        } => Expr::Unary {
            op: *op,
            operand: Box::new(substitute_expr(operand, type_map)),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Binary {
            op,
            left,
            right,
            span,
            inferred_type,
        } => Expr::Binary {
            op: *op,
            left: Box::new(substitute_expr(left, type_map)),
            right: Box::new(substitute_expr(right, type_map)),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Call {
            callee,
            type_args,
            args,
            span,
            inferred_type,
        } => Expr::Call {
            callee: callee.clone(),
            type_args: type_args
                .iter()
                .map(|ta| substitute_type_annotation(ta, type_map))
                .collect(),
            args: args.iter().map(|a| substitute_expr(a, type_map)).collect(),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::StructLiteral {
            name,
            type_args,
            fields,
            span,
            inferred_type,
        } => Expr::StructLiteral {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|ta| substitute_type_annotation(ta, type_map))
                .collect(),
            fields: fields
                .iter()
                .map(|(n, e)| (n.clone(), substitute_expr(e, type_map)))
                .collect(),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::MethodCall {
            object,
            method,
            type_args,
            args,
            span,
            object_type,
            inferred_type,
        } => Expr::MethodCall {
            object: Box::new(substitute_expr(object, type_map)),
            method: method.clone(),
            type_args: type_args
                .iter()
                .map(|ta| substitute_type_annotation(ta, type_map))
                .collect(),
            args: args.iter().map(|a| substitute_expr(a, type_map)).collect(),
            span: *span,
            object_type: object_type
                .as_ref()
                .map(|t| substitute_type_in_type(t, type_map)),
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::AssociatedFunctionCall {
            type_name,
            type_args,
            function,
            fn_type_args,
            args,
            span,
            inferred_type,
        } => Expr::AssociatedFunctionCall {
            type_name: type_name.clone(),
            type_args: type_args
                .iter()
                .map(|ta| substitute_type_annotation(ta, type_map))
                .collect(),
            function: function.clone(),
            fn_type_args: fn_type_args
                .iter()
                .map(|ta| substitute_type_annotation(ta, type_map))
                .collect(),
            args: args.iter().map(|a| substitute_expr(a, type_map)).collect(),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Asm(asm_block) => Expr::Asm(asm_block.clone()),
        Expr::NewLiteral {
            type_name,
            type_args,
            elements,
            span,
            inferred_type,
        } => Expr::NewLiteral {
            type_name: type_name.clone(),
            type_args: type_args
                .iter()
                .map(|ta| substitute_type_annotation(ta, type_map))
                .collect(),
            elements: elements
                .iter()
                .map(|elem| match elem {
                    crate::compiler::ast::NewLiteralElement::Value(e) => {
                        crate::compiler::ast::NewLiteralElement::Value(substitute_expr(e, type_map))
                    }
                    crate::compiler::ast::NewLiteralElement::KeyValue { key, value } => {
                        crate::compiler::ast::NewLiteralElement::KeyValue {
                            key: substitute_expr(key, type_map),
                            value: substitute_expr(value, type_map),
                        }
                    }
                })
                .collect(),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Block {
            statements,
            expr,
            span,
            inferred_type,
        } => Expr::Block {
            statements: statements
                .iter()
                .map(|s| substitute_statement(s, type_map))
                .collect(),
            expr: Box::new(substitute_expr(expr, type_map)),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::Lambda {
            params,
            return_type,
            body,
            span,
            inferred_type,
        } => Expr::Lambda {
            params: params.clone(),
            return_type: return_type
                .as_ref()
                .map(|ta| substitute_type_annotation(ta, type_map)),
            body: substitute_block(body, type_map),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::CallExpr {
            callee,
            args,
            span,
            inferred_type,
        } => Expr::CallExpr {
            callee: Box::new(substitute_expr(callee, type_map)),
            args: args.iter().map(|a| substitute_expr(a, type_map)).collect(),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
        Expr::AsDyn {
            expr,
            span,
            inferred_type,
            is_implicit,
        } => Expr::AsDyn {
            expr: Box::new(substitute_expr(expr, type_map)),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
            is_implicit: *is_implicit,
        },
        Expr::StringInterpolation {
            parts,
            span,
            inferred_type,
        } => Expr::StringInterpolation {
            parts: parts
                .iter()
                .map(|part| match part {
                    crate::compiler::ast::StringInterpPart::Literal(s) => {
                        crate::compiler::ast::StringInterpPart::Literal(s.clone())
                    }
                    crate::compiler::ast::StringInterpPart::Expr(e) => {
                        crate::compiler::ast::StringInterpPart::Expr(Box::new(substitute_expr(
                            e, type_map,
                        )))
                    }
                })
                .collect(),
            span: *span,
            inferred_type: substitute_inferred_type(inferred_type, type_map),
        },
    }
}

/// Main entry point: monomorphise a program.
///
/// This function:
/// 1. Collects all instantiations of generic functions and structs
/// 2. Generates specialized versions for each instantiation
/// 3. Rewrites call sites to use the specialized versions
/// 4. Returns a new program with specialized definitions added
pub fn monomorphise_program(program: Program) -> Program {
    // Step 1: Collect instantiations
    let mut collector = InstantiationCollector::new();
    collector.collect(&program);

    // If no generic instantiations, return unchanged
    if collector.instantiations().is_empty() {
        return program;
    }

    // Step 2: Generate specialized items, transitively collecting new instantiations
    let monomorphiser = Monomorphiser::from_collector(&collector);
    let mut all_instantiations = collector.instantiations().clone();
    let mut pending = all_instantiations.clone();
    let mut all_specialized_items = Vec::new();

    loop {
        let specialized_items = monomorphiser.generate_all(&pending);
        if specialized_items.is_empty() {
            break;
        }

        // Collect new instantiations from the specialized items' bodies
        let mut new_instantiations = HashSet::new();
        for item in &specialized_items {
            collector.collect_item(item);
        }
        for inst in collector.instantiations() {
            if !all_instantiations.contains(inst) {
                new_instantiations.insert(inst.clone());
                all_instantiations.insert(inst.clone());
            }
        }

        all_specialized_items.extend(specialized_items);

        if new_instantiations.is_empty() {
            break;
        }
        pending = new_instantiations;
    }

    // Step 3: Rewrite call sites in the original program
    let rewritten_items: Vec<Item> = program
        .items
        .into_iter()
        .map(|item| rewrite_item(item, &all_instantiations))
        .collect();

    // Step 4: Rewrite call sites in specialized items too
    let rewritten_specialized: Vec<Item> = all_specialized_items
        .into_iter()
        .map(|item| rewrite_item(item, &all_instantiations))
        .collect();

    // Step 5: Combine specialized items with rewritten items
    let mut final_items = rewritten_specialized;
    final_items.extend(rewritten_items);

    Program { items: final_items }
}

/// Rewrite call sites in an item to use mangled names.
fn rewrite_item(item: Item, instantiations: &HashSet<Instantiation>) -> Item {
    match item {
        Item::FnDef(fn_def) => Item::FnDef(FnDef {
            name: fn_def.name,
            type_params: fn_def.type_params,
            type_param_bounds: fn_def.type_param_bounds,
            params: fn_def.params,
            return_type: fn_def.return_type,
            body: rewrite_block(&fn_def.body, instantiations),
            attributes: fn_def.attributes,
            span: fn_def.span,
        }),
        Item::ImplBlock(impl_block) => Item::ImplBlock(ImplBlock {
            type_params: impl_block.type_params,
            interface_name: impl_block.interface_name,
            struct_name: impl_block.struct_name,
            struct_type_args: impl_block.struct_type_args,
            methods: impl_block
                .methods
                .into_iter()
                .map(|m| FnDef {
                    name: m.name,
                    type_params: m.type_params,
                    type_param_bounds: m.type_param_bounds,
                    params: m.params,
                    return_type: m.return_type,
                    body: rewrite_block(&m.body, instantiations),
                    attributes: m.attributes,
                    span: m.span,
                })
                .collect(),
            span: impl_block.span,
        }),
        Item::Statement(stmt) => Item::Statement(rewrite_statement(&stmt, instantiations)),
        other => other,
    }
}

/// Rewrite call sites in a block.
fn rewrite_block(block: &Block, instantiations: &HashSet<Instantiation>) -> Block {
    Block {
        statements: block
            .statements
            .iter()
            .map(|stmt| rewrite_statement(stmt, instantiations))
            .collect(),
        span: block.span,
    }
}

/// Rewrite call sites in a statement.
fn rewrite_statement(stmt: &Statement, instantiations: &HashSet<Instantiation>) -> Statement {
    match stmt {
        Statement::Let {
            name,
            type_annotation,
            init,
            span,
            inferred_type,
        } => Statement::Let {
            name: name.clone(),
            type_annotation: type_annotation.clone(),
            init: rewrite_expr(init, instantiations),
            span: *span,
            inferred_type: inferred_type.clone(),
        },
        Statement::Assign { name, value, span } => Statement::Assign {
            name: name.clone(),
            value: rewrite_expr(value, instantiations),
            span: *span,
        },
        Statement::IndexAssign {
            object,
            index,
            value,
            span,
            object_type,
        } => Statement::IndexAssign {
            object: rewrite_expr(object, instantiations),
            index: rewrite_expr(index, instantiations),
            value: rewrite_expr(value, instantiations),
            span: *span,
            object_type: object_type.clone(),
        },
        Statement::FieldAssign {
            object,
            field,
            value,
            span,
        } => Statement::FieldAssign {
            object: rewrite_expr(object, instantiations),
            field: field.clone(),
            value: rewrite_expr(value, instantiations),
            span: *span,
        },
        Statement::If {
            condition,
            then_block,
            else_block,
            span,
        } => Statement::If {
            condition: rewrite_expr(condition, instantiations),
            then_block: rewrite_block(then_block, instantiations),
            else_block: else_block
                .as_ref()
                .map(|b| rewrite_block(b, instantiations)),
            span: *span,
        },
        Statement::While {
            condition,
            body,
            span,
        } => Statement::While {
            condition: rewrite_expr(condition, instantiations),
            body: rewrite_block(body, instantiations),
            span: *span,
        },
        Statement::ForIn {
            var,
            iterable,
            body,
            span,
        } => Statement::ForIn {
            var: var.clone(),
            iterable: rewrite_expr(iterable, instantiations),
            body: rewrite_block(body, instantiations),
            span: *span,
        },
        Statement::ForRange { .. } => {
            unreachable!("ForRange should be desugared before monomorphisation")
        }
        Statement::Return { value, span } => Statement::Return {
            value: value.as_ref().map(|v| rewrite_expr(v, instantiations)),
            span: *span,
        },
        Statement::Throw { value, span } => Statement::Throw {
            value: rewrite_expr(value, instantiations),
            span: *span,
        },
        Statement::Try {
            try_block,
            catch_var,
            catch_block,
            span,
        } => Statement::Try {
            try_block: rewrite_block(try_block, instantiations),
            catch_var: catch_var.clone(),
            catch_block: rewrite_block(catch_block, instantiations),
            span: *span,
        },
        Statement::MatchDyn {
            expr,
            arms,
            default_block,
            span,
        } => Statement::MatchDyn {
            expr: rewrite_expr(expr, instantiations),
            arms: arms
                .iter()
                .map(|arm| MatchDynArm {
                    var_name: arm.var_name.clone(),
                    type_annotation: arm.type_annotation.clone(),
                    body: rewrite_block(&arm.body, instantiations),
                    span: arm.span,
                })
                .collect(),
            default_block: rewrite_block(default_block, instantiations),
            span: *span,
        },
        Statement::Expr { expr, span } => Statement::Expr {
            expr: rewrite_expr(expr, instantiations),
            span: *span,
        },
        Statement::Const { name, init, span } => Statement::Const {
            name: name.clone(),
            init: init.clone(),
            span: *span,
        },
    }
}

/// Rewrite call sites in an expression.
fn rewrite_expr(expr: &Expr, instantiations: &HashSet<Instantiation>) -> Expr {
    match expr {
        Expr::Call {
            callee,
            type_args,
            args,
            span,
            inferred_type,
        } => {
            // Check if this is a call to a generic function with type args
            if !type_args.is_empty() {
                // Try to find matching instantiation
                let concrete_types: Vec<Type> = type_args
                    .iter()
                    .filter_map(|ta| ta.to_type().ok())
                    .collect();

                if concrete_types.len() == type_args.len() {
                    let inst = Instantiation {
                        name: callee.clone(),
                        type_args: concrete_types,
                    };

                    if instantiations.contains(&inst) {
                        // Rewrite to use mangled name without type args
                        return Expr::Call {
                            callee: inst.mangled_name(),
                            type_args: Vec::new(),
                            args: args
                                .iter()
                                .map(|a| rewrite_expr(a, instantiations))
                                .collect(),
                            span: *span,
                            inferred_type: inferred_type.clone(),
                        };
                    }
                }
            }

            // No rewrite needed, but still recurse into args
            Expr::Call {
                callee: callee.clone(),
                type_args: type_args.clone(),
                args: args
                    .iter()
                    .map(|a| rewrite_expr(a, instantiations))
                    .collect(),
                span: *span,
                inferred_type: inferred_type.clone(),
            }
        }
        Expr::StructLiteral {
            name,
            type_args,
            fields,
            span,
            inferred_type,
        } => {
            // Check if this is a generic struct instantiation
            if !type_args.is_empty() {
                let concrete_types: Vec<Type> = type_args
                    .iter()
                    .filter_map(|ta| ta.to_type().ok())
                    .collect();

                if concrete_types.len() == type_args.len() {
                    let inst = Instantiation {
                        name: name.clone(),
                        type_args: concrete_types,
                    };

                    if instantiations.contains(&inst) {
                        return Expr::StructLiteral {
                            name: inst.mangled_name(),
                            type_args: Vec::new(),
                            fields: fields
                                .iter()
                                .map(|(n, e)| (n.clone(), rewrite_expr(e, instantiations)))
                                .collect(),
                            span: *span,
                            inferred_type: inferred_type.clone(),
                        };
                    }
                }
            }

            Expr::StructLiteral {
                name: name.clone(),
                type_args: type_args.clone(),
                fields: fields
                    .iter()
                    .map(|(n, e)| (n.clone(), rewrite_expr(e, instantiations)))
                    .collect(),
                span: *span,
                inferred_type: inferred_type.clone(),
            }
        }
        // Recurse into other expressions
        Expr::Array {
            elements,
            span,
            inferred_type,
        } => Expr::Array {
            elements: elements
                .iter()
                .map(|e| rewrite_expr(e, instantiations))
                .collect(),
            span: *span,
            inferred_type: inferred_type.clone(),
        },
        Expr::Index {
            object,
            index,
            span,
            object_type,
            inferred_type,
        } => Expr::Index {
            object: Box::new(rewrite_expr(object, instantiations)),
            index: Box::new(rewrite_expr(index, instantiations)),
            span: *span,
            object_type: object_type.clone(),
            inferred_type: inferred_type.clone(),
        },
        Expr::Field {
            object,
            field,
            span,
            inferred_type,
        } => Expr::Field {
            object: Box::new(rewrite_expr(object, instantiations)),
            field: field.clone(),
            span: *span,
            inferred_type: inferred_type.clone(),
        },
        Expr::Unary {
            op,
            operand,
            span,
            inferred_type,
        } => Expr::Unary {
            op: *op,
            operand: Box::new(rewrite_expr(operand, instantiations)),
            span: *span,
            inferred_type: inferred_type.clone(),
        },
        Expr::Binary {
            op,
            left,
            right,
            span,
            inferred_type,
        } => Expr::Binary {
            op: *op,
            left: Box::new(rewrite_expr(left, instantiations)),
            right: Box::new(rewrite_expr(right, instantiations)),
            span: *span,
            inferred_type: inferred_type.clone(),
        },
        Expr::MethodCall {
            object,
            method,
            type_args,
            args,
            span,
            object_type,
            inferred_type,
        } => Expr::MethodCall {
            object: Box::new(rewrite_expr(object, instantiations)),
            method: method.clone(),
            type_args: type_args.clone(),
            args: args
                .iter()
                .map(|a| rewrite_expr(a, instantiations))
                .collect(),
            span: *span,
            object_type: object_type.clone(),
            inferred_type: inferred_type.clone(),
        },
        Expr::AssociatedFunctionCall {
            type_name,
            type_args,
            function,
            fn_type_args,
            args,
            span,
            inferred_type,
        } => Expr::AssociatedFunctionCall {
            type_name: type_name.clone(),
            type_args: type_args.clone(),
            function: function.clone(),
            fn_type_args: fn_type_args.clone(),
            args: args
                .iter()
                .map(|a| rewrite_expr(a, instantiations))
                .collect(),
            span: *span,
            inferred_type: inferred_type.clone(),
        },
        Expr::AsDyn {
            expr,
            span,
            inferred_type,
            is_implicit,
        } => Expr::AsDyn {
            expr: Box::new(rewrite_expr(expr, instantiations)),
            span: *span,
            inferred_type: inferred_type.clone(),
            is_implicit: *is_implicit,
        },
        // Literals and identifiers don't need rewriting
        _ => expr.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mangle_type() {
        assert_eq!(mangle_type(&Type::Int), "int");
        assert_eq!(mangle_type(&Type::String), "string");
        assert_eq!(mangle_type(&Type::array(Type::Int)), "array_int");
        assert_eq!(mangle_type(&Type::vector(Type::Int)), "vec_int");
    }

    #[test]
    fn test_instantiation_mangled_name() {
        let inst = Instantiation {
            name: "identity".to_string(),
            type_args: vec![Type::Int],
        };
        assert_eq!(inst.mangled_name(), "identity__int");

        let inst2 = Instantiation {
            name: "pair".to_string(),
            type_args: vec![Type::Int, Type::String],
        };
        assert_eq!(inst2.mangled_name(), "pair__int_string");
    }
}
