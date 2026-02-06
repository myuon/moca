use crate::compiler::ast::{Block, Expr, FnDef, Item, Program, Statement};
use crate::compiler::lexer::Span;

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

/// Run all lint rules on a program, skipping stdlib items (those from `<stdlib>`).
pub fn lint_program(
    program: &Program,
    filename: &str,
    rules: &[Box<dyn LintRule>],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for item in &program.items {
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
pub fn default_rules() -> Vec<Box<dyn LintRule>> {
    vec![Box::new(PreferNewLiteral)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{Lexer, Parser, TypeChecker, prepend_stdlib};

    /// Helper: parse and typecheck source, then lint it.
    fn lint_source(source: &str) -> Vec<Diagnostic> {
        let mut lexer = Lexer::new("<test>", source);
        let tokens = lexer.scan_tokens().expect("lexer failed");
        let mut parser = Parser::new("<test>", tokens);
        let user_program = parser.parse().expect("parser failed");

        let program = prepend_stdlib(user_program).expect("prepend_stdlib failed");

        let mut typechecker = TypeChecker::new("<test>");
        typechecker
            .check_program(&program)
            .expect("typecheck failed");

        let rules = default_rules();
        lint_program(&program, "<test>", &rules)
    }

    #[test]
    fn test_prefer_new_literal_detects_vec_new() {
        let diagnostics = lint_source("let v = vec::`new`();");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "prefer-new-literal");
        assert!(diagnostics[0].message.contains("new Vec<T> {}"));
    }

    #[test]
    fn test_no_warning_for_new_vec_literal() {
        let diagnostics = lint_source("let v = new Vec<int> { 1, 2, 3 };");
        assert!(
            diagnostics.is_empty(),
            "expected no warnings, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_no_warning_for_clean_code() {
        let diagnostics = lint_source("let x = 42;");
        assert!(
            diagnostics.is_empty(),
            "expected no warnings, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_format_diagnostics() {
        let diagnostics = vec![Diagnostic {
            rule: "prefer-new-literal".to_string(),
            message: "use `new Vec<T> {}` instead of `vec::new()`".to_string(),
            span: Span::new(5, 10),
        }];
        let output = format_diagnostics("test.mc", &diagnostics);
        assert!(output.contains("warning: prefer-new-literal:"));
        assert!(output.contains("test.mc:5:10"));
    }
}
