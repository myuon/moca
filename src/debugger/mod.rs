// Some fields are stored for future use
#![allow(dead_code)]

use std::collections::HashSet;
use std::io::{self};
use std::path::Path;

use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::compiler::{Codegen, ModuleLoader, Resolver};
use crate::vm::{Chunk, Heap, Op, Value};

/// Debugger state.
pub struct Debugger {
    /// Source code lines
    source_lines: Vec<String>,
    /// Compiled chunk
    chunk: Chunk,
    /// Current program counter
    pc: usize,
    /// Current function index (-1 for main)
    func_index: i32,
    /// Breakpoints: (func_index, pc)
    breakpoints: HashSet<(i32, usize)>,
    /// Line-based breakpoints: line number (1-based)
    line_breakpoints: HashSet<usize>,
    /// VM stack
    stack: Vec<Value>,
    /// VM locals
    locals: Vec<Value>,
    /// VM heap
    heap: Heap,
    /// Call stack for backtrace
    call_stack: Vec<CallFrame>,
    /// Whether the debugger is running
    running: bool,
    /// Whether the program has ended
    finished: bool,
    /// Command input buffer
    input: String,
    /// Status message
    status: String,
    /// Output messages
    output: Vec<String>,
}

/// A call frame for the backtrace.
#[derive(Clone)]
struct CallFrame {
    func_name: String,
    pc: usize,
    line: usize,
}

impl Debugger {
    /// Create a new debugger for a source file.
    pub fn new(path: &Path) -> Result<Self, String> {
        // Read source
        let source =
            std::fs::read_to_string(path).map_err(|e| format!("failed to read file: {}", e))?;
        let source_lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();

        // Compile
        let root_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut loader = ModuleLoader::new(root_dir);
        let program = loader.load_with_imports(path)?;

        let filename = path.to_string_lossy().to_string();
        let mut resolver = Resolver::new(&filename);
        let resolved = resolver.resolve(program)?;

        let mut codegen = Codegen::new();
        let chunk = codegen.compile(resolved)?;

        Ok(Self {
            source_lines,
            chunk,
            pc: 0,
            func_index: -1, // Main
            breakpoints: HashSet::new(),
            line_breakpoints: HashSet::new(),
            stack: Vec::new(),
            locals: vec![Value::Null; 256], // Pre-allocate locals
            heap: Heap::new(),
            call_stack: vec![CallFrame {
                func_name: "__main__".to_string(),
                pc: 0,
                line: 1,
            }],
            running: true,
            finished: false,
            input: String::new(),
            status: "Ready. Type 'h' for help.".to_string(),
            output: Vec::new(),
        })
    }

    /// Get the current instruction.
    fn current_op(&self) -> Option<&Op> {
        let code = if self.func_index < 0 {
            &self.chunk.main.code
        } else {
            &self.chunk.functions[self.func_index as usize].code
        };
        code.get(self.pc)
    }

    /// Get current line number (approximate).
    fn current_line(&self) -> usize {
        // Simple heuristic: use PC as line offset from start
        // In a real implementation, we'd use the line table
        (self.pc + 1).min(self.source_lines.len()).max(1)
    }

    /// Execute one instruction.
    fn step(&mut self) {
        if self.finished {
            self.status = "Program has ended.".to_string();
            return;
        }

        let op = match self.current_op() {
            Some(op) => op.clone(),
            None => {
                self.finished = true;
                self.status = "Program finished.".to_string();
                return;
            }
        };

        // Execute the operation (simplified VM)
        match &op {
            Op::PushInt(v) => self.stack.push(Value::I64(*v)),
            Op::PushFloat(v) => self.stack.push(Value::F64(*v)),
            Op::PushTrue => self.stack.push(Value::Bool(true)),
            Op::PushFalse => self.stack.push(Value::Bool(false)),
            Op::PushNull => self.stack.push(Value::Null),
            Op::Pop => {
                self.stack.pop();
            }
            Op::GetL(slot) => {
                let val = self.locals[*slot];
                self.stack.push(val);
            }
            Op::SetL(slot) => {
                if let Some(val) = self.stack.pop() {
                    self.locals[*slot] = val;
                }
            }
            Op::Add => self.binary_op(|a, b| a + b),
            Op::Sub => self.binary_op(|a, b| a - b),
            Op::Mul => self.binary_op(|a, b| a * b),
            Op::Div => self.binary_op(|a, b| if b != 0 { a / b } else { 0 }),
            Op::Neg => {
                if let Some(Value::I64(v)) = self.stack.pop() {
                    self.stack.push(Value::I64(-v));
                }
            }
            Op::PrintDebug => {
                if let Some(val) = self.stack.last() {
                    self.output.push(self.format_value(val).to_string());
                }
                self.stack.pop();
            }
            Op::Ret => {
                self.finished = true;
                self.status = "Program returned.".to_string();
                return;
            }
            Op::Jmp(target) => {
                self.pc = *target;
                return; // Don't increment PC
            }
            Op::JmpIfFalse(target) => {
                if let Some(val) = self.stack.pop()
                    && !self.is_truthy(&val)
                {
                    self.pc = *target;
                    return;
                }
            }
            _ => {
                // Other ops not fully implemented for debugger
                self.status = format!("Executed: {:?}", op);
            }
        }

        self.pc += 1;
        self.status = format!("PC: {}, Stack size: {}", self.pc, self.stack.len());
    }

    fn binary_op<F>(&mut self, op: F)
    where
        F: Fn(i64, i64) -> i64,
    {
        if let (Some(Value::I64(b)), Some(Value::I64(a))) = (self.stack.pop(), self.stack.pop()) {
            self.stack.push(Value::I64(op(a, b)));
        }
    }

    fn is_truthy(&self, val: &Value) -> bool {
        match val {
            Value::Bool(b) => *b,
            Value::Null => false,
            Value::I64(0) => false,
            _ => true,
        }
    }

    fn format_value(&self, val: &Value) -> String {
        match val {
            Value::Null => "nil".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::I64(i) => i.to_string(),
            Value::F64(f) => f.to_string(),
            Value::Ref(_) => "<object>".to_string(),
        }
    }

    /// Continue execution until breakpoint or end.
    fn continue_exec(&mut self) {
        loop {
            if self.finished {
                break;
            }
            self.step();
            // Check breakpoints
            if self.line_breakpoints.contains(&self.current_line()) {
                self.status = format!("Breakpoint hit at line {}", self.current_line());
                break;
            }
        }
    }

    /// Set a breakpoint at line number.
    fn set_breakpoint(&mut self, line: usize) {
        self.line_breakpoints.insert(line);
        self.status = format!("Breakpoint set at line {}", line);
    }

    /// Delete a breakpoint at line number.
    fn delete_breakpoint(&mut self, line: usize) {
        self.line_breakpoints.remove(&line);
        self.status = format!("Breakpoint deleted at line {}", line);
    }

    /// Process a command.
    fn process_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return;
        }

        match parts[0] {
            "s" | "step" => self.step(),
            "n" | "next" => self.step(), // Same as step for now
            "c" | "continue" => self.continue_exec(),
            "b" => {
                if parts.len() > 1 {
                    if let Ok(line) = parts[1].parse::<usize>() {
                        self.set_breakpoint(line);
                    }
                } else {
                    self.status = "Usage: b <line>".to_string();
                }
            }
            "d" => {
                if parts.len() > 1 {
                    if let Ok(line) = parts[1].parse::<usize>() {
                        self.delete_breakpoint(line);
                    }
                } else {
                    self.status = "Usage: d <line>".to_string();
                }
            }
            "bl" => {
                let bps: Vec<String> = self
                    .line_breakpoints
                    .iter()
                    .map(|l| l.to_string())
                    .collect();
                self.status = format!("Breakpoints: {}", bps.join(", "));
            }
            "p" => {
                if parts.len() > 1 {
                    // Print local variable by slot
                    if let Ok(slot) = parts[1].parse::<usize>() {
                        let val = &self.locals[slot];
                        self.status = format!("slot[{}] = {}", slot, self.format_value(val));
                    } else {
                        self.status = format!("Unknown: {}", parts[1]);
                    }
                } else {
                    self.status = "Usage: p <slot>".to_string();
                }
            }
            "locals" => {
                let mut locals_str = String::new();
                for (i, val) in self.locals.iter().enumerate().take(8) {
                    if !matches!(val, Value::Null) {
                        locals_str.push_str(&format!("[{}]={} ", i, self.format_value(val)));
                    }
                }
                self.status = format!("Locals: {}", locals_str);
            }
            "bt" | "backtrace" => {
                let bt: Vec<String> = self
                    .call_stack
                    .iter()
                    .enumerate()
                    .map(|(i, f)| format!("#{} {} (pc:{})", i, f.func_name, f.pc))
                    .collect();
                self.status = format!("Stack: {}", bt.join(" -> "));
            }
            "q" | "quit" => {
                self.running = false;
            }
            "h" | "help" => {
                self.status = "Commands: s(tep) n(ext) c(ontinue) b <line> d <line> bl p <slot> locals bt q(uit)".to_string();
            }
            _ => {
                self.status = format!("Unknown command: {}", parts[0]);
            }
        }
    }

    /// Run the debugger TUI.
    pub fn run(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        stdout.execute(EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        while self.running {
            terminal.draw(|frame| self.ui(frame))?;

            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                match key.code {
                    KeyCode::Enter => {
                        let cmd = self.input.clone();
                        self.input.clear();
                        self.process_command(&cmd);
                    }
                    KeyCode::Char(c) => {
                        self.input.push(c);
                    }
                    KeyCode::Backspace => {
                        self.input.pop();
                    }
                    KeyCode::Esc => {
                        self.running = false;
                    }
                    _ => {}
                }
            }
        }

        disable_raw_mode()?;
        terminal.backend_mut().execute(LeaveAlternateScreen)?;
        Ok(())
    }

    fn ui(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),   // Source
                Constraint::Length(6), // Locals + Stack
                Constraint::Length(5), // Output
                Constraint::Length(3), // Status
                Constraint::Length(3), // Input
            ])
            .split(frame.area());

        // Source view
        let current_line = self.current_line();
        let source_items: Vec<ListItem> = self
            .source_lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let line_num = i + 1;
                let prefix = if self.line_breakpoints.contains(&line_num) {
                    "●"
                } else {
                    " "
                };
                let marker = if line_num == current_line { "▶" } else { " " };
                let content = format!("{}{}{:4} │ {}", prefix, marker, line_num, line);
                let style = if line_num == current_line {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };
                ListItem::new(content).style(style)
            })
            .collect();

        let source =
            List::new(source_items).block(Block::default().title("Source").borders(Borders::ALL));
        frame.render_widget(source, chunks[0]);

        // Locals and stack
        let info_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

        let locals_text: Vec<String> = self
            .locals
            .iter()
            .enumerate()
            .take(8)
            .filter(|(_, v)| !matches!(v, Value::Null))
            .map(|(i, v)| format!("[{}] = {}", i, self.format_value(v)))
            .collect();
        let locals = Paragraph::new(locals_text.join("\n"))
            .block(Block::default().title("Locals").borders(Borders::ALL));
        frame.render_widget(locals, info_chunks[0]);

        let stack_text: Vec<String> = self
            .stack
            .iter()
            .rev()
            .take(5)
            .map(|v| self.format_value(v))
            .collect();
        let stack = Paragraph::new(stack_text.join("\n"))
            .block(Block::default().title("Stack").borders(Borders::ALL));
        frame.render_widget(stack, info_chunks[1]);

        // Output
        let output_text = self
            .output
            .iter()
            .rev()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let output = Paragraph::new(output_text)
            .block(Block::default().title("Output").borders(Borders::ALL));
        frame.render_widget(output, chunks[2]);

        // Status
        let status = Paragraph::new(self.status.clone())
            .block(Block::default().title("Status").borders(Borders::ALL));
        frame.render_widget(status, chunks[3]);

        // Input
        let input = Paragraph::new(format!("(debug) {}", self.input))
            .block(Block::default().title("Command").borders(Borders::ALL));
        frame.render_widget(input, chunks[4]);
    }
}

/// Run the debugger on a file.
pub fn run_debugger(path: &Path) -> Result<(), String> {
    let mut debugger = Debugger::new(path)?;
    debugger.run().map_err(|e| e.to_string())
}
