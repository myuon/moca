use crate::compiler::lexer::Span;
use crate::compiler::types::{Type, TypeAnnotation};

/// A complete program consisting of items (functions and statements).
#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
}

/// Top-level items in a program.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Item {
    Import(Import),
    FnDef(FnDef),
    StructDef(StructDef),
    ImplBlock(ImplBlock),
    Statement(Statement),
}

/// An import statement.
#[derive(Debug, Clone)]
pub struct Import {
    /// Module path segments (e.g., ["utils", "http"] for `import utils.http;`)
    pub path: Vec<String>,
    /// Whether it's a relative import (starts with ./)
    pub relative: bool,
    pub span: Span,
}

/// A struct field definition.
#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub type_annotation: TypeAnnotation,
    pub span: Span,
}

/// A struct definition.
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    /// Type parameters for generic structs: `struct Container<T> { ... }`
    pub type_params: Vec<String>,
    pub fields: Vec<StructField>,
    pub span: Span,
}

/// An impl block containing methods for a struct.
#[derive(Debug, Clone)]
pub struct ImplBlock {
    /// Type parameters for the impl block: `impl<T> Container<T> { ... }`
    pub type_params: Vec<String>,
    pub struct_name: String,
    /// Type arguments for the struct: `impl<T> Container<T> { ... }` has `[T]`
    pub struct_type_args: Vec<TypeAnnotation>,
    pub methods: Vec<FnDef>,
    pub span: Span,
}

/// An attribute annotation (e.g., `@inline`).
#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: String,
    pub span: Span,
}

/// A function parameter with optional type annotation.
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub type_annotation: Option<TypeAnnotation>,
    pub span: Span,
}

/// A function definition.
#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    /// Type parameters for generic functions: `fun identity<T>(x: T) -> T { ... }`
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeAnnotation>,
    pub body: Block,
    pub attributes: Vec<Attribute>,
    pub span: Span,
}

/// A block of statements.
#[derive(Debug, Clone)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

/// Statements in the language.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Statement {
    Let {
        name: String,
        type_annotation: Option<TypeAnnotation>,
        init: Expr,
        span: Span,
        /// Inferred type of the variable (set by typechecker)
        inferred_type: Option<Type>,
    },
    Assign {
        name: String,
        value: Expr,
        span: Span,
    },
    IndexAssign {
        object: Expr,
        index: Expr,
        value: Expr,
        span: Span,
        /// Type of the object (set by typechecker for codegen)
        object_type: Option<Type>,
    },
    FieldAssign {
        object: Expr,
        field: String,
        value: Expr,
        span: Span,
    },
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<Block>,
        span: Span,
    },
    While {
        condition: Expr,
        body: Block,
        span: Span,
    },
    ForIn {
        var: String,
        iterable: Expr,
        body: Block,
        span: Span,
    },
    /// Range-based for loop: `for i in start..end { body }` or `for i in start..=end { body }`
    ForRange {
        var: String,
        start: Expr,
        end: Expr,
        inclusive: bool,
        body: Block,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Throw {
        value: Expr,
        span: Span,
    },
    Try {
        try_block: Block,
        catch_var: String,
        catch_block: Block,
        span: Span,
    },
    Expr {
        expr: Expr,
        span: Span,
    },
    /// A const declaration: `const NAME = <literal>;`
    /// Only literal values (int, float, string, bool) are allowed.
    Const {
        name: String,
        init: Expr,
        span: Span,
    },
}

/// A part of a string interpolation expression.
#[derive(Debug, Clone)]
pub enum StringInterpPart {
    /// Literal text.
    Literal(String),
    /// An expression to be evaluated and converted to string.
    Expr(Box<Expr>),
}

/// Expressions in the language.
#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Expr {
    Int {
        value: i64,
        span: Span,
        inferred_type: Option<Type>,
    },
    Float {
        value: f64,
        span: Span,
        inferred_type: Option<Type>,
    },
    Bool {
        value: bool,
        span: Span,
        inferred_type: Option<Type>,
    },
    Str {
        value: String,
        span: Span,
        inferred_type: Option<Type>,
    },
    /// String interpolation: `"hello {name}, age {age}"`
    StringInterpolation {
        parts: Vec<StringInterpPart>,
        span: Span,
        inferred_type: Option<Type>,
    },
    Nil {
        span: Span,
        inferred_type: Option<Type>,
    },
    Ident {
        name: String,
        span: Span,
        inferred_type: Option<Type>,
    },
    Array {
        elements: Vec<Expr>,
        span: Span,
        inferred_type: Option<Type>,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
        span: Span,
        /// Type of the object (set by typechecker for codegen)
        object_type: Option<Type>,
        inferred_type: Option<Type>,
    },
    Field {
        object: Box<Expr>,
        field: String,
        span: Span,
        inferred_type: Option<Type>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
        inferred_type: Option<Type>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
        inferred_type: Option<Type>,
    },
    Call {
        callee: String,
        /// Type arguments for generic function calls: `identity<int>(42)`
        type_args: Vec<TypeAnnotation>,
        args: Vec<Expr>,
        span: Span,
        inferred_type: Option<Type>,
    },
    /// Struct literal: `Point { x: 1, y: 2 }` or `Container<int> { value: 42 }`
    StructLiteral {
        name: String,
        /// Type arguments for generic struct literals: `Container<int> { value: 42 }`
        type_args: Vec<TypeAnnotation>,
        fields: Vec<(String, Expr)>,
        span: Span,
        inferred_type: Option<Type>,
    },
    /// Method call: `obj.method(args)` or `obj.method<U>(args)`
    MethodCall {
        object: Box<Expr>,
        method: String,
        /// Type arguments for generic method calls: `container.map<string>(f)`
        type_args: Vec<TypeAnnotation>,
        args: Vec<Expr>,
        span: Span,
        /// Type of the object (set by typechecker)
        object_type: Option<Type>,
        inferred_type: Option<Type>,
    },
    /// Associated function call: `Type::func(args)` or `Type<T>::func(args)`
    AssociatedFunctionCall {
        type_name: String,
        /// Type arguments for the type: `Container<int>::new(42)`
        type_args: Vec<TypeAnnotation>,
        function: String,
        /// Type arguments for the function: `Container<int>::create<U>()`
        fn_type_args: Vec<TypeAnnotation>,
        args: Vec<Expr>,
        span: Span,
        inferred_type: Option<Type>,
    },
    /// Inline assembly block: `asm(inputs) -> type { ... }`
    Asm(AsmBlock),
    /// New literal: `new Vec<int> {1, 2, 3}` or `new Map<string, int> {"a": 1, "b": 2}`
    NewLiteral {
        type_name: String,
        /// Type arguments: `Vec<int>` has `[int]`
        type_args: Vec<TypeAnnotation>,
        /// Elements: either all Value or all KeyValue
        elements: Vec<NewLiteralElement>,
        span: Span,
        inferred_type: Option<Type>,
    },
    /// Block expression: `{ stmt1; stmt2; expr }` - evaluates to the final expression.
    /// This is generated by the desugar phase to expand NewLiteral.
    Block {
        /// Statements to execute before the final expression
        statements: Vec<Statement>,
        /// The final expression whose value is the result of the block
        expr: Box<Expr>,
        span: Span,
        inferred_type: Option<Type>,
    },
    /// Lambda expression: `fun(x: int) -> int { return x + 1; }`
    Lambda {
        params: Vec<Param>,
        return_type: Option<TypeAnnotation>,
        body: Block,
        span: Span,
        inferred_type: Option<Type>,
    },
    /// Dynamic call expression: `expr(args)` where expr is not a simple identifier.
    /// Used for calling closures stored in variables: `f(10)` or `make_adder(5)(10)`
    CallExpr {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
        inferred_type: Option<Type>,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Int { span, .. } => *span,
            Expr::Float { span, .. } => *span,
            Expr::Bool { span, .. } => *span,
            Expr::Str { span, .. } => *span,
            Expr::Nil { span, .. } => *span,
            Expr::Ident { span, .. } => *span,
            Expr::Array { span, .. } => *span,
            Expr::Index { span, .. } => *span,
            Expr::Field { span, .. } => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::StructLiteral { span, .. } => *span,
            Expr::MethodCall { span, .. } => *span,
            Expr::AssociatedFunctionCall { span, .. } => *span,
            Expr::Asm(asm_block) => asm_block.span,
            Expr::NewLiteral { span, .. } => *span,
            Expr::Block { span, .. } => *span,
            Expr::Lambda { span, .. } => *span,
            Expr::CallExpr { span, .. } => *span,
            Expr::StringInterpolation { span, .. } => *span,
        }
    }

    /// Set the inferred type of this expression (called by typechecker).
    pub fn set_inferred_type(&mut self, ty: Type) {
        match self {
            Expr::Int { inferred_type, .. }
            | Expr::Float { inferred_type, .. }
            | Expr::Bool { inferred_type, .. }
            | Expr::Str { inferred_type, .. }
            | Expr::StringInterpolation { inferred_type, .. }
            | Expr::Nil { inferred_type, .. }
            | Expr::Ident { inferred_type, .. }
            | Expr::Array { inferred_type, .. }
            | Expr::Index { inferred_type, .. }
            | Expr::Field { inferred_type, .. }
            | Expr::Unary { inferred_type, .. }
            | Expr::Binary { inferred_type, .. }
            | Expr::Call { inferred_type, .. }
            | Expr::StructLiteral { inferred_type, .. }
            | Expr::MethodCall { inferred_type, .. }
            | Expr::AssociatedFunctionCall { inferred_type, .. }
            | Expr::NewLiteral { inferred_type, .. }
            | Expr::Block { inferred_type, .. }
            | Expr::Lambda { inferred_type, .. }
            | Expr::CallExpr { inferred_type, .. } => *inferred_type = Some(ty),
            Expr::Asm(_) => {}
        }
    }

    /// Get the inferred type of this expression.
    pub fn inferred_type(&self) -> Option<&Type> {
        match self {
            Expr::Int { inferred_type, .. }
            | Expr::Float { inferred_type, .. }
            | Expr::Bool { inferred_type, .. }
            | Expr::Str { inferred_type, .. }
            | Expr::Nil { inferred_type, .. }
            | Expr::Ident { inferred_type, .. }
            | Expr::Array { inferred_type, .. }
            | Expr::Index { inferred_type, .. }
            | Expr::Field { inferred_type, .. }
            | Expr::Unary { inferred_type, .. }
            | Expr::Binary { inferred_type, .. }
            | Expr::Call { inferred_type, .. }
            | Expr::StructLiteral { inferred_type, .. }
            | Expr::MethodCall { inferred_type, .. }
            | Expr::AssociatedFunctionCall { inferred_type, .. }
            | Expr::NewLiteral { inferred_type, .. }
            | Expr::Block { inferred_type, .. }
            | Expr::Lambda { inferred_type, .. }
            | Expr::CallExpr { inferred_type, .. }
            | Expr::StringInterpolation { inferred_type, .. } => inferred_type.as_ref(),
            Expr::Asm(_) => None,
        }
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    Shl,
    Shr,
}

/// An element in a new literal.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum NewLiteralElement {
    /// Simple expression: `1`, `"foo"` etc.
    Value(Expr),
    /// Key-value pair: `"a": 1`, `key: value`
    KeyValue { key: Expr, value: Expr },
}

/// An inline assembly block.
#[derive(Debug, Clone)]
pub struct AsmBlock {
    /// Input variable names to push onto the stack.
    pub inputs: Vec<String>,
    /// Output type name (e.g., "i64", "f64", "bool").
    pub output_type: Option<String>,
    /// Assembly instructions.
    pub body: Vec<AsmInstruction>,
    pub span: Span,
}

/// An instruction within an asm block.
#[derive(Debug, Clone)]
pub enum AsmInstruction {
    /// `__emit("OpName", args...)`
    Emit {
        op_name: String,
        args: Vec<AsmArg>,
        span: Span,
    },
    /// `__safepoint()`
    Safepoint { span: Span },
    /// `__gc_hint(size)`
    GcHint { size: i64, span: Span },
}

/// An argument to an asm instruction.
#[derive(Debug, Clone)]
pub enum AsmArg {
    Int(i64),
    Float(f64),
    String(String),
}
