use crate::compiler::lexer::Span;
use crate::compiler::types::{Type, TypeAnnotation};

/// A complete program consisting of items (functions and statements).
#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
}

/// Top-level items in a program.
#[derive(Debug, Clone)]
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
    pub fields: Vec<StructField>,
    pub span: Span,
}

/// An impl block containing methods for a struct.
#[derive(Debug, Clone)]
pub struct ImplBlock {
    pub struct_name: String,
    pub methods: Vec<FnDef>,
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
    pub params: Vec<Param>,
    pub return_type: Option<TypeAnnotation>,
    pub body: Block,
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
pub enum Statement {
    Let {
        name: String,
        mutable: bool,
        type_annotation: Option<TypeAnnotation>,
        init: Expr,
        span: Span,
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
}

/// Expressions in the language.
#[derive(Debug, Clone)]
pub enum Expr {
    Int {
        value: i64,
        span: Span,
    },
    Float {
        value: f64,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    Str {
        value: String,
        span: Span,
    },
    Nil {
        span: Span,
    },
    Ident {
        name: String,
        span: Span,
    },
    Array {
        elements: Vec<Expr>,
        span: Span,
    },
    Object {
        fields: Vec<(String, Expr)>,
        span: Span,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
        span: Span,
        /// Type of the object (set by typechecker for codegen)
        object_type: Option<Type>,
    },
    Field {
        object: Box<Expr>,
        field: String,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    Call {
        callee: String,
        args: Vec<Expr>,
        span: Span,
    },
    /// Struct literal: `Point { x: 1, y: 2 }`
    StructLiteral {
        name: String,
        fields: Vec<(String, Expr)>,
        span: Span,
    },
    /// Method call: `obj.method(args)`
    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
        span: Span,
    },
    /// Inline assembly block: `asm(inputs) -> type { ... }`
    Asm(AsmBlock),
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
            Expr::Object { span, .. } => *span,
            Expr::Index { span, .. } => *span,
            Expr::Field { span, .. } => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::StructLiteral { span, .. } => *span,
            Expr::MethodCall { span, .. } => *span,
            Expr::Asm(asm_block) => asm_block.span,
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
