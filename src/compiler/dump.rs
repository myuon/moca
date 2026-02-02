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
                mutable,
                type_annotation,
                init,
                ..
            } => {
                let mut_str = if *mutable { "mut " } else { "" };
                let type_str = type_annotation
                    .as_ref()
                    .map(|t| format!(": {}", t))
                    .unwrap_or_default();
                self.write_prefixed(prefix, &format!("Let: {}{}{}", mut_str, name, type_str));
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
            Expr::TypeLiteral {
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
                    "{}TypeLiteral: {}{} [{} elements]",
                    prefix,
                    type_name,
                    type_args_str,
                    elements.len()
                ));
                self.write_type_suffix(expr);
                self.newline();
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
            self.writeln("Main:");
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

            ResolvedExpr::Builtin { name, args } => {
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
            ResolvedExpr::TypeLiteral {
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
                    "{}TypeLiteral(type {}{}, {} elements)",
                    prefix,
                    type_name,
                    type_args_str,
                    elements.len()
                ));
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
            // Stack operations
            Op::PushInt(v) => self.output.push_str(&format!("PushInt {}", v)),
            Op::PushFloat(v) => self.output.push_str(&format!("PushFloat {}", v)),
            Op::PushTrue => self.output.push_str("PushTrue"),
            Op::PushFalse => self.output.push_str("PushFalse"),
            Op::PushNull => self.output.push_str("PushNil"),
            Op::PushString(idx) => {
                let s = self
                    .chunk
                    .strings
                    .get(*idx)
                    .map(|s| s.as_str())
                    .unwrap_or("<?>");
                let escaped = s.replace('\n', "\\n").replace('\t', "\\t");
                self.output
                    .push_str(&format!("PushString {} ; \"{}\"", idx, escaped));
            }
            Op::Pop => self.output.push_str("Pop"),
            Op::Dup => self.output.push_str("Dup"),
            Op::Swap => self.output.push_str("Swap"),
            Op::Pick(n) => self.output.push_str(&format!("Pick {}", n)),
            Op::PickDyn => self.output.push_str("PickDyn"),

            // Local variables
            Op::GetL(slot) => self.output.push_str(&format!("GetL {}", slot)),
            Op::SetL(slot) => self.output.push_str(&format!("SetL {}", slot)),

            // Arithmetic
            Op::Add => self.output.push_str("Add"),
            Op::Sub => self.output.push_str("Sub"),
            Op::Mul => self.output.push_str("Mul"),
            Op::Div => self.output.push_str("Div"),
            Op::Mod => self.output.push_str("Mod"),
            Op::Neg => self.output.push_str("Neg"),

            // Comparison
            Op::Eq => self.output.push_str("Eq"),
            Op::Ne => self.output.push_str("Ne"),
            Op::Lt => self.output.push_str("Lt"),
            Op::Le => self.output.push_str("Le"),
            Op::Gt => self.output.push_str("Gt"),
            Op::Ge => self.output.push_str("Ge"),

            // Logical
            Op::Not => self.output.push_str("Not"),

            // Control flow
            Op::Jmp(target) => self.output.push_str(&format!("Jmp {}", target)),
            Op::JmpIfFalse(target) => self.output.push_str(&format!("JmpIfFalse {}", target)),
            Op::JmpIfTrue(target) => self.output.push_str(&format!("JmpIfTrue {}", target)),

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

            // Array operations (legacy)
            Op::ArrayLen => self.output.push_str("ArrayLen"),

            // Type operations
            Op::TypeOf => self.output.push_str("TypeOf"),
            Op::ToString => self.output.push_str("ToString"),
            Op::ParseInt => self.output.push_str("ParseInt"),
            Op::StrLen => self.output.push_str("StrLen"),

            // Exception handling
            Op::Throw => self.output.push_str("Throw"),
            Op::TryBegin(target) => self.output.push_str(&format!("TryBegin {}", target)),
            Op::TryEnd => self.output.push_str("TryEnd"),

            // Builtins
            Op::PrintDebug => self.output.push_str("PrintDebug"),

            // GC hint
            Op::GcHint(size) => self.output.push_str(&format!("GcHint {}", size)),

            // Thread operations
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

            // Heap slot operations
            Op::AllocHeap(n) => self.output.push_str(&format!("AllocHeap {}", n)),
            Op::AllocHeapDyn => self.output.push_str("AllocHeapDyn"),
            Op::AllocHeapDynSimple => self.output.push_str("AllocHeapDynSimple"),
            Op::HeapLoad(offset) => self.output.push_str(&format!("HeapLoad {}", offset)),
            Op::HeapStore(offset) => self.output.push_str(&format!("HeapStore {}", offset)),
            Op::HeapLoadDyn => self.output.push_str("HeapLoadDyn"),
            Op::HeapStoreDyn => self.output.push_str("HeapStoreDyn"),

            // Syscall operations
            Op::Syscall(num, argc) => self.output.push_str(&format!("Syscall {} {}", num, argc)),

            // CLI argument operations
            Op::Argc => self.output.push_str("Argc"),
            Op::Argv => self.output.push_str("Argv"),
            Op::Args => self.output.push_str("Args"),

            // Type literal operations
            Op::VecLiteral(n) => self.output.push_str(&format!("VecLiteral {}", n)),
            Op::MapLiteral(n) => self.output.push_str(&format!("MapLiteral {}", n)),
        }
    }
}

/// Format a chunk as a disassembled bytecode string.
pub fn format_bytecode(chunk: &Chunk) -> String {
    let mut disassembler = Disassembler::new(chunk);
    disassembler.disassemble().to_string()
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
        assert!(output.contains("Main:"));
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
        assert!(output.contains("PushInt 42"));
        assert!(output.contains("SetL")); // renamed from StoreLocal
        assert!(output.contains("GetL")); // renamed from LoadLocal
        assert!(output.contains("PrintDebug"));
    }

    #[test]
    fn test_bytecode_function() {
        let chunk = compile("fun add(a, b) { return a + b; } print_debug(add(1, 2));");
        let output = format_bytecode(&chunk);
        assert!(output.contains("== Function[0]: add"));
        assert!(output.contains("GetL 0")); // renamed from LoadLocal
        assert!(output.contains("GetL 1")); // renamed from LoadLocal
        assert!(output.contains("Add"));
        assert!(output.contains("Ret"));
        assert!(output.contains("Call 0, 2 ; add"));
    }

    #[test]
    fn test_bytecode_control_flow() {
        let chunk = compile("if true { print_debug(1); } else { print_debug(2); }");
        let output = format_bytecode(&chunk);
        assert!(output.contains("PushTrue"));
        assert!(output.contains("JmpIfFalse"));
        assert!(output.contains("Jmp"));
    }

    #[test]
    fn test_bytecode_string_constants() {
        let chunk = compile(r#"let s = "hello"; print_debug(s);"#);
        let output = format_bytecode(&chunk);
        assert!(output.contains("== String Constants =="));
        assert!(output.contains("\"hello\""));
        assert!(output.contains("PushString"));
    }
}
