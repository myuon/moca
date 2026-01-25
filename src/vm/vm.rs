use crate::vm::{Chunk, Function, Op, Value};

/// A call frame for the VM.
#[derive(Debug)]
struct Frame {
    /// Index into the function table (usize::MAX for main)
    func_index: usize,
    /// Program counter
    pc: usize,
    /// Base index into the stack for locals
    stack_base: usize,
}

/// The mica virtual machine.
pub struct VM {
    stack: Vec<Value>,
    frames: Vec<Frame>,
}

impl VM {
    pub fn new() -> Self {
        Self {
            stack: Vec::with_capacity(1024),
            frames: Vec::with_capacity(64),
        }
    }

    pub fn run(&mut self, chunk: &Chunk) -> Result<(), String> {
        // Start with main
        self.frames.push(Frame {
            func_index: usize::MAX, // Marker for main
            pc: 0,
            stack_base: 0,
        });

        // Pre-allocate locals for main (estimate based on code)
        // For v0, we'll grow the stack as needed

        loop {
            let frame = self.frames.last_mut().unwrap();
            let func = if frame.func_index == usize::MAX {
                &chunk.main
            } else {
                &chunk.functions[frame.func_index]
            };

            if frame.pc >= func.code.len() {
                // End of function without explicit return
                break;
            }

            let op = func.code[frame.pc].clone();
            frame.pc += 1;

            match op {
                Op::PushInt(n) => {
                    self.stack.push(Value::Int(n));
                }
                Op::PushTrue => {
                    self.stack.push(Value::Bool(true));
                }
                Op::PushFalse => {
                    self.stack.push(Value::Bool(false));
                }
                Op::Pop => {
                    self.stack.pop();
                }
                Op::LoadLocal(slot) => {
                    let frame = self.frames.last().unwrap();
                    let index = frame.stack_base + slot;
                    let value = self.stack.get(index).copied().unwrap_or(Value::Int(0));
                    self.stack.push(value);
                }
                Op::StoreLocal(slot) => {
                    let value = self.stack.pop().ok_or("stack underflow")?;
                    let frame = self.frames.last().unwrap();
                    let index = frame.stack_base + slot;

                    // Ensure stack is large enough
                    while self.stack.len() <= index {
                        self.stack.push(Value::Int(0));
                    }

                    self.stack[index] = value;
                }
                Op::Add => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::Int(a + b));
                }
                Op::Sub => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::Int(a - b));
                }
                Op::Mul => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::Int(a * b));
                }
                Op::Div => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    if b == 0 {
                        return Err("runtime error: division by zero".to_string());
                    }
                    self.stack.push(Value::Int(a / b));
                }
                Op::Mod => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    if b == 0 {
                        return Err("runtime error: division by zero".to_string());
                    }
                    self.stack.push(Value::Int(a % b));
                }
                Op::Neg => {
                    let a = self.pop_int()?;
                    self.stack.push(Value::Int(-a));
                }
                Op::Eq => {
                    let b = self.stack.pop().ok_or("stack underflow")?;
                    let a = self.stack.pop().ok_or("stack underflow")?;
                    self.stack.push(Value::Bool(a == b));
                }
                Op::Ne => {
                    let b = self.stack.pop().ok_or("stack underflow")?;
                    let a = self.stack.pop().ok_or("stack underflow")?;
                    self.stack.push(Value::Bool(a != b));
                }
                Op::Lt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::Bool(a < b));
                }
                Op::Le => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::Bool(a <= b));
                }
                Op::Gt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::Bool(a > b));
                }
                Op::Ge => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::Bool(a >= b));
                }
                Op::Not => {
                    let a = self.stack.pop().ok_or("stack underflow")?;
                    self.stack.push(Value::Bool(!a.is_truthy()));
                }
                Op::Jmp(target) => {
                    let frame = self.frames.last_mut().unwrap();
                    frame.pc = target;
                }
                Op::JmpIfFalse(target) => {
                    let cond = self.stack.pop().ok_or("stack underflow")?;
                    if !cond.is_truthy() {
                        let frame = self.frames.last_mut().unwrap();
                        frame.pc = target;
                    }
                }
                Op::JmpIfTrue(target) => {
                    let cond = self.stack.pop().ok_or("stack underflow")?;
                    if cond.is_truthy() {
                        let frame = self.frames.last_mut().unwrap();
                        frame.pc = target;
                    }
                }
                Op::Call(func_index, argc) => {
                    let func = &chunk.functions[func_index];

                    if argc != func.arity {
                        return Err(format!(
                            "runtime error: function '{}' expects {} arguments, got {}",
                            func.name, func.arity, argc
                        ));
                    }

                    // Calculate the new stack base
                    // Arguments are already on the stack
                    let new_stack_base = self.stack.len() - argc;

                    self.frames.push(Frame {
                        func_index,
                        pc: 0,
                        stack_base: new_stack_base,
                    });
                }
                Op::Ret => {
                    let return_value = self.stack.pop().unwrap_or(Value::Int(0));

                    let frame = self.frames.pop().unwrap();

                    if self.frames.is_empty() {
                        // Returning from main
                        break;
                    }

                    // Clean up the stack (remove locals and arguments)
                    self.stack.truncate(frame.stack_base);

                    // Push return value
                    self.stack.push(return_value);
                }
                Op::Print => {
                    let value = self.stack.pop().ok_or("stack underflow")?;
                    println!("{}", value);
                    // print returns the value it printed (for expression statements)
                    self.stack.push(value);
                }
            }
        }

        Ok(())
    }

    fn pop_int(&mut self) -> Result<i64, String> {
        let value = self.stack.pop().ok_or("stack underflow")?;
        value.as_int().ok_or_else(|| "expected integer".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_code(ops: Vec<Op>) -> Result<Vec<Value>, String> {
        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "__main__".to_string(),
                arity: 0,
                locals_count: 0,
                code: ops,
            },
        };

        let mut vm = VM::new();
        vm.run(&chunk)?;
        Ok(vm.stack)
    }

    #[test]
    fn test_push_int() {
        let stack = run_code(vec![Op::PushInt(42)]).unwrap();
        assert_eq!(stack, vec![Value::Int(42)]);
    }

    #[test]
    fn test_add() {
        let stack = run_code(vec![Op::PushInt(1), Op::PushInt(2), Op::Add]).unwrap();
        assert_eq!(stack, vec![Value::Int(3)]);
    }

    #[test]
    fn test_comparison() {
        let stack = run_code(vec![Op::PushInt(1), Op::PushInt(2), Op::Lt]).unwrap();
        assert_eq!(stack, vec![Value::Bool(true)]);
    }

    #[test]
    fn test_division_by_zero() {
        let result = run_code(vec![Op::PushInt(1), Op::PushInt(0), Op::Div]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("division by zero"));
    }

    #[test]
    fn test_locals() {
        let stack = run_code(vec![
            Op::PushInt(42),
            Op::StoreLocal(0),
            Op::LoadLocal(0),
        ])
        .unwrap();
        assert_eq!(stack, vec![Value::Int(42), Value::Int(42)]);
    }

    #[test]
    fn test_conditional_jump() {
        // if false, skip push 1, else push 2
        let stack = run_code(vec![
            Op::PushFalse,
            Op::JmpIfFalse(4),
            Op::PushInt(1),
            Op::Jmp(5),
            Op::PushInt(2),
        ])
        .unwrap();
        assert_eq!(stack, vec![Value::Int(2)]);
    }
}
