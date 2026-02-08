use std::collections::HashMap;

use crate::compiler::ast::{Block, Expr, FnDef, Item, Program, Statement};
use crate::compiler::lexer::Span;
use crate::compiler::types::{Type, TypeAnnotation};

/// A single lint diagnostic.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub rule: String,
    pub message: String,
    pub span: Span,
}

/// Trait for lint rules. Each rule inspects the AST and produces diagnostics.
pub trait LintRule {
    /// The name of this rule (e.g., "prefer-new-literal").
    fn name(&self) -> &str;

    /// Check a single expression. Default implementation does nothing.
    fn check_expr(&self, _expr: &Expr, _diagnostics: &mut Vec<Diagnostic>) {}

    /// Check a single statement. Default implementation does nothing.
    fn check_statement(&self, _stmt: &Statement, _diagnostics: &mut Vec<Diagnostic>) {}
}

/// Run all lint rules on a program, skipping the first `skip_items` items (stdlib).
pub fn lint_program(
    program: &Program,
    filename: &str,
    rules: &[Box<dyn LintRule>],
    skip_items: usize,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for item in program.items.iter().skip(skip_items) {
        match item {
            Item::FnDef(fn_def) => {
                // Skip stdlib functions
                if is_stdlib_span(&fn_def.span, filename) {
                    continue;
                }
                lint_fn_def(fn_def, rules, &mut diagnostics);
            }
            Item::ImplBlock(impl_block) => {
                if is_stdlib_span(&impl_block.span, filename) {
                    continue;
                }
                for method in &impl_block.methods {
                    lint_fn_def(method, rules, &mut diagnostics);
                }
            }
            Item::Statement(stmt) => {
                lint_statement(stmt, rules, &mut diagnostics);
            }
            _ => {}
        }
    }

    diagnostics
}

/// Check if a span belongs to stdlib (line 0 is not possible in user code,
/// but we use filename comparison since stdlib uses `<stdlib>` as filename).
/// Since stdlib items are prepended, we filter by checking if the item
/// was defined in the user's file. Stdlib spans have filename `<stdlib>`.
/// After prepend_stdlib, all items share the same program but stdlib FnDefs
/// retain their original span line numbers. We rely on the fact that user
/// code items come after stdlib items and typically have reasonable line numbers.
/// A simpler approach: we skip items whose span is from before user code.
/// Actually, the cleanest approach is to accept the user filename and only
/// lint items that are from the user's file. But since spans don't carry
/// filename, we simply lint all items - the stdlib `vec::new` is defined
/// in an impl block, not called via AssociatedFunctionCall, so it won't
/// trigger false positives.
fn is_stdlib_span(_span: &Span, _filename: &str) -> bool {
    // Stdlib functions are added via prepend_stdlib, but they are function
    // definitions, not calls. Lint rules check for call patterns in user code,
    // so we don't need to skip them - stdlib defs won't match call patterns.
    // However, to be safe and avoid linting stdlib internals, we could track
    // which items are from stdlib. For now, we lint everything since our rules
    // only match specific call patterns that don't appear in stdlib definitions.
    false
}

fn lint_fn_def(fn_def: &FnDef, rules: &[Box<dyn LintRule>], diagnostics: &mut Vec<Diagnostic>) {
    lint_block(&fn_def.body, rules, diagnostics);
}

fn lint_block(block: &Block, rules: &[Box<dyn LintRule>], diagnostics: &mut Vec<Diagnostic>) {
    for stmt in &block.statements {
        lint_statement(stmt, rules, diagnostics);
    }
}

fn lint_statement(
    stmt: &Statement,
    rules: &[Box<dyn LintRule>],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for rule in rules {
        rule.check_statement(stmt, diagnostics);
    }

    match stmt {
        Statement::Let { init, .. } => {
            lint_expr(init, rules, diagnostics);
        }
        Statement::Assign { value, .. } => {
            lint_expr(value, rules, diagnostics);
        }
        Statement::IndexAssign {
            object,
            index,
            value,
            ..
        } => {
            lint_expr(object, rules, diagnostics);
            lint_expr(index, rules, diagnostics);
            lint_expr(value, rules, diagnostics);
        }
        Statement::FieldAssign { object, value, .. } => {
            lint_expr(object, rules, diagnostics);
            lint_expr(value, rules, diagnostics);
        }
        Statement::If {
            condition,
            then_block,
            else_block,
            ..
        } => {
            lint_expr(condition, rules, diagnostics);
            lint_block(then_block, rules, diagnostics);
            if let Some(else_block) = else_block {
                lint_block(else_block, rules, diagnostics);
            }
        }
        Statement::While {
            condition, body, ..
        } => {
            lint_expr(condition, rules, diagnostics);
            lint_block(body, rules, diagnostics);
        }
        Statement::ForIn { iterable, body, .. } => {
            lint_expr(iterable, rules, diagnostics);
            lint_block(body, rules, diagnostics);
        }
        Statement::Return { value, .. } => {
            if let Some(value) = value {
                lint_expr(value, rules, diagnostics);
            }
        }
        Statement::Throw { value, .. } => {
            lint_expr(value, rules, diagnostics);
        }
        Statement::Try {
            try_block,
            catch_block,
            ..
        } => {
            lint_block(try_block, rules, diagnostics);
            lint_block(catch_block, rules, diagnostics);
        }
        Statement::Expr { expr, .. } => {
            lint_expr(expr, rules, diagnostics);
        }
    }
}

fn lint_expr(expr: &Expr, rules: &[Box<dyn LintRule>], diagnostics: &mut Vec<Diagnostic>) {
    for rule in rules {
        rule.check_expr(expr, diagnostics);
    }

    match expr {
        Expr::Array { elements, .. } => {
            for el in elements {
                lint_expr(el, rules, diagnostics);
            }
        }
        Expr::Index { object, index, .. } => {
            lint_expr(object, rules, diagnostics);
            lint_expr(index, rules, diagnostics);
        }
        Expr::Field { object, .. } => {
            lint_expr(object, rules, diagnostics);
        }
        Expr::Unary { operand, .. } => {
            lint_expr(operand, rules, diagnostics);
        }
        Expr::Binary { left, right, .. } => {
            lint_expr(left, rules, diagnostics);
            lint_expr(right, rules, diagnostics);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                lint_expr(arg, rules, diagnostics);
            }
        }
        Expr::StructLiteral { fields, .. } => {
            for (_, field_expr) in fields {
                lint_expr(field_expr, rules, diagnostics);
            }
        }
        Expr::MethodCall { object, args, .. } => {
            lint_expr(object, rules, diagnostics);
            for arg in args {
                lint_expr(arg, rules, diagnostics);
            }
        }
        Expr::AssociatedFunctionCall { args, .. } => {
            for arg in args {
                lint_expr(arg, rules, diagnostics);
            }
        }
        Expr::NewLiteral { elements, .. } => {
            use crate::compiler::ast::NewLiteralElement;
            for el in elements {
                match el {
                    NewLiteralElement::Value(e) => lint_expr(e, rules, diagnostics),
                    NewLiteralElement::KeyValue { key, value } => {
                        lint_expr(key, rules, diagnostics);
                        lint_expr(value, rules, diagnostics);
                    }
                }
            }
        }
        Expr::Block {
            statements, expr, ..
        } => {
            for stmt in statements {
                lint_statement(stmt, rules, diagnostics);
            }
            lint_expr(expr, rules, diagnostics);
        }
        // Leaf expressions: no sub-expressions to recurse into
        Expr::Int { .. }
        | Expr::Float { .. }
        | Expr::Bool { .. }
        | Expr::Str { .. }
        | Expr::Nil { .. }
        | Expr::Ident { .. }
        | Expr::Asm(_) => {}
    }
}

/// Format lint diagnostics for display (similar to type error format).
pub fn format_diagnostics(filename: &str, diagnostics: &[Diagnostic]) -> String {
    let mut output = String::new();
    for diag in diagnostics {
        output.push_str(&format!(
            "warning: {}: {}\n  --> {}:{}:{}\n",
            diag.rule, diag.message, filename, diag.span.line, diag.span.column
        ));
    }
    output
}

/// Return the default set of lint rules.
pub fn default_rules(method_call_types: HashMap<Span, Type>) -> Vec<Box<dyn LintRule>> {
    vec![
        Box::new(PreferNewLiteral),
        Box::new(PreferIndexAccess { method_call_types }),
        Box::new(RedundantTypeAnnotation),
    ]
}

// ============================================================================
// Rules
// ============================================================================

/// Suggests using `new Vec<T> {}` instead of `vec::new()`.
pub struct PreferNewLiteral;

impl LintRule for PreferNewLiteral {
    fn name(&self) -> &str {
        "prefer-new-literal"
    }

    fn check_expr(&self, expr: &Expr, diagnostics: &mut Vec<Diagnostic>) {
        if let Expr::AssociatedFunctionCall {
            type_name,
            function,
            args,
            span,
            ..
        } = expr
            && type_name == "vec"
            && function == "new"
            && args.is_empty()
        {
            diagnostics.push(Diagnostic {
                rule: self.name().to_string(),
                message: "use `new Vec<T> {}` instead of `vec::\\`new\\`()`".to_string(),
                span: *span,
            });
        }
    }
}

/// Suggests using `obj[index]` instead of `obj.get(index)` and
/// `obj[index] = value` instead of `obj.set(index, value)` / `obj.put(key, value)`
/// for vec and map types.
pub struct PreferIndexAccess {
    method_call_types: HashMap<Span, Type>,
}

impl PreferIndexAccess {
    fn is_vec_or_map(&self, span: &Span) -> bool {
        match self.method_call_types.get(span) {
            Some(Type::Vector(_)) | Some(Type::Map(_, _)) => true,
            Some(Type::GenericStruct { name, .. }) => name == "Vec" || name == "Map",
            _ => false,
        }
    }
}

impl LintRule for PreferIndexAccess {
    fn name(&self) -> &str {
        "prefer-index-access"
    }

    fn check_expr(&self, expr: &Expr, diagnostics: &mut Vec<Diagnostic>) {
        if let Expr::MethodCall {
            method, args, span, ..
        } = expr
        {
            if !self.is_vec_or_map(span) {
                return;
            }

            match method.as_str() {
                "get" if args.len() == 1 => {
                    diagnostics.push(Diagnostic {
                        rule: self.name().to_string(),
                        message: "use `[]` indexing instead of `.get()`".to_string(),
                        span: *span,
                    });
                }
                "set" if args.len() == 2 => {
                    diagnostics.push(Diagnostic {
                        rule: self.name().to_string(),
                        message: "use `[] =` indexing instead of `.set()`".to_string(),
                        span: *span,
                    });
                }
                "put" if args.len() == 2 => {
                    diagnostics.push(Diagnostic {
                        rule: self.name().to_string(),
                        message: "use `[] =` indexing instead of `.put()`".to_string(),
                        span: *span,
                    });
                }
                _ => {}
            }
        }
    }
}

/// Warns against redundant type annotations on let/var when the type is
/// already specified by a `new` literal (e.g., `let v: Vec<int> = new Vec<int> {}`).
pub struct RedundantTypeAnnotation;

/// Check if a TypeAnnotation matches the type specified by a NewLiteral's
/// type_name and type_args.
fn type_annotation_matches_new_literal(
    annotation: &TypeAnnotation,
    type_name: &str,
    type_args: &[TypeAnnotation],
) -> bool {
    match annotation {
        TypeAnnotation::Named(name) => name == type_name && type_args.is_empty(),
        TypeAnnotation::Vec(inner) => {
            type_name == "Vec" && type_args.len() == 1 && type_args[0] == **inner
        }
        TypeAnnotation::Map(key, val) => {
            type_name == "Map"
                && type_args.len() == 2
                && type_args[0] == **key
                && type_args[1] == **val
        }
        TypeAnnotation::Generic {
            name,
            type_args: ann_args,
        } => name == type_name && ann_args == type_args,
        _ => false,
    }
}

fn format_new_type(type_name: &str, type_args: &[TypeAnnotation]) -> String {
    if type_args.is_empty() {
        type_name.to_string()
    } else {
        let args: Vec<String> = type_args.iter().map(|a| a.to_string()).collect();
        format!("{}<{}>", type_name, args.join(", "))
    }
}

impl LintRule for RedundantTypeAnnotation {
    fn name(&self) -> &str {
        "redundant-type-annotation"
    }

    fn check_statement(&self, stmt: &Statement, diagnostics: &mut Vec<Diagnostic>) {
        if let Statement::Let {
            type_annotation: Some(annotation),
            init:
                Expr::NewLiteral {
                    type_name,
                    type_args,
                    ..
                },
            span,
            ..
        } = stmt
            && type_annotation_matches_new_literal(annotation, type_name, type_args)
        {
            diagnostics.push(Diagnostic {
                rule: self.name().to_string(),
                message: format!(
                    "remove redundant type annotation; type is already specified by `new {}`",
                    format_new_type(type_name, type_args)
                ),
                span: *span,
            });
        }
    }
}
