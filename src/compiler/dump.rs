//! Pretty-printer for AST and other intermediate representations.
//!
//! This module provides human-readable output for debugging the compiler pipeline.

use crate::compiler::ast::{
    BinaryOp, Block, Expr, FnDef, ImplBlock, Import, Item, Param, Program, Statement, StructDef,
    UnaryOp,
};
use crate::compiler::resolver::{
    ResolvedExpr, ResolvedFunction, ResolvedProgram, ResolvedStatement, ResolvedStruct,
};
use crate::compiler::types::Type;
use crate::vm::microop::{CmpCond, MicroOp, VReg};
use crate::vm::microop_converter;
use crate::vm::{Chunk, Function, Op};
use std::collections::HashMap;

/// A span-based key for looking up types.
/// Uses (file_id, start_offset) as a unique identifier for expressions.
pub type ExprTypeMap = HashMap<(u32, u32), Type>;

/// Pretty-printer for the AST with optional type information.
pub struct AstPrinter<'a> {
    output: String,
    indent: usize,
    type_map: Option<&'a ExprTypeMap>,
}

impl<'a> AstPrinter<'a> {
    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
            type_map: None,
        }
    }

    pub fn with_types(mut self, type_map: &'a ExprTypeMap) -> Self {
        self.type_map = Some(type_map);
        self
    }

    pub fn print_program(&mut self, program: &Program) -> &str {
        self.writeln("Program");
        self.indent += 1;
        for (i, item) in program.items.iter().enumerate() {
            let is_last = i == program.items.len() - 1;
            self.print_item(item, is_last);
        }
        self.indent -= 1;
        &self.output
    }

    fn print_item(&mut self, item: &Item, is_last: bool) {
        let prefix = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };
        self.write_indent();
        match item {
            Item::Import(import) => self.print_import(import, prefix),
            Item::FnDef(fn_def) => self.print_fn_def(fn_def, prefix, child_prefix),
            Item::StructDef(struct_def) => self.print_struct_def(struct_def, prefix, child_prefix),
            Item::ImplBlock(impl_block) => self.print_impl_block(impl_block, prefix, child_prefix),
            Item::Statement(stmt) => self.print_statement(stmt, prefix, child_prefix),
        }
    }

    fn print_import(&mut self, import: &Import, prefix: &str) {
        let path = import.path.join(".");
        let relative = if import.relative { "(relative) " } else { "" };
        self.write_prefixed(prefix, &format!("Import: {}{}", relative, path));
        self.newline();
    }

    fn print_fn_def(&mut self, fn_def: &FnDef, prefix: &str, parent_prefix: &str) {
        let params = fn_def
            .params
            .iter()
            .map(|p| self.format_param(p))
            .collect::<Vec<_>>()
            .join(", ");

        let ret_type = fn_def
            .return_type
            .as_ref()
            .map(|t| format!(" -> {}", t))
            .unwrap_or_default();

        self.write_prefixed(
            prefix,
            &format!("FnDef: {}({}){}", fn_def.name, params, ret_type),
        );
        self.newline();

        self.print_block_contents(&fn_def.body, parent_prefix);
    }

    fn print_struct_def(&mut self, struct_def: &StructDef, prefix: &str, parent_prefix: &str) {
        self.write_prefixed(prefix, &format!("StructDef: {}", struct_def.name));
        self.newline();

        for (i, field) in struct_def.fields.iter().enumerate() {
            let field_is_last = i == struct_def.fields.len() - 1;
            let field_prefix = if field_is_last {
                "└── "
            } else {
                "├── "
            };
            self.write_indent_with(parent_prefix);
            self.write(&format!(
                "{}Field: {}: {}",
                field_prefix, field.name, field.type_annotation
            ));
            self.newline();
        }
    }

    fn print_impl_block(&mut self, impl_block: &ImplBlock, prefix: &str, parent_prefix: &str) {
        self.write_prefixed(prefix, &format!("ImplBlock: {}", impl_block.struct_name));
        self.newline();

        for (i, method) in impl_block.methods.iter().enumerate() {
            let method_is_last = i == impl_block.methods.len() - 1;
            let method_prefix = if method_is_last {
                "└── "
            } else {
                "├── "
            };
            let method_child_prefix = if method_is_last {
                format!("{}    ", parent_prefix)
            } else {
                format!("{}│   ", parent_prefix)
            };
            self.write_indent_with(parent_prefix);
            self.print_fn_def(method, method_prefix, &method_child_prefix);
        }
    }

    fn print_statement(&mut self, stmt: &Statement, prefix: &str, parent_prefix: &str) {
        match stmt {
            Statement::Let {
                name,
                type_annotation,
                init,
                ..
            } => {
                let type_str = type_annotation
                    .as_ref()
                    .map(|t| format!(": {}", t))
                    .unwrap_or_default();
                self.write_prefixed(prefix, &format!("Let: {}{}", name, type_str));
                self.newline();
                self.write_indent_with(parent_prefix);
                self.print_expr(init, "└── ", true, parent_prefix);
            }

            Statement::Assign { name, value, .. } => {
                self.write_prefixed(prefix, &format!("Assign: {}", name));
                self.newline();
                self.write_indent_with(parent_prefix);
                self.print_expr(value, "└── ", true, parent_prefix);
            }

            Statement::IndexAssign {
                object,
                index,
                value,
                ..
            } => {
                self.write_prefixed(prefix, "IndexAssign");
                self.newline();
                self.write_indent_with(parent_prefix);
                self.print_expr(object, "├── object: ", false, parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(index, "├── index: ", false, parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(value, "└── value: ", true, parent_prefix);
            }

            Statement::FieldAssign {
                object,
                field,
                value,
                ..
            } => {
                self.write_prefixed(prefix, &format!("FieldAssign: .{}", field));
                self.newline();
                self.write_indent_with(parent_prefix);
                self.print_expr(object, "├── object: ", false, parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(value, "└── value: ", true, parent_prefix);
            }

            Statement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                self.write_prefixed(prefix, "If");
                self.newline();
                self.write_indent_with(parent_prefix);
                self.print_expr(condition, "├── condition: ", false, parent_prefix);
                self.write_indent_with(parent_prefix);
                let has_else = else_block.is_some();
                let then_prefix = if has_else { "├── " } else { "└── " };
                self.write(&format!("{}then:", then_prefix));
                self.newline();
                let then_child = if has_else {
                    format!("{}│   ", parent_prefix)
                } else {
                    format!("{}    ", parent_prefix)
                };
                self.print_block_contents(then_block, &then_child);
                if let Some(else_blk) = else_block {
                    self.write_indent_with(parent_prefix);
                    self.write("└── else:");
                    self.newline();
                    let else_child = format!("{}    ", parent_prefix);
                    self.print_block_contents(else_blk, &else_child);
                }
            }

            Statement::While {
                condition, body, ..
            } => {
                self.write_prefixed(prefix, "While");
                self.newline();
                self.write_indent_with(parent_prefix);
                self.print_expr(condition, "├── condition: ", false, parent_prefix);
                self.write_indent_with(parent_prefix);
                self.write("└── body:");
                self.newline();
                let body_child = format!("{}    ", parent_prefix);
                self.print_block_contents(body, &body_child);
            }

            Statement::ForIn {
                var,
                iterable,
                body,
                ..
            } => {
                self.write_prefixed(prefix, &format!("ForIn: {}", var));
                self.newline();
                self.write_indent_with(parent_prefix);
                self.print_expr(iterable, "├── iterable: ", false, parent_prefix);
                self.write_indent_with(parent_prefix);
                self.write("└── body:");
                self.newline();
                let body_child = format!("{}    ", parent_prefix);
                self.print_block_contents(body, &body_child);
            }

            Statement::ForRange {
                var,
                start,
                end,
                inclusive,
                body,
                ..
            } => {
                let op = if *inclusive { "..=" } else { ".." };
                self.write_prefixed(prefix, &format!("ForRange: {}{}", var, op));
                self.newline();
                self.write_indent_with(parent_prefix);
                self.print_expr(start, "├── start: ", false, parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(end, "├── end: ", false, parent_prefix);
                self.write_indent_with(parent_prefix);
                self.write("└── body:");
                self.newline();
                let body_child = format!("{}    ", parent_prefix);
                self.print_block_contents(body, &body_child);
            }

            Statement::Return { value, .. } => {
                self.write_prefixed(prefix, "Return");
                self.newline();
                if let Some(expr) = value {
                    self.write_indent_with(parent_prefix);
                    self.print_expr(expr, "└── ", true, parent_prefix);
                }
            }

            Statement::Throw { value, .. } => {
                self.write_prefixed(prefix, "Throw");
                self.newline();
                self.write_indent_with(parent_prefix);
                self.print_expr(value, "└── ", true, parent_prefix);
            }

            Statement::Try {
                try_block,
                catch_var,
                catch_block,
                ..
            } => {
                self.write_prefixed(prefix, "Try");
                self.newline();
                self.write_indent_with(parent_prefix);
                self.write("├── try:");
                self.newline();
                let try_child = format!("{}│   ", parent_prefix);
                self.print_block_contents(try_block, &try_child);
                self.write_indent_with(parent_prefix);
                self.write(&format!("└── catch ({}):", catch_var));
                self.newline();
                let catch_child = format!("{}    ", parent_prefix);
                self.print_block_contents(catch_block, &catch_child);
            }

            Statement::Expr { expr, .. } => {
                self.write_prefixed(prefix, "Expr");
                self.newline();
                self.write_indent_with(parent_prefix);
                self.print_expr(expr, "└── ", true, parent_prefix);
            }

            Statement::Const { name, init, .. } => {
                self.write_prefixed(prefix, &format!("Const: {}", name));
                self.newline();
                self.write_indent_with(parent_prefix);
                self.print_expr(init, "└── ", true, parent_prefix);
            }
        }
    }

    fn print_expr(&mut self, expr: &Expr, prefix: &str, is_last: bool, parent_prefix: &str) {
        let child_prefix = if is_last {
            format!("{}    ", parent_prefix)
        } else {
            format!("{}│   ", parent_prefix)
        };

        match expr {
            Expr::Int { value, .. } => {
                self.write(&format!("{}Int: {}", prefix, value));
                self.write_type_suffix(expr);
                self.newline();
            }

            Expr::Float { value, .. } => {
                self.write(&format!("{}Float: {}", prefix, value));
                self.write_type_suffix(expr);
                self.newline();
            }

            Expr::Bool { value, .. } => {
                self.write(&format!("{}Bool: {}", prefix, value));
                self.write_type_suffix(expr);
                self.newline();
            }

            Expr::Str { value, .. } => {
                let escaped = value.replace('\n', "\\n").replace('\t', "\\t");
                self.write(&format!("{}String: \"{}\"", prefix, escaped));
                self.write_type_suffix(expr);
                self.newline();
            }

            Expr::Nil { .. } => {
                self.write(&format!("{}Nil", prefix));
                self.write_type_suffix(expr);
                self.newline();
            }

            Expr::Ident { name, .. } => {
                self.write(&format!("{}Ident: {}", prefix, name));
                self.write_type_suffix(expr);
                self.newline();
            }

            Expr::Array { elements, .. } => {
                self.write(&format!("{}Array[{}]", prefix, elements.len()));
                self.write_type_suffix(expr);
                self.newline();
                for (i, elem) in elements.iter().enumerate() {
                    let elem_is_last = i == elements.len() - 1;
                    let elem_prefix = if elem_is_last {
                        "└── "
                    } else {
                        "├── "
                    };
                    self.write_indent_with(&child_prefix);
                    self.print_expr(elem, elem_prefix, elem_is_last, &child_prefix);
                }
            }

            Expr::Index { object, index, .. } => {
                self.write(&format!("{}Index", prefix));
                self.write_type_suffix(expr);
                self.newline();
                self.write_indent_with(&child_prefix);
                self.print_expr(object, "├── object: ", false, &child_prefix);
                self.write_indent_with(&child_prefix);
                self.print_expr(index, "└── index: ", true, &child_prefix);
            }

            Expr::Field { object, field, .. } => {
                self.write(&format!("{}Field: .{}", prefix, field));
                self.write_type_suffix(expr);
                self.newline();
                self.write_indent_with(&child_prefix);
                self.print_expr(object, "└── ", true, &child_prefix);
            }

            Expr::Unary { op, operand, .. } => {
                let op_str = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                };
                self.write(&format!("{}Unary: {}", prefix, op_str));
                self.write_type_suffix(expr);
                self.newline();
                self.write_indent_with(&child_prefix);
                self.print_expr(operand, "└── ", true, &child_prefix);
            }

            Expr::Binary {
                op, left, right, ..
            } => {
                let op_str = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Mod => "%",
                    BinaryOp::Eq => "==",
                    BinaryOp::Ne => "!=",
                    BinaryOp::Lt => "<",
                    BinaryOp::Le => "<=",
                    BinaryOp::Gt => ">",
                    BinaryOp::Ge => ">=",
                    BinaryOp::And => "&&",
                    BinaryOp::Or => "||",
                };
                self.write(&format!("{}Binary: {}", prefix, op_str));
                self.write_type_suffix(expr);
                self.newline();
                self.write_indent_with(&child_prefix);
                self.print_expr(left, "├── ", false, &child_prefix);
                self.write_indent_with(&child_prefix);
                self.print_expr(right, "└── ", true, &child_prefix);
            }

            Expr::Call { callee, args, .. } => {
                self.write(&format!("{}Call: {}({})", prefix, callee, args.len()));
                self.write_type_suffix(expr);
                self.newline();
                for (i, arg) in args.iter().enumerate() {
                    let arg_is_last = i == args.len() - 1;
                    let arg_prefix = if arg_is_last {
                        "└── "
                    } else {
                        "├── "
                    };
                    self.write_indent_with(&child_prefix);
                    self.print_expr(arg, arg_prefix, arg_is_last, &child_prefix);
                }
            }

            Expr::StructLiteral { name, fields, .. } => {
                self.write(&format!("{}StructLiteral: {}", prefix, name));
                self.write_type_suffix(expr);
                self.newline();
                for (i, (field_name, value)) in fields.iter().enumerate() {
                    let field_is_last = i == fields.len() - 1;
                    let field_prefix = if field_is_last {
                        "└── "
                    } else {
                        "├── "
                    };
                    self.write_indent_with(&child_prefix);
                    self.write(&format!("{}{}: ", field_prefix, field_name));
                    self.newline();
                    let field_child = if field_is_last {
                        format!("{}    ", child_prefix)
                    } else {
                        format!("{}│   ", child_prefix)
                    };
                    self.write_indent_with(&field_child);
                    self.print_expr(value, "└── ", true, &field_child);
                }
            }

            Expr::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                self.write(&format!(
                    "{}MethodCall: .{}({})",
                    prefix,
                    method,
                    args.len()
                ));
                self.write_type_suffix(expr);
                self.newline();
                let has_args = !args.is_empty();
                let obj_prefix = if has_args { "├── " } else { "└── " };
                self.write_indent_with(&child_prefix);
                self.print_expr(
                    object,
                    &format!("{}object: ", obj_prefix),
                    !has_args,
                    &child_prefix,
                );
                for (i, arg) in args.iter().enumerate() {
                    let arg_is_last = i == args.len() - 1;
                    let arg_prefix = if arg_is_last {
                        "└── "
                    } else {
                        "├── "
                    };
                    self.write_indent_with(&child_prefix);
                    self.print_expr(
                        arg,
                        &format!("{}arg: ", arg_prefix),
                        arg_is_last,
                        &child_prefix,
                    );
                }
            }
            Expr::AssociatedFunctionCall {
                type_name,
                function,
                args,
                ..
            } => {
                self.write(&format!(
                    "{}AssociatedFunctionCall: {}::{}({})",
                    prefix,
                    type_name,
                    function,
                    args.len()
                ));
                self.write_type_suffix(expr);
                self.newline();
                for (i, arg) in args.iter().enumerate() {
                    let arg_is_last = i == args.len() - 1;
                    let arg_prefix = if arg_is_last {
                        "└── "
                    } else {
                        "├── "
                    };
                    self.write_indent_with(&child_prefix);
                    self.print_expr(
                        arg,
                        &format!("{}arg: ", arg_prefix),
                        arg_is_last,
                        &child_prefix,
                    );
                }
            }
            Expr::Asm(asm_block) => {
                let output_str = asm_block
                    .output_type
                    .as_ref()
                    .map(|t| format!(" -> {}", t))
                    .unwrap_or_default();
                self.write(&format!(
                    "{}AsmBlock({} inputs, {} instructions){}",
                    prefix,
                    asm_block.inputs.len(),
                    asm_block.body.len(),
                    output_str
                ));
                self.write_type_suffix(expr);
                self.newline();
            }
            Expr::NewLiteral {
                type_name,
                type_args,
                elements,
                ..
            } => {
                let type_args_str = if type_args.is_empty() {
                    String::new()
                } else {
                    format!(
                        "<{}>",
                        type_args
                            .iter()
                            .map(|t| format!("{:?}", t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                self.write(&format!(
                    "{}NewLiteral: {}{} [{} elements]",
                    prefix,
                    type_name,
                    type_args_str,
                    elements.len()
                ));
                self.write_type_suffix(expr);
                self.newline();
            }
            Expr::Block {
                statements,
                expr: block_expr,
                ..
            } => {
                self.write(&format!("{}Block: [{} stmts]", prefix, statements.len()));
                self.newline();
                for stmt in statements.iter() {
                    let stmt_prefix = "├── ";
                    let child_prefix_str = format!("{}│   ", child_prefix);
                    self.write_indent_with(&child_prefix);
                    self.print_statement(stmt, stmt_prefix, &child_prefix_str);
                }
                self.write_indent_with(&child_prefix);
                self.print_expr(
                    block_expr,
                    "└── ",
                    statements.is_empty(),
                    &format!("{}    ", child_prefix),
                );
            }

            Expr::Lambda {
                params,
                return_type,
                body,
                ..
            } => {
                let params_str = params
                    .iter()
                    .map(|p| self.format_param(p))
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret_str = return_type
                    .as_ref()
                    .map(|t| format!(" -> {}", t))
                    .unwrap_or_default();
                self.write(&format!("{}Lambda: fun({}){}", prefix, params_str, ret_str));
                self.write_type_suffix(expr);
                self.newline();
                let body_prefix = format!("{}    ", child_prefix);
                self.print_block_contents(body, &body_prefix);
            }

            Expr::CallExpr { callee, args, .. } => {
                self.write(&format!("{}CallExpr({})", prefix, args.len()));
                self.write_type_suffix(expr);
                self.newline();
                let has_args = !args.is_empty();
                let callee_prefix = if has_args { "├── " } else { "└── " };
                self.write_indent_with(&child_prefix);
                self.print_expr(
                    callee,
                    &format!("{}callee: ", callee_prefix),
                    !has_args,
                    &child_prefix,
                );
                for (i, arg) in args.iter().enumerate() {
                    let arg_is_last = i == args.len() - 1;
                    let arg_prefix = if arg_is_last {
                        "└── "
                    } else {
                        "├── "
                    };
                    self.write_indent_with(&child_prefix);
                    self.print_expr(
                        arg,
                        &format!("{}arg: ", arg_prefix),
                        arg_is_last,
                        &child_prefix,
                    );
                }
            }

            Expr::StringInterpolation { parts, .. } => {
                self.write(&format!(
                    "{}StringInterpolation[{} parts]",
                    prefix,
                    parts.len()
                ));
                self.write_type_suffix(expr);
                self.newline();
                for (i, part) in parts.iter().enumerate() {
                    let part_is_last = i == parts.len() - 1;
                    let part_prefix = if part_is_last {
                        "└── "
                    } else {
                        "├── "
                    };
                    match part {
                        crate::compiler::ast::StringInterpPart::Literal(s) => {
                            let escaped = s.replace('\n', "\\n").replace('\t', "\\t");
                            self.write_indent_with(&child_prefix);
                            self.write(&format!("{}Literal: \"{}\"", part_prefix, escaped));
                            self.newline();
                        }
                        crate::compiler::ast::StringInterpPart::Expr(e) => {
                            self.write_indent_with(&child_prefix);
                            self.print_expr(
                                e,
                                &format!("{}Expr: ", part_prefix),
                                part_is_last,
                                &child_prefix,
                            );
                        }
                    }
                }
            }
        }
    }

    fn print_block_contents(&mut self, block: &Block, parent_prefix: &str) {
        for (i, stmt) in block.statements.iter().enumerate() {
            let is_last = i == block.statements.len() - 1;
            let stmt_prefix = if is_last { "└── " } else { "├── " };
            let child_prefix = if is_last {
                format!("{}    ", parent_prefix)
            } else {
                format!("{}│   ", parent_prefix)
            };
            self.write_indent_with(parent_prefix);
            self.print_statement(stmt, stmt_prefix, &child_prefix);
        }
    }

    fn format_param(&self, param: &Param) -> String {
        match &param.type_annotation {
            Some(ann) => format!("{}: {}", param.name, ann),
            None => param.name.clone(),
        }
    }

    fn write_type_suffix(&mut self, _expr: &Expr) {
        // Type information will be added when type_map is available
        // For now, this is a placeholder
        // TODO: Look up type from type_map using expr.span()
    }

    fn write(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn writeln(&mut self, s: &str) {
        self.write_indent();
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn newline(&mut self) {
        self.output.push('\n');
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("  ");
        }
    }

    fn write_indent_with(&mut self, prefix: &str) {
        for _ in 0..self.indent {
            self.output.push_str("  ");
        }
        self.output.push_str(prefix);
    }

    fn write_prefixed(&mut self, prefix: &str, content: &str) {
        self.write(prefix);
        self.write(content);
    }
}

impl Default for AstPrinter<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Format a program as a pretty-printed AST string.
pub fn format_ast(program: &Program) -> String {
    let mut printer = AstPrinter::new();
    printer.print_program(program).to_string()
}

/// Format a program as a pretty-printed AST string with type information.
pub fn format_ast_with_types(program: &Program, type_map: &ExprTypeMap) -> String {
    let mut printer = AstPrinter::new().with_types(type_map);
    printer.print_program(program).to_string()
}

// ============================================================================
// ResolvedProgram Printer
// ============================================================================

/// Pretty-printer for the resolved program.
pub struct ResolvedProgramPrinter {
    output: String,
    indent: usize,
}

impl ResolvedProgramPrinter {
    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
        }
    }

    pub fn print(&mut self, program: &ResolvedProgram) -> &str {
        self.writeln("ResolvedProgram");
        self.indent += 1;

        // Print structs
        if !program.structs.is_empty() {
            self.writeln("Structs:");
            self.indent += 1;
            for (i, s) in program.structs.iter().enumerate() {
                let is_last = i == program.structs.len() - 1;
                self.print_struct(s, i, is_last);
            }
            self.indent -= 1;
        }

        // Print functions
        if !program.functions.is_empty() {
            self.writeln("Functions:");
            self.indent += 1;
            for (i, func) in program.functions.iter().enumerate() {
                let is_last = i == program.functions.len() - 1;
                self.print_function(func, i, is_last);
            }
            self.indent -= 1;
        }

        // Print main body
        if !program.main_body.is_empty() {
            self.writeln(&format!("Main [locals: {}]:", program.main_locals_count));
            self.indent += 1;
            for (i, stmt) in program.main_body.iter().enumerate() {
                let is_last = i == program.main_body.len() - 1;
                let prefix = if is_last { "└── " } else { "├── " };
                let child_prefix = if is_last { "    " } else { "│   " };
                self.print_statement(stmt, prefix, child_prefix);
            }
            self.indent -= 1;
        }

        self.indent -= 1;
        &self.output
    }

    fn print_struct(&mut self, s: &ResolvedStruct, index: usize, is_last: bool) {
        let prefix = if is_last { "└── " } else { "├── " };
        self.write_indent();
        self.write(&format!("{}[{}] {} {{ ", prefix, index, s.name));
        for (i, field) in s.fields.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(field);
        }
        self.write(" }");
        self.newline();
    }

    fn print_function(&mut self, func: &ResolvedFunction, index: usize, is_last: bool) {
        let prefix = if is_last { "└── " } else { "├── " };
        let func_child_prefix = if is_last { "    " } else { "│   " };

        // Print function signature
        let params: Vec<String> = func
            .params
            .iter()
            .enumerate()
            .map(|(i, name)| format!("{} -> slot:{}", name, i))
            .collect();
        self.write_indent();
        self.write(&format!(
            "{}[{}] {}({}) [locals: {}]",
            prefix,
            index,
            func.name,
            params.join(", "),
            func.locals_count
        ));
        self.newline();

        // Print body
        self.indent += 1;
        for (i, stmt) in func.body.iter().enumerate() {
            let stmt_is_last = i == func.body.len() - 1;
            let stmt_prefix = if stmt_is_last {
                "└── "
            } else {
                "├── "
            };
            let stmt_child_prefix = if stmt_is_last {
                format!("{}    ", func_child_prefix)
            } else {
                format!("{}│   ", func_child_prefix)
            };
            self.write_indent_with(func_child_prefix);
            self.print_statement(stmt, stmt_prefix, &stmt_child_prefix);
        }
        self.indent -= 1;
    }

    fn print_statement(&mut self, stmt: &ResolvedStatement, prefix: &str, parent_prefix: &str) {
        match stmt {
            ResolvedStatement::Let { slot, init } => {
                self.write(&format!("{}Let slot:{}", prefix, slot));
                self.newline();
                self.write_indent_with(parent_prefix);
                let expr_child = format!("{}    ", parent_prefix);
                self.print_expr(init, "└── init: ", &expr_child);
            }

            ResolvedStatement::Assign { slot, value } => {
                self.write(&format!("{}Assign slot:{}", prefix, slot));
                self.newline();
                self.write_indent_with(parent_prefix);
                let expr_child = format!("{}    ", parent_prefix);
                self.print_expr(value, "└── value: ", &expr_child);
            }

            ResolvedStatement::IndexAssign {
                object,
                index,
                value,
                ..
            } => {
                self.write(&format!("{}IndexAssign", prefix));
                self.newline();
                let obj_child = format!("{}│   ", parent_prefix);
                let val_child = format!("{}    ", parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(object, "├── object: ", &obj_child);
                self.write_indent_with(parent_prefix);
                self.print_expr(index, "├── index: ", &obj_child);
                self.write_indent_with(parent_prefix);
                self.print_expr(value, "└── value: ", &val_child);
            }

            ResolvedStatement::FieldAssign {
                object,
                field,
                value,
            } => {
                self.write(&format!("{}FieldAssign .{}", prefix, field));
                self.newline();
                let obj_child = format!("{}│   ", parent_prefix);
                let val_child = format!("{}    ", parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(object, "├── object: ", &obj_child);
                self.write_indent_with(parent_prefix);
                self.print_expr(value, "└── value: ", &val_child);
            }

            ResolvedStatement::If {
                condition,
                then_block,
                else_block,
            } => {
                self.write(&format!("{}If", prefix));
                self.newline();
                let cond_child = format!("{}│   ", parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(condition, "├── condition: ", &cond_child);
                self.write_indent_with(parent_prefix);
                let has_else = else_block.is_some();
                let then_prefix = if has_else { "├── " } else { "└── " };
                self.write(&format!("{}then:", then_prefix));
                self.newline();
                let then_child = if has_else {
                    format!("{}│   ", parent_prefix)
                } else {
                    format!("{}    ", parent_prefix)
                };
                self.print_block(then_block, &then_child);
                if let Some(else_blk) = else_block {
                    self.write_indent_with(parent_prefix);
                    self.write("└── else:");
                    self.newline();
                    let else_child = format!("{}    ", parent_prefix);
                    self.print_block(else_blk, &else_child);
                }
            }

            ResolvedStatement::While { condition, body } => {
                self.write(&format!("{}While", prefix));
                self.newline();
                let cond_child = format!("{}│   ", parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(condition, "├── condition: ", &cond_child);
                self.write_indent_with(parent_prefix);
                self.write("└── body:");
                self.newline();
                let body_child = format!("{}    ", parent_prefix);
                self.print_block(body, &body_child);
            }

            ResolvedStatement::ForIn {
                slot,
                iterable,
                body,
            } => {
                self.write(&format!("{}ForIn slot:{}", prefix, slot));
                self.newline();
                let iter_child = format!("{}│   ", parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(iterable, "├── iterable: ", &iter_child);
                self.write_indent_with(parent_prefix);
                self.write("└── body:");
                self.newline();
                let body_child = format!("{}    ", parent_prefix);
                self.print_block(body, &body_child);
            }

            ResolvedStatement::Return { value } => {
                self.write(&format!("{}Return", prefix));
                self.newline();
                if let Some(v) = value {
                    self.write_indent_with(parent_prefix);
                    let expr_child = format!("{}    ", parent_prefix);
                    self.print_expr(v, "└── ", &expr_child);
                }
            }

            ResolvedStatement::Throw { value } => {
                self.write(&format!("{}Throw", prefix));
                self.newline();
                self.write_indent_with(parent_prefix);
                let expr_child = format!("{}    ", parent_prefix);
                self.print_expr(value, "└── ", &expr_child);
            }

            ResolvedStatement::Try {
                try_block,
                catch_slot,
                catch_block,
            } => {
                self.write(&format!("{}Try", prefix));
                self.newline();
                self.write_indent_with(parent_prefix);
                self.write("├── try:");
                self.newline();
                let try_child = format!("{}│   ", parent_prefix);
                self.print_block(try_block, &try_child);
                self.write_indent_with(parent_prefix);
                self.write(&format!("└── catch slot:{}:", catch_slot));
                self.newline();
                let catch_child = format!("{}    ", parent_prefix);
                self.print_block(catch_block, &catch_child);
            }

            ResolvedStatement::Expr { expr } => {
                self.write(&format!("{}Expr", prefix));
                self.newline();
                self.write_indent_with(parent_prefix);
                let expr_child = format!("{}    ", parent_prefix);
                self.print_expr(expr, "└── ", &expr_child);
            }

            ResolvedStatement::RefCellStore { slot, value } => {
                self.write(&format!("{}RefCellStore slot:{}", prefix, slot));
                self.newline();
                self.write_indent_with(parent_prefix);
                let expr_child = format!("{}    ", parent_prefix);
                self.print_expr(value, "└── value: ", &expr_child);
            }
        }
    }

    fn print_block(&mut self, block: &[ResolvedStatement], parent_prefix: &str) {
        for (i, stmt) in block.iter().enumerate() {
            let is_last = i == block.len() - 1;
            let stmt_prefix = if is_last { "└── " } else { "├── " };
            let stmt_child = if is_last {
                format!("{}    ", parent_prefix)
            } else {
                format!("{}│   ", parent_prefix)
            };
            self.write_indent_with(parent_prefix);
            self.print_statement(stmt, stmt_prefix, &stmt_child);
        }
    }

    fn print_expr(&mut self, expr: &ResolvedExpr, prefix: &str, parent_prefix: &str) {
        match expr {
            ResolvedExpr::Int(v) => {
                self.write(&format!("{}Int({})", prefix, v));
                self.newline();
            }

            ResolvedExpr::Float(v) => {
                self.write(&format!("{}Float({})", prefix, v));
                self.newline();
            }

            ResolvedExpr::Bool(v) => {
                self.write(&format!("{}Bool({})", prefix, v));
                self.newline();
            }

            ResolvedExpr::Str(v) => {
                let escaped = v.replace('\n', "\\n").replace('\t', "\\t");
                self.write(&format!("{}String(\"{}\")", prefix, escaped));
                self.newline();
            }

            ResolvedExpr::Nil => {
                self.write(&format!("{}Nil", prefix));
                self.newline();
            }

            ResolvedExpr::Local(slot) => {
                self.write(&format!("{}Local(slot:{})", prefix, slot));
                self.newline();
            }

            ResolvedExpr::Array { elements } => {
                self.write(&format!("{}Array[{}]", prefix, elements.len()));
                self.newline();
                for (i, elem) in elements.iter().enumerate() {
                    let is_last = i == elements.len() - 1;
                    let elem_prefix = if is_last { "└── " } else { "├── " };
                    let elem_child = if is_last {
                        format!("{}    ", parent_prefix)
                    } else {
                        format!("{}│   ", parent_prefix)
                    };
                    self.write_indent_with(parent_prefix);
                    self.print_expr(elem, elem_prefix, &elem_child);
                }
            }

            ResolvedExpr::Index { object, index, .. } => {
                self.write(&format!("{}Index", prefix));
                self.newline();
                let obj_child = format!("{}│   ", parent_prefix);
                let idx_child = format!("{}    ", parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(object, "├── object: ", &obj_child);
                self.write_indent_with(parent_prefix);
                self.print_expr(index, "└── index: ", &idx_child);
            }

            ResolvedExpr::Field { object, field } => {
                self.write(&format!("{}Field .{}", prefix, field));
                self.newline();
                let obj_child = format!("{}    ", parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(object, "└── ", &obj_child);
            }

            ResolvedExpr::Unary { op, operand } => {
                let op_str = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                };
                self.write(&format!("{}Unary({})", prefix, op_str));
                self.newline();
                let operand_child = format!("{}    ", parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(operand, "└── ", &operand_child);
            }

            ResolvedExpr::Binary { op, left, right } => {
                let op_str = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Mod => "%",
                    BinaryOp::Eq => "==",
                    BinaryOp::Ne => "!=",
                    BinaryOp::Lt => "<",
                    BinaryOp::Le => "<=",
                    BinaryOp::Gt => ">",
                    BinaryOp::Ge => ">=",
                    BinaryOp::And => "&&",
                    BinaryOp::Or => "||",
                };
                self.write(&format!("{}Binary({})", prefix, op_str));
                self.newline();
                let left_child = format!("{}│   ", parent_prefix);
                let right_child = format!("{}    ", parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(left, "├── ", &left_child);
                self.write_indent_with(parent_prefix);
                self.print_expr(right, "└── ", &right_child);
            }

            ResolvedExpr::Call { func_index, args } => {
                self.write(&format!(
                    "{}Call func:{} args:{}",
                    prefix,
                    func_index,
                    args.len()
                ));
                self.newline();
                for (i, arg) in args.iter().enumerate() {
                    let is_last = i == args.len() - 1;
                    let arg_prefix = if is_last { "└── " } else { "├── " };
                    let arg_child = if is_last {
                        format!("{}    ", parent_prefix)
                    } else {
                        format!("{}│   ", parent_prefix)
                    };
                    self.write_indent_with(parent_prefix);
                    self.print_expr(arg, arg_prefix, &arg_child);
                }
            }

            ResolvedExpr::Builtin { name, args, .. } => {
                self.write(&format!("{}Builtin({}) args:{}", prefix, name, args.len()));
                self.newline();
                for (i, arg) in args.iter().enumerate() {
                    let is_last = i == args.len() - 1;
                    let arg_prefix = if is_last { "└── " } else { "├── " };
                    let arg_child = if is_last {
                        format!("{}    ", parent_prefix)
                    } else {
                        format!("{}│   ", parent_prefix)
                    };
                    self.write_indent_with(parent_prefix);
                    self.print_expr(arg, arg_prefix, &arg_child);
                }
            }

            ResolvedExpr::SpawnFunc { func_index } => {
                self.write(&format!("{}SpawnFunc func:{}", prefix, func_index));
                self.newline();
            }

            ResolvedExpr::StructLiteral {
                struct_index,
                fields,
            } => {
                self.write(&format!("{}StructLiteral struct:{}", prefix, struct_index));
                self.newline();
                for (i, field) in fields.iter().enumerate() {
                    let is_last = i == fields.len() - 1;
                    let field_prefix = if is_last { "└── " } else { "├── " };
                    let field_child = if is_last {
                        format!("{}    ", parent_prefix)
                    } else {
                        format!("{}│   ", parent_prefix)
                    };
                    self.write_indent_with(parent_prefix);
                    self.print_expr(field, &format!("{}[{}]: ", field_prefix, i), &field_child);
                }
            }

            ResolvedExpr::MethodCall {
                object,
                method,
                func_index,
                args,
                return_struct_name,
            } => {
                let ret_info = return_struct_name
                    .as_ref()
                    .map(|s| format!(" -> {}", s))
                    .unwrap_or_default();
                self.write(&format!(
                    "{}MethodCall .{}({}) -> func[{}]{}",
                    prefix,
                    method,
                    args.len(),
                    func_index,
                    ret_info
                ));
                self.newline();
                let has_args = !args.is_empty();
                let obj_prefix = if has_args { "├── " } else { "└── " };
                let obj_child = if has_args {
                    format!("{}│   ", parent_prefix)
                } else {
                    format!("{}    ", parent_prefix)
                };
                self.write_indent_with(parent_prefix);
                self.print_expr(object, &format!("{}object: ", obj_prefix), &obj_child);
                for (i, arg) in args.iter().enumerate() {
                    let is_last = i == args.len() - 1;
                    let arg_prefix = if is_last { "└── " } else { "├── " };
                    let arg_child = if is_last {
                        format!("{}    ", parent_prefix)
                    } else {
                        format!("{}│   ", parent_prefix)
                    };
                    self.write_indent_with(parent_prefix);
                    self.print_expr(arg, &format!("{}arg: ", arg_prefix), &arg_child);
                }
            }
            ResolvedExpr::AssociatedFunctionCall {
                func_index,
                args,
                return_struct_name,
            } => {
                let ret_str = return_struct_name
                    .as_ref()
                    .map(|s| format!(" -> {}", s))
                    .unwrap_or_default();
                self.write(&format!(
                    "{}AssociatedFunctionCall(func_index={}, {} args){}",
                    prefix,
                    func_index,
                    args.len(),
                    ret_str
                ));
                self.newline();
                for (i, arg) in args.iter().enumerate() {
                    let is_last = i == args.len() - 1;
                    let arg_prefix = if is_last { "└── " } else { "├── " };
                    let arg_child = if is_last {
                        format!("{}    ", parent_prefix)
                    } else {
                        format!("{}│   ", parent_prefix)
                    };
                    self.write_indent_with(parent_prefix);
                    self.print_expr(arg, &format!("{}arg: ", arg_prefix), &arg_child);
                }
            }
            ResolvedExpr::AsmBlock {
                input_slots,
                output_type,
                body,
            } => {
                let output_str = output_type
                    .as_ref()
                    .map(|t| format!(" -> {}", t))
                    .unwrap_or_default();
                self.write(&format!(
                    "{}AsmBlock(inputs: {:?}, {} instructions){}",
                    prefix,
                    input_slots,
                    body.len(),
                    output_str
                ));
                self.newline();
            }
            ResolvedExpr::NewLiteral {
                type_name,
                type_args,
                elements,
            } => {
                let type_args_str = if type_args.is_empty() {
                    String::new()
                } else {
                    format!(
                        "<{}>",
                        type_args
                            .iter()
                            .map(|t| format!("{:?}", t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                self.write(&format!(
                    "{}NewLiteral(new {}{}, {} elements)",
                    prefix,
                    type_name,
                    type_args_str,
                    elements.len()
                ));
                self.newline();
            }
            ResolvedExpr::Block { statements, expr } => {
                self.write(&format!("{}Block({} stmts)", prefix, statements.len()));
                self.newline();
                let block_child_prefix = format!("{}    ", parent_prefix);
                for stmt in statements {
                    self.write_indent_with(parent_prefix);
                    self.print_statement(stmt, "├── ", &block_child_prefix);
                }
                self.write_indent_with(parent_prefix);
                self.print_expr(expr, "└── result: ", &block_child_prefix);
            }

            ResolvedExpr::Closure {
                func_index,
                captures,
            } => {
                let cap_strs: Vec<String> = captures
                    .iter()
                    .map(|c| {
                        if c.mutable {
                            format!("slot:{}(ref)", c.outer_slot)
                        } else {
                            format!("slot:{}", c.outer_slot)
                        }
                    })
                    .collect();
                self.write(&format!(
                    "{}Closure(func:{}, captures:[{}])",
                    prefix,
                    func_index,
                    cap_strs.join(", ")
                ));
                self.newline();
            }

            ResolvedExpr::CallIndirect { callee, args } => {
                self.write(&format!("{}CallIndirect(args:{})", prefix, args.len()));
                self.newline();
                let has_args = !args.is_empty();
                let callee_prefix = if has_args { "├── " } else { "└── " };
                let callee_child = if has_args {
                    format!("{}│   ", parent_prefix)
                } else {
                    format!("{}    ", parent_prefix)
                };
                self.write_indent_with(parent_prefix);
                self.print_expr(callee, &format!("{}callee: ", callee_prefix), &callee_child);
                for (i, arg) in args.iter().enumerate() {
                    let is_last = i == args.len() - 1;
                    let arg_prefix = if is_last { "└── " } else { "├── " };
                    let arg_child = if is_last {
                        format!("{}    ", parent_prefix)
                    } else {
                        format!("{}│   ", parent_prefix)
                    };
                    self.write_indent_with(parent_prefix);
                    self.print_expr(arg, &format!("{}arg: ", arg_prefix), &arg_child);
                }
            }

            ResolvedExpr::CaptureLoad { offset, is_ref } => {
                let ref_str = if *is_ref { ", ref" } else { "" };
                self.write(&format!(
                    "{}CaptureLoad(offset:{}{})",
                    prefix, offset, ref_str
                ));
                self.newline();
            }
            ResolvedExpr::CaptureStore { offset, value } => {
                self.write(&format!("{}CaptureStore(offset:{})", prefix, offset));
                self.newline();
                let val_child = format!("{}    ", parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(value, "└── value: ", &val_child);
            }
            ResolvedExpr::RefCellNew { value } => {
                self.write(&format!("{}RefCellNew", prefix));
                self.newline();
                let val_child = format!("{}    ", parent_prefix);
                self.write_indent_with(parent_prefix);
                self.print_expr(value, "└── value: ", &val_child);
            }
            ResolvedExpr::RefCellLoad { slot } => {
                self.write(&format!("{}RefCellLoad(slot:{})", prefix, slot));
                self.newline();
            }
        }
    }

    fn write(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn write_no_indent(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn writeln(&mut self, s: &str) {
        self.write_indent();
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn newline(&mut self) {
        self.output.push('\n');
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("  ");
        }
    }

    fn write_indent_with(&mut self, prefix: &str) {
        for _ in 0..self.indent {
            self.output.push_str("  ");
        }
        self.output.push_str(prefix);
    }
}

impl Default for ResolvedProgramPrinter {
    fn default() -> Self {
        Self::new()
    }
}

/// Format a resolved program as a pretty-printed string.
pub fn format_resolved(program: &ResolvedProgram) -> String {
    let mut printer = ResolvedProgramPrinter::new();
    printer.print(program).to_string()
}

// ============================================================================
// Bytecode Disassembler
// ============================================================================

/// Disassembler for bytecode.
pub struct Disassembler<'a> {
    chunk: &'a Chunk,
    output: String,
}

impl<'a> Disassembler<'a> {
    pub fn new(chunk: &'a Chunk) -> Self {
        Self {
            chunk,
            output: String::new(),
        }
    }

    pub fn disassemble(&mut self) -> &str {
        // Print string constants if any
        if !self.chunk.strings.is_empty() {
            self.output.push_str("== String Constants ==\n");
            for (i, s) in self.chunk.strings.iter().enumerate() {
                let escaped = s.replace('\n', "\\n").replace('\t', "\\t");
                self.output
                    .push_str(&format!("  [{:04}] \"{}\"\n", i, escaped));
            }
            self.output.push('\n');
        }

        // Print functions
        for (i, func) in self.chunk.functions.iter().enumerate() {
            self.disassemble_function(func, i);
            self.output.push('\n');
        }

        // Print main
        self.output.push_str("== Main ==\n");
        self.disassemble_code(&self.chunk.main.code);

        &self.output
    }

    fn disassemble_function(&mut self, func: &Function, index: usize) {
        self.output.push_str(&format!(
            "== Function[{}]: {} (arity: {}, locals: {}) ==\n",
            index, func.name, func.arity, func.locals_count
        ));
        self.disassemble_code(&func.code);
    }

    fn disassemble_code(&mut self, code: &[Op]) {
        for (pc, op) in code.iter().enumerate() {
            self.output.push_str(&format!("{:04}: ", pc));
            self.disassemble_op(op);
            self.output.push('\n');
        }
    }

    fn disassemble_op(&mut self, op: &Op) {
        match op {
            // Constants
            Op::I32Const(v) => self.output.push_str(&format!("I32Const {}", v)),
            Op::I64Const(v) => self.output.push_str(&format!("I64Const {}", v)),
            Op::F32Const(v) => self.output.push_str(&format!("F32Const {}", v)),
            Op::F64Const(v) => self.output.push_str(&format!("F64Const {}", v)),
            Op::RefNull => self.output.push_str("RefNull"),
            Op::StringConst(idx) => {
                let s = self
                    .chunk
                    .strings
                    .get(*idx)
                    .map(|s| s.as_str())
                    .unwrap_or("<?>");
                let escaped = s.replace('\n', "\\n").replace('\t', "\\t");
                self.output
                    .push_str(&format!("StringConst {} ; \"{}\"", idx, escaped));
            }

            // Local variables
            Op::LocalGet(slot) => self.output.push_str(&format!("LocalGet {}", slot)),
            Op::LocalSet(slot) => self.output.push_str(&format!("LocalSet {}", slot)),

            // Stack manipulation
            Op::Drop => self.output.push_str("Drop"),
            Op::Dup => self.output.push_str("Dup"),
            Op::Pick(n) => self.output.push_str(&format!("Pick {}", n)),
            Op::PickDyn => self.output.push_str("PickDyn"),

            // i32 Arithmetic
            Op::I32Add => self.output.push_str("I32Add"),
            Op::I32Sub => self.output.push_str("I32Sub"),
            Op::I32Mul => self.output.push_str("I32Mul"),
            Op::I32DivS => self.output.push_str("I32DivS"),
            Op::I32RemS => self.output.push_str("I32RemS"),
            Op::I32Eqz => self.output.push_str("I32Eqz"),

            // i64 Arithmetic
            Op::I64Add => self.output.push_str("I64Add"),
            Op::I64Sub => self.output.push_str("I64Sub"),
            Op::I64Mul => self.output.push_str("I64Mul"),
            Op::I64DivS => self.output.push_str("I64DivS"),
            Op::I64RemS => self.output.push_str("I64RemS"),
            Op::I64Neg => self.output.push_str("I64Neg"),

            // f32 Arithmetic
            Op::F32Add => self.output.push_str("F32Add"),
            Op::F32Sub => self.output.push_str("F32Sub"),
            Op::F32Mul => self.output.push_str("F32Mul"),
            Op::F32Div => self.output.push_str("F32Div"),
            Op::F32Neg => self.output.push_str("F32Neg"),

            // f64 Arithmetic
            Op::F64Add => self.output.push_str("F64Add"),
            Op::F64Sub => self.output.push_str("F64Sub"),
            Op::F64Mul => self.output.push_str("F64Mul"),
            Op::F64Div => self.output.push_str("F64Div"),
            Op::F64Neg => self.output.push_str("F64Neg"),

            // i32 Comparison
            Op::I32Eq => self.output.push_str("I32Eq"),
            Op::I32Ne => self.output.push_str("I32Ne"),
            Op::I32LtS => self.output.push_str("I32LtS"),
            Op::I32LeS => self.output.push_str("I32LeS"),
            Op::I32GtS => self.output.push_str("I32GtS"),
            Op::I32GeS => self.output.push_str("I32GeS"),

            // i64 Comparison
            Op::I64Eq => self.output.push_str("I64Eq"),
            Op::I64Ne => self.output.push_str("I64Ne"),
            Op::I64LtS => self.output.push_str("I64LtS"),
            Op::I64LeS => self.output.push_str("I64LeS"),
            Op::I64GtS => self.output.push_str("I64GtS"),
            Op::I64GeS => self.output.push_str("I64GeS"),

            // f32 Comparison
            Op::F32Eq => self.output.push_str("F32Eq"),
            Op::F32Ne => self.output.push_str("F32Ne"),
            Op::F32Lt => self.output.push_str("F32Lt"),
            Op::F32Le => self.output.push_str("F32Le"),
            Op::F32Gt => self.output.push_str("F32Gt"),
            Op::F32Ge => self.output.push_str("F32Ge"),

            // f64 Comparison
            Op::F64Eq => self.output.push_str("F64Eq"),
            Op::F64Ne => self.output.push_str("F64Ne"),
            Op::F64Lt => self.output.push_str("F64Lt"),
            Op::F64Le => self.output.push_str("F64Le"),
            Op::F64Gt => self.output.push_str("F64Gt"),
            Op::F64Ge => self.output.push_str("F64Ge"),

            // Ref Comparison
            Op::RefEq => self.output.push_str("RefEq"),
            Op::RefIsNull => self.output.push_str("RefIsNull"),

            // Type Conversion
            Op::I32WrapI64 => self.output.push_str("I32WrapI64"),
            Op::I64ExtendI32S => self.output.push_str("I64ExtendI32S"),
            Op::I64ExtendI32U => self.output.push_str("I64ExtendI32U"),
            Op::F64ConvertI64S => self.output.push_str("F64ConvertI64S"),
            Op::I64TruncF64S => self.output.push_str("I64TruncF64S"),
            Op::F64ConvertI32S => self.output.push_str("F64ConvertI32S"),
            Op::F32ConvertI32S => self.output.push_str("F32ConvertI32S"),
            Op::F32ConvertI64S => self.output.push_str("F32ConvertI64S"),
            Op::I32TruncF32S => self.output.push_str("I32TruncF32S"),
            Op::I32TruncF64S => self.output.push_str("I32TruncF64S"),
            Op::I64TruncF32S => self.output.push_str("I64TruncF32S"),
            Op::F32DemoteF64 => self.output.push_str("F32DemoteF64"),
            Op::F64PromoteF32 => self.output.push_str("F64PromoteF32"),

            // Control flow
            Op::Jmp(target) => self.output.push_str(&format!("Jmp {}", target)),
            Op::BrIf(target) => self.output.push_str(&format!("BrIf {}", target)),
            Op::BrIfFalse(target) => self.output.push_str(&format!("BrIfFalse {}", target)),

            // Functions
            Op::Call(func_idx, argc) => {
                let func_name = self
                    .chunk
                    .functions
                    .get(*func_idx)
                    .map(|f| f.name.as_str())
                    .unwrap_or("<?>");
                self.output
                    .push_str(&format!("Call {}, {} ; {}", func_idx, argc, func_name));
            }
            Op::Ret => self.output.push_str("Ret"),

            // Heap operations
            Op::HeapAlloc(n) => self.output.push_str(&format!("HeapAlloc {}", n)),
            Op::HeapAllocArray(n) => self.output.push_str(&format!("HeapAllocArray {}", n)),
            Op::HeapAllocDyn => self.output.push_str("HeapAllocDyn"),
            Op::HeapAllocDynSimple => self.output.push_str("HeapAllocDynSimple"),
            Op::HeapLoad(offset) => self.output.push_str(&format!("HeapLoad {}", offset)),
            Op::HeapStore(offset) => self.output.push_str(&format!("HeapStore {}", offset)),
            Op::HeapLoadDyn => self.output.push_str("HeapLoadDyn"),
            Op::HeapStoreDyn => self.output.push_str("HeapStoreDyn"),
            Op::HeapLoad2 => self.output.push_str("HeapLoad2"),
            Op::HeapStore2 => self.output.push_str("HeapStore2"),
            // System / Builtins
            Op::Syscall(num, argc) => self.output.push_str(&format!("Syscall {} {}", num, argc)),
            Op::GcHint(size) => self.output.push_str(&format!("GcHint {}", size)),
            Op::PrintDebug => self.output.push_str("PrintDebug"),
            Op::TypeOf => self.output.push_str("TypeOf"),
            Op::ToString => self.output.push_str("ToString"),
            Op::ParseInt => self.output.push_str("ParseInt"),
            // Exception handling
            Op::Throw => self.output.push_str("Throw"),
            Op::TryBegin(target) => self.output.push_str(&format!("TryBegin {}", target)),
            Op::TryEnd => self.output.push_str("TryEnd"),

            // CLI arguments
            Op::Argc => self.output.push_str("Argc"),
            Op::Argv => self.output.push_str("Argv"),
            Op::Args => self.output.push_str("Args"),

            // Threading
            Op::ThreadSpawn(func_idx) => {
                let func_name = self
                    .chunk
                    .functions
                    .get(*func_idx)
                    .map(|f| f.name.as_str())
                    .unwrap_or("<?>");
                self.output
                    .push_str(&format!("ThreadSpawn {} ; {}", func_idx, func_name));
            }
            Op::ChannelCreate => self.output.push_str("ChannelCreate"),
            Op::ChannelSend => self.output.push_str("ChannelSend"),
            Op::ChannelRecv => self.output.push_str("ChannelRecv"),
            Op::ThreadJoin => self.output.push_str("ThreadJoin"),

            // Closures
            Op::CallIndirect(argc) => {
                self.output.push_str(&format!("CallIndirect {}", argc));
            }
        }
    }
}

/// Format a chunk as a disassembled bytecode string.
pub fn format_bytecode(chunk: &Chunk) -> String {
    let mut disassembler = Disassembler::new(chunk);
    disassembler.disassemble().to_string()
}

/// Format a chunk as disassembled MicroOp (register-based IR) string.
pub fn format_microops(chunk: &Chunk) -> String {
    let mut output = String::new();

    for (i, func) in chunk.functions.iter().enumerate() {
        let converted = microop_converter::convert(func);
        output.push_str(&format!(
            "== Function[{}]: {} (arity: {}, locals: {}, temps: {}) ==\n",
            i, func.name, func.arity, func.locals_count, converted.temps_count
        ));
        format_microop_code(&mut output, &converted.micro_ops, chunk);
        output.push('\n');
    }

    // Main
    let converted = microop_converter::convert(&chunk.main);
    output.push_str(&format!(
        "== Main (locals: {}, temps: {}) ==\n",
        chunk.main.locals_count, converted.temps_count
    ));
    format_microop_code(&mut output, &converted.micro_ops, chunk);

    output
}

fn format_microop_code(output: &mut String, ops: &[MicroOp], chunk: &Chunk) {
    for (pc, mop) in ops.iter().enumerate() {
        output.push_str(&format!("{:04}: ", pc));
        format_single_microop(output, mop, chunk);
        output.push('\n');
    }
}

fn format_vreg(v: &VReg) -> String {
    format!("v{}", v.0)
}

fn format_cond(cond: &CmpCond) -> &'static str {
    match cond {
        CmpCond::Eq => "eq",
        CmpCond::Ne => "ne",
        CmpCond::LtS => "lt",
        CmpCond::LeS => "le",
        CmpCond::GtS => "gt",
        CmpCond::GeS => "ge",
    }
}

fn format_single_microop(output: &mut String, mop: &MicroOp, chunk: &Chunk) {
    match mop {
        // Control flow
        MicroOp::Jmp {
            target,
            old_pc,
            old_target,
        } => output.push_str(&format!("Jmp {} (op {}→{})", target, old_pc, old_target)),
        MicroOp::BrIf { cond, target } => {
            output.push_str(&format!("BrIf {}, target={}", format_vreg(cond), target))
        }
        MicroOp::BrIfFalse { cond, target } => output.push_str(&format!(
            "BrIfFalse {}, target={}",
            format_vreg(cond),
            target
        )),
        MicroOp::Call { func_id, args, ret } => {
            let func_name = chunk
                .functions
                .get(*func_id)
                .map(|f| f.name.as_str())
                .unwrap_or("<?>");
            let args_str: Vec<String> = args.iter().map(format_vreg).collect();
            let ret_str = match ret {
                Some(r) => format!(" → {}", format_vreg(r)),
                None => String::new(),
            };
            output.push_str(&format!(
                "Call {}({}){}  ; {}",
                func_id,
                args_str.join(", "),
                ret_str,
                func_name
            ))
        }
        MicroOp::Ret { src } => match src {
            Some(s) => output.push_str(&format!("Ret {}", format_vreg(s))),
            None => output.push_str("Ret"),
        },
        MicroOp::CallIndirect { callee, args, ret } => {
            let args_str: Vec<String> = args.iter().map(format_vreg).collect();
            let ret_str = match ret {
                Some(r) => format!(" → {}", format_vreg(r)),
                None => String::new(),
            };
            output.push_str(&format!(
                "CallIndirect {}({}){}",
                format_vreg(callee),
                args_str.join(", "),
                ret_str
            ))
        }

        // Move / Constants
        MicroOp::Mov { dst, src } => {
            output.push_str(&format!("Mov {}, {}", format_vreg(dst), format_vreg(src)))
        }
        MicroOp::ConstI64 { dst, imm } => {
            output.push_str(&format!("ConstI64 {}, {}", format_vreg(dst), imm))
        }
        MicroOp::ConstI32 { dst, imm } => {
            output.push_str(&format!("ConstI32 {}, {}", format_vreg(dst), imm))
        }
        MicroOp::ConstF64 { dst, imm } => {
            output.push_str(&format!("ConstF64 {}, {}", format_vreg(dst), imm))
        }
        MicroOp::ConstF32 { dst, imm } => {
            output.push_str(&format!("ConstF32 {}, {}", format_vreg(dst), imm))
        }

        // i64 ALU
        MicroOp::AddI64 { dst, a, b } => output.push_str(&format!(
            "AddI64 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::AddI64Imm { dst, a, imm } => output.push_str(&format!(
            "AddI64Imm {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            imm
        )),
        MicroOp::SubI64 { dst, a, b } => output.push_str(&format!(
            "SubI64 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::MulI64 { dst, a, b } => output.push_str(&format!(
            "MulI64 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::DivI64 { dst, a, b } => output.push_str(&format!(
            "DivI64 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::RemI64 { dst, a, b } => output.push_str(&format!(
            "RemI64 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::NegI64 { dst, src } => output.push_str(&format!(
            "NegI64 {}, {}",
            format_vreg(dst),
            format_vreg(src)
        )),

        // i32 ALU
        MicroOp::AddI32 { dst, a, b } => output.push_str(&format!(
            "AddI32 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::SubI32 { dst, a, b } => output.push_str(&format!(
            "SubI32 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::MulI32 { dst, a, b } => output.push_str(&format!(
            "MulI32 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::DivI32 { dst, a, b } => output.push_str(&format!(
            "DivI32 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::RemI32 { dst, a, b } => output.push_str(&format!(
            "RemI32 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::EqzI32 { dst, src } => output.push_str(&format!(
            "EqzI32 {}, {}",
            format_vreg(dst),
            format_vreg(src)
        )),

        // f64 ALU
        MicroOp::AddF64 { dst, a, b } => output.push_str(&format!(
            "AddF64 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::SubF64 { dst, a, b } => output.push_str(&format!(
            "SubF64 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::MulF64 { dst, a, b } => output.push_str(&format!(
            "MulF64 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::DivF64 { dst, a, b } => output.push_str(&format!(
            "DivF64 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::NegF64 { dst, src } => output.push_str(&format!(
            "NegF64 {}, {}",
            format_vreg(dst),
            format_vreg(src)
        )),

        // f32 ALU
        MicroOp::AddF32 { dst, a, b } => output.push_str(&format!(
            "AddF32 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::SubF32 { dst, a, b } => output.push_str(&format!(
            "SubF32 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::MulF32 { dst, a, b } => output.push_str(&format!(
            "MulF32 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::DivF32 { dst, a, b } => output.push_str(&format!(
            "DivF32 {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::NegF32 { dst, src } => output.push_str(&format!(
            "NegF32 {}, {}",
            format_vreg(dst),
            format_vreg(src)
        )),

        // Comparisons
        MicroOp::CmpI64 { dst, a, b, cond } => output.push_str(&format!(
            "CmpI64.{} {}, {}, {}",
            format_cond(cond),
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::CmpI64Imm { dst, a, imm, cond } => output.push_str(&format!(
            "CmpI64Imm.{} {}, {}, {}",
            format_cond(cond),
            format_vreg(dst),
            format_vreg(a),
            imm
        )),
        MicroOp::CmpI32 { dst, a, b, cond } => output.push_str(&format!(
            "CmpI32.{} {}, {}, {}",
            format_cond(cond),
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::CmpF64 { dst, a, b, cond } => output.push_str(&format!(
            "CmpF64.{} {}, {}, {}",
            format_cond(cond),
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::CmpF32 { dst, a, b, cond } => output.push_str(&format!(
            "CmpF32.{} {}, {}, {}",
            format_cond(cond),
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),

        // Type conversions
        MicroOp::I32WrapI64 { dst, src }
        | MicroOp::I64ExtendI32S { dst, src }
        | MicroOp::I64ExtendI32U { dst, src }
        | MicroOp::F64ConvertI64S { dst, src }
        | MicroOp::I64TruncF64S { dst, src }
        | MicroOp::F64ConvertI32S { dst, src }
        | MicroOp::F32ConvertI32S { dst, src }
        | MicroOp::F32ConvertI64S { dst, src }
        | MicroOp::I32TruncF32S { dst, src }
        | MicroOp::I32TruncF64S { dst, src }
        | MicroOp::I64TruncF32S { dst, src }
        | MicroOp::F32DemoteF64 { dst, src }
        | MicroOp::F64PromoteF32 { dst, src } => {
            // Use Debug name of the variant
            let name = match mop {
                MicroOp::I32WrapI64 { .. } => "I32WrapI64",
                MicroOp::I64ExtendI32S { .. } => "I64ExtendI32S",
                MicroOp::I64ExtendI32U { .. } => "I64ExtendI32U",
                MicroOp::F64ConvertI64S { .. } => "F64ConvertI64S",
                MicroOp::I64TruncF64S { .. } => "I64TruncF64S",
                MicroOp::F64ConvertI32S { .. } => "F64ConvertI32S",
                MicroOp::F32ConvertI32S { .. } => "F32ConvertI32S",
                MicroOp::F32ConvertI64S { .. } => "F32ConvertI64S",
                MicroOp::I32TruncF32S { .. } => "I32TruncF32S",
                MicroOp::I32TruncF64S { .. } => "I32TruncF64S",
                MicroOp::I64TruncF32S { .. } => "I64TruncF32S",
                MicroOp::F32DemoteF64 { .. } => "F32DemoteF64",
                MicroOp::F64PromoteF32 { .. } => "F64PromoteF32",
                _ => unreachable!(),
            };
            output.push_str(&format!(
                "{} {}, {}",
                name,
                format_vreg(dst),
                format_vreg(src)
            ))
        }

        // Ref operations
        MicroOp::RefEq { dst, a, b } => output.push_str(&format!(
            "RefEq {}, {}, {}",
            format_vreg(dst),
            format_vreg(a),
            format_vreg(b)
        )),
        MicroOp::RefIsNull { dst, src } => output.push_str(&format!(
            "RefIsNull {}, {}",
            format_vreg(dst),
            format_vreg(src)
        )),
        MicroOp::RefNull { dst } => output.push_str(&format!("RefNull {}", format_vreg(dst))),

        // Heap operations
        MicroOp::HeapLoad { dst, src, offset } => output.push_str(&format!(
            "HeapLoad {}, {}, {}",
            format_vreg(dst),
            format_vreg(src),
            offset
        )),
        MicroOp::HeapLoadDyn { dst, obj, idx } => output.push_str(&format!(
            "HeapLoadDyn {}, {}, {}",
            format_vreg(dst),
            format_vreg(obj),
            format_vreg(idx)
        )),
        MicroOp::HeapStore {
            dst_obj,
            offset,
            src,
        } => output.push_str(&format!(
            "HeapStore {}, {}, {}",
            format_vreg(dst_obj),
            offset,
            format_vreg(src)
        )),
        MicroOp::HeapStoreDyn { obj, idx, src } => output.push_str(&format!(
            "HeapStoreDyn {}, {}, {}",
            format_vreg(obj),
            format_vreg(idx),
            format_vreg(src)
        )),
        MicroOp::HeapLoad2 { dst, obj, idx } => output.push_str(&format!(
            "HeapLoad2 {}, {}, {}",
            format_vreg(dst),
            format_vreg(obj),
            format_vreg(idx)
        )),
        MicroOp::HeapStore2 { obj, idx, src } => output.push_str(&format!(
            "HeapStore2 {}, {}, {}",
            format_vreg(obj),
            format_vreg(idx),
            format_vreg(src)
        )),

        // Stack bridge
        MicroOp::StackPush { src } => output.push_str(&format!("StackPush {}", format_vreg(src))),
        MicroOp::StackPop { dst } => output.push_str(&format!("StackPop {}", format_vreg(dst))),

        // Raw fallback
        MicroOp::Raw { op } => {
            output.push_str("Raw { ");
            // Reuse the Op disassembler for the inner op
            match op {
                Op::Call(func_idx, argc) => {
                    let func_name = chunk
                        .functions
                        .get(*func_idx)
                        .map(|f| f.name.as_str())
                        .unwrap_or("<?>");
                    output.push_str(&format!("Call {}, {} ; {}", func_idx, argc, func_name));
                }
                Op::StringConst(idx) => {
                    let s = chunk
                        .strings
                        .get(*idx)
                        .map(|s| {
                            let escaped = s.replace('\n', "\\n").replace('\t', "\\t");
                            if escaped.len() > 40 {
                                format!("{}...", &escaped[..40])
                            } else {
                                escaped
                            }
                        })
                        .unwrap_or_else(|| "<?>".to_string());
                    output.push_str(&format!("StringConst {} ; \"{}\"", idx, s));
                }
                _ => output.push_str(&format!("{:?}", op)),
            }
            output.push_str(" }");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lexer::Lexer;
    use crate::compiler::parser::Parser;
    use crate::compiler::resolver::Resolver;

    fn parse(source: &str) -> Program {
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();
        let mut parser = Parser::new("test.mc", tokens);
        parser.parse().unwrap()
    }

    fn resolve(source: &str) -> ResolvedProgram {
        let program = parse(source);
        let mut resolver = Resolver::new("test.mc");
        resolver.resolve(program).unwrap()
    }

    #[test]
    fn test_simple_let() {
        let program = parse("let x = 42;");
        let output = format_ast(&program);
        assert!(output.contains("Program"));
        assert!(output.contains("Let: x"));
        assert!(output.contains("Int: 42"));
    }

    #[test]
    fn test_function() {
        let program = parse("fun add(a: int, b: int) -> int { return a + b; }");
        let output = format_ast(&program);
        assert!(output.contains("FnDef: add(a: int, b: int) -> int"));
        assert!(output.contains("Return"));
        assert!(output.contains("Binary: +"));
    }

    #[test]
    fn test_struct() {
        let program = parse("struct Point { x: int, y: int }");
        let output = format_ast(&program);
        assert!(output.contains("StructDef: Point"));
        assert!(output.contains("Field: x: int"));
        assert!(output.contains("Field: y: int"));
    }

    #[test]
    fn test_if_else() {
        let program = parse("if true { let x = 1; } else { let y = 2; }");
        let output = format_ast(&program);
        assert!(output.contains("If"));
        assert!(output.contains("condition:"));
        assert!(output.contains("then:"));
        assert!(output.contains("else:"));
    }

    // ResolvedProgram tests

    #[test]
    fn test_resolved_simple_let() {
        let resolved = resolve("let x = 42;");
        let output = format_resolved(&resolved);
        assert!(output.contains("ResolvedProgram"));
        assert!(output.contains("Main [locals: 1]:"));
        assert!(output.contains("Let slot:0"));
        assert!(output.contains("Int(42)"));
    }

    #[test]
    fn test_resolved_function() {
        let resolved = resolve("fun add(a, b) { return a + b; } let r = add(1, 2);");
        let output = format_resolved(&resolved);
        assert!(output.contains("Functions:"));
        assert!(output.contains("add"));
        assert!(output.contains("a -> slot:0"));
        assert!(output.contains("b -> slot:1"));
        assert!(output.contains("Binary(+)"));
        assert!(output.contains("Call func:0"));
    }

    #[test]
    fn test_resolved_struct() {
        let resolved = resolve("struct Point { x: int, y: int } let p = Point { x: 1, y: 2 };");
        let output = format_resolved(&resolved);
        assert!(output.contains("Structs:"));
        assert!(output.contains("[0] Point { x, y }"));
        assert!(output.contains("StructLiteral struct:0"));
    }

    #[test]
    fn test_resolved_builtin() {
        let resolved = resolve("print_debug(42);");
        let output = format_resolved(&resolved);
        assert!(output.contains("Builtin(print_debug)"));
    }

    // Bytecode disassembler tests

    fn compile(source: &str) -> Chunk {
        use crate::compiler::{Codegen, Resolver};
        let program = parse(source);
        let mut resolver = Resolver::new("test.mc");
        let resolved = resolver.resolve(program).unwrap();
        let mut codegen = Codegen::new();
        codegen.compile(resolved).unwrap()
    }

    #[test]
    fn test_bytecode_simple() {
        let chunk = compile("let x = 42; print_debug(x);");
        let output = format_bytecode(&chunk);
        assert!(output.contains("== Main =="));
        assert!(output.contains("I64Const 42"));
        assert!(output.contains("LocalSet"));
        assert!(output.contains("LocalGet"));
        assert!(output.contains("PrintDebug"));
    }

    #[test]
    fn test_bytecode_function() {
        let chunk = compile("fun add(a, b) { return a + b; } print_debug(add(1, 2));");
        let output = format_bytecode(&chunk);
        assert!(output.contains("== Function[0]: add"));
        assert!(output.contains("LocalGet 0"));
        assert!(output.contains("LocalGet 1"));
        assert!(output.contains("I64Add"));
        assert!(output.contains("Ret"));
        assert!(output.contains("Call 0, 2 ; add"));
    }

    #[test]
    fn test_bytecode_control_flow() {
        let chunk = compile("if true { print_debug(1); } else { print_debug(2); }");
        let output = format_bytecode(&chunk);
        assert!(output.contains("I32Const 1"));
        assert!(output.contains("BrIfFalse"));
        assert!(output.contains("Jmp"));
    }

    #[test]
    fn test_bytecode_string_constants() {
        let chunk = compile(r#"let s = "hello"; print_debug(s);"#);
        let output = format_bytecode(&chunk);
        assert!(output.contains("== String Constants =="));
        assert!(output.contains("\"hello\""));
        assert!(output.contains("StringConst"));
    }
}
