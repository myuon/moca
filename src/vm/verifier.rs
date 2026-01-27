//! Bytecode Verifier for BCVM v0
//!
//! Implements verification rules from the BCVM v0 specification:
//! - Control Flow: Jump targets must be instruction boundaries
//! - Stack Height Consistency: Each basic block has unique entry stack height
//! - Stack Effect Validation: No underflow/overflow
//!
//! Type validation is deferred to a later phase.

#![allow(clippy::collapsible_if)]

use std::collections::{HashMap, HashSet, VecDeque};

use super::Function;
use super::ops::Op;

/// Verification error types
#[derive(Debug, Clone, PartialEq)]
pub enum VerifyError {
    /// Jump target is not a valid instruction index
    InvalidJumpTarget { pc: usize, target: usize },
    /// Stack height mismatch at merge point
    StackHeightMismatch {
        pc: usize,
        expected: usize,
        actual: usize,
    },
    /// Stack underflow
    StackUnderflow {
        pc: usize,
        required: usize,
        actual: usize,
    },
    /// Stack overflow (exceeds max_stack)
    StackOverflow {
        pc: usize,
        height: usize,
        max: usize,
    },
    /// Empty function
    EmptyFunction,
    /// Function does not end with Ret
    MissingReturn,
    /// Missing StackMap entry at safepoint
    MissingStackMap { pc: usize },
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyError::InvalidJumpTarget { pc, target } => {
                write!(
                    f,
                    "invalid jump target at pc={}: target={} is out of bounds",
                    pc, target
                )
            }
            VerifyError::StackHeightMismatch {
                pc,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "stack height mismatch at pc={}: expected {}, got {}",
                    pc, expected, actual
                )
            }
            VerifyError::StackUnderflow {
                pc,
                required,
                actual,
            } => {
                write!(
                    f,
                    "stack underflow at pc={}: requires {} values, but only {} on stack",
                    pc, required, actual
                )
            }
            VerifyError::StackOverflow { pc, height, max } => {
                write!(
                    f,
                    "stack overflow at pc={}: height {} exceeds max_stack {}",
                    pc, height, max
                )
            }
            VerifyError::EmptyFunction => {
                write!(f, "empty function")
            }
            VerifyError::MissingReturn => {
                write!(f, "function does not end with Ret")
            }
            VerifyError::MissingStackMap { pc } => {
                write!(f, "missing StackMap entry at safepoint pc={}", pc)
            }
        }
    }
}

impl std::error::Error for VerifyError {}

/// A basic block in the control flow graph
#[derive(Debug, Clone)]
pub struct BasicBlock {
    /// Start PC (inclusive)
    pub start: usize,
    /// End PC (exclusive)
    pub end: usize,
    /// Successor block indices
    pub successors: Vec<usize>,
}

/// Control flow graph
#[derive(Debug)]
#[allow(clippy::upper_case_acronyms)]
pub struct CFG {
    /// Basic blocks
    pub blocks: Vec<BasicBlock>,
    /// Map from PC to block index
    pub pc_to_block: HashMap<usize, usize>,
}

/// Bytecode verifier
pub struct Verifier {
    /// Maximum stack size (configurable, default 1024)
    pub max_stack: usize,
}

impl Default for Verifier {
    fn default() -> Self {
        Self { max_stack: 1024 }
    }
}

impl Verifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Verify a function
    pub fn verify_function(&self, func: &Function) -> Result<(), VerifyError> {
        if func.code.is_empty() {
            return Err(VerifyError::EmptyFunction);
        }

        // Build CFG
        let cfg = self.build_cfg(func)?;

        // Verify stack heights
        self.verify_stack_heights(func, &cfg)?;

        // Verify StackMap if present
        if let Some(ref stackmap) = func.stackmap {
            self.verify_stackmap(func, stackmap)?;
        }

        Ok(())
    }

    /// Verify StackMap entries exist at all safepoints
    pub fn verify_stackmap(
        &self,
        func: &Function,
        stackmap: &super::stackmap::FunctionStackMap,
    ) -> Result<(), VerifyError> {
        use super::stackmap::is_safepoint;

        for (pc, op) in func.code.iter().enumerate() {
            if is_safepoint(op, pc) {
                if !stackmap.has_safepoint(pc as u32) {
                    return Err(VerifyError::MissingStackMap { pc });
                }
            }
        }

        Ok(())
    }

    /// Build control flow graph from function bytecode
    pub fn build_cfg(&self, func: &Function) -> Result<CFG, VerifyError> {
        let code = &func.code;
        let len = code.len();

        // Find all leader PCs (start of basic blocks)
        let mut leaders: HashSet<usize> = HashSet::new();
        leaders.insert(0); // Entry point is always a leader

        for (pc, op) in code.iter().enumerate() {
            match op {
                Op::Jmp(target)
                | Op::JmpIfTrue(target)
                | Op::JmpIfFalse(target)
                | Op::TryBegin(target) => {
                    // Validate jump target
                    if *target >= len {
                        return Err(VerifyError::InvalidJumpTarget {
                            pc,
                            target: *target,
                        });
                    }
                    leaders.insert(*target);
                    // Instruction after conditional jump is also a leader
                    if !matches!(op, Op::Jmp(_)) && pc + 1 < len {
                        leaders.insert(pc + 1);
                    }
                }
                Op::Ret | Op::Throw => {
                    // Instruction after terminator is a leader (if any)
                    if pc + 1 < len {
                        leaders.insert(pc + 1);
                    }
                }
                _ => {}
            }
        }

        // Sort leaders to build blocks in order
        let mut sorted_leaders: Vec<usize> = leaders.into_iter().collect();
        sorted_leaders.sort();

        // Build basic blocks
        let mut blocks: Vec<BasicBlock> = Vec::new();
        let mut pc_to_block: HashMap<usize, usize> = HashMap::new();

        for (i, &start) in sorted_leaders.iter().enumerate() {
            let end = sorted_leaders.get(i + 1).copied().unwrap_or(len);
            let block_idx = blocks.len();

            // Map all PCs in this block to the block index
            for pc in start..end {
                pc_to_block.insert(pc, block_idx);
            }

            blocks.push(BasicBlock {
                start,
                end,
                successors: Vec::new(),
            });
        }

        // Compute successors
        for block in &mut blocks {
            let last_pc = block.end - 1;
            let last_op = &code[last_pc];

            let mut successors = Vec::new();

            match last_op {
                Op::Jmp(target) => {
                    if let Some(&succ) = pc_to_block.get(target) {
                        successors.push(succ);
                    }
                }
                Op::JmpIfTrue(target) | Op::JmpIfFalse(target) | Op::TryBegin(target) => {
                    // Conditional: fall-through and target
                    if let Some(&succ) = pc_to_block.get(target) {
                        successors.push(succ);
                    }
                    if block.end < len {
                        if let Some(&succ) = pc_to_block.get(&block.end) {
                            if !successors.contains(&succ) {
                                successors.push(succ);
                            }
                        }
                    }
                }
                Op::Ret | Op::Throw => {
                    // No successors (terminator)
                }
                _ => {
                    // Fall-through to next block
                    if block.end < len {
                        if let Some(&succ) = pc_to_block.get(&block.end) {
                            successors.push(succ);
                        }
                    }
                }
            }

            block.successors = successors;
        }

        Ok(CFG {
            blocks,
            pc_to_block,
        })
    }

    /// Verify stack heights using abstract interpretation
    pub fn verify_stack_heights(&self, func: &Function, cfg: &CFG) -> Result<(), VerifyError> {
        let code = &func.code;

        // Stack height at entry of each block (None = not yet visited)
        let mut block_heights: Vec<Option<usize>> = vec![None; cfg.blocks.len()];

        // Worklist for BFS
        let mut worklist: VecDeque<usize> = VecDeque::new();

        // Entry block starts with 0 stack height (arguments are in locals)
        block_heights[0] = Some(0);
        worklist.push_back(0);

        while let Some(block_idx) = worklist.pop_front() {
            let block = &cfg.blocks[block_idx];
            let mut height = block_heights[block_idx].unwrap();

            // Simulate stack effects through the block
            for (pc, op) in code
                .iter()
                .enumerate()
                .skip(block.start)
                .take(block.end - block.start)
            {
                let (pops, pushes) = self.stack_effect(op);

                // Check underflow
                if height < pops {
                    return Err(VerifyError::StackUnderflow {
                        pc,
                        required: pops,
                        actual: height,
                    });
                }

                height = height - pops + pushes;

                // Check overflow
                if height > self.max_stack {
                    return Err(VerifyError::StackOverflow {
                        pc,
                        height,
                        max: self.max_stack,
                    });
                }
            }

            // Propagate to successors
            for &succ_idx in &block.successors {
                match block_heights[succ_idx] {
                    None => {
                        block_heights[succ_idx] = Some(height);
                        worklist.push_back(succ_idx);
                    }
                    Some(existing) => {
                        if existing != height {
                            return Err(VerifyError::StackHeightMismatch {
                                pc: cfg.blocks[succ_idx].start,
                                expected: existing,
                                actual: height,
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Get the stack effect of an operation: (pops, pushes)
    fn stack_effect(&self, op: &Op) -> (usize, usize) {
        match op {
            // Constants: push 1
            Op::PushInt(_)
            | Op::PushFloat(_)
            | Op::PushTrue
            | Op::PushFalse
            | Op::PushNull
            | Op::PushString(_) => (0, 1),

            // Stack operations
            Op::Pop => (1, 0),
            Op::Dup => (0, 1), // Technically reads 1, but doesn't pop

            // Local variables
            Op::GetL(_) => (0, 1),
            Op::SetL(_) => (1, 0),

            // Binary arithmetic: pop 2, push 1
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod => (2, 1),
            Op::AddI64 | Op::SubI64 | Op::MulI64 | Op::DivI64 => (2, 1),
            Op::AddF64 | Op::SubF64 | Op::MulF64 | Op::DivF64 => (2, 1),

            // Unary
            Op::Neg | Op::Not => (1, 1),

            // Comparison: pop 2, push 1
            Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => (2, 1),
            Op::LtI64 | Op::LeI64 | Op::GtI64 | Op::GeI64 | Op::LtF64 => (2, 1),

            // Control flow
            Op::Jmp(_) => (0, 0),
            Op::JmpIfTrue(_) | Op::JmpIfFalse(_) => (1, 0),

            // Functions
            Op::Call(_, argc) => (*argc, 1), // pops argc args, pushes result
            Op::Ret => (1, 0),               // pops return value

            // Heap & Objects
            Op::New(n) => (*n * 2, 1), // pops n key-value pairs, pushes object
            Op::GetF(_) => (1, 1),     // pops object, pushes field value
            Op::SetF(_) => (2, 0),     // pops object and value
            Op::GetFCached(_, _) => (1, 1),
            Op::SetFCached(_, _) => (2, 0),

            // Array operations
            Op::AllocArray(n) => (*n, 1), // pops n elements, pushes array
            Op::ArrayLen => (1, 1),
            Op::ArrayGet => (2, 1),  // pops array and index, pushes value
            Op::ArraySet => (3, 0),  // pops array, index, value
            Op::ArrayPush => (2, 0), // pops array and value
            Op::ArrayPop => (1, 1),  // pops array, pushes value
            Op::ArrayGetInt => (2, 1),

            // String operations
            Op::StringLen => (1, 1),
            Op::StringConcat => (2, 1),

            // Type operations
            Op::TypeOf => (1, 1),
            Op::ToString => (1, 1),
            Op::ParseInt => (1, 1),

            // Exception handling
            Op::Throw => (1, 0),
            Op::TryBegin(_) => (0, 0),
            Op::TryEnd => (0, 0),

            // Builtins
            Op::Print => (1, 0),

            // GC hint
            Op::GcHint(_) => (0, 0),

            // Thread operations
            Op::ThreadSpawn(_) => (0, 1), // pushes handle
            Op::ChannelCreate => (0, 1),  // pushes [sender, receiver]
            Op::ChannelSend => (2, 0),    // pops channel and value
            Op::ChannelRecv => (1, 1),    // pops channel, pushes value
            Op::ThreadJoin => (1, 1),     // pops handle, pushes result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_func(code: Vec<Op>) -> Function {
        Function {
            name: "test".to_string(),
            arity: 0,
            locals_count: 0,
            code,
            stackmap: None,
        }
    }

    #[test]
    fn test_simple_function() {
        let verifier = Verifier::new();
        let func = make_func(vec![Op::PushInt(42), Op::Ret]);
        assert!(verifier.verify_function(&func).is_ok());
    }

    #[test]
    fn test_cfg_build() {
        let verifier = Verifier::new();
        let func = make_func(vec![
            Op::PushTrue,      // 0: block 0
            Op::JmpIfFalse(4), // 1
            Op::PushInt(1),    // 2: block 1
            Op::Jmp(5),        // 3
            Op::PushInt(2),    // 4: block 2
            Op::Ret,           // 5: block 3
        ]);

        let cfg = verifier.build_cfg(&func).unwrap();
        assert_eq!(cfg.blocks.len(), 4);
        assert_eq!(cfg.blocks[0].start, 0);
        assert_eq!(cfg.blocks[0].end, 2);
    }

    #[test]
    fn test_stack_height_mismatch() {
        let verifier = Verifier::new();
        // if true { push 1 } else { push 2; push 3 }
        // This has inconsistent stack heights at merge point
        let func = make_func(vec![
            Op::PushTrue,      // 0: stack=1
            Op::JmpIfFalse(4), // 1: stack=0
            Op::PushInt(1),    // 2: stack=1
            Op::Jmp(6),        // 3: stack=1 -> merge at 6
            Op::PushInt(2),    // 4: stack=1
            Op::PushInt(3),    // 5: stack=2 -> merge at 6 (MISMATCH)
            Op::Ret,           // 6: expects consistent height
        ]);

        let result = verifier.verify_function(&func);
        assert!(matches!(
            result,
            Err(VerifyError::StackHeightMismatch { .. })
        ));
    }

    #[test]
    fn test_stack_underflow() {
        let verifier = Verifier::new();
        let func = make_func(vec![
            Op::Add, // pop 2 but stack is empty
            Op::Ret,
        ]);

        let result = verifier.verify_function(&func);
        assert!(matches!(result, Err(VerifyError::StackUnderflow { .. })));
    }

    #[test]
    fn test_invalid_jump_target() {
        let verifier = Verifier::new();
        let func = make_func(vec![
            Op::Jmp(100), // invalid target
            Op::Ret,
        ]);

        let result = verifier.verify_function(&func);
        assert!(matches!(result, Err(VerifyError::InvalidJumpTarget { .. })));
    }

    #[test]
    fn test_empty_function() {
        let verifier = Verifier::new();
        let func = make_func(vec![]);

        let result = verifier.verify_function(&func);
        assert!(matches!(result, Err(VerifyError::EmptyFunction)));
    }

    #[test]
    fn test_arithmetic_operations() {
        let verifier = Verifier::new();
        let func = make_func(vec![Op::PushInt(1), Op::PushInt(2), Op::Add, Op::Ret]);
        assert!(verifier.verify_function(&func).is_ok());
    }

    #[test]
    fn test_loop() {
        let verifier = Verifier::new();
        // Simple loop: while true { }
        let func = make_func(vec![
            Op::PushTrue,      // 0
            Op::JmpIfFalse(3), // 1: if false, exit
            Op::Jmp(0),        // 2: back to start (backward jump)
            Op::PushNull,      // 3
            Op::Ret,           // 4
        ]);
        assert!(verifier.verify_function(&func).is_ok());
    }

    #[test]
    fn test_stackmap_verification_missing() {
        use crate::vm::stackmap::{FunctionStackMap, StackMapEntry};

        let verifier = Verifier::new();
        let mut func = Function {
            name: "test".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::PushInt(1),
                Op::Call(0, 0), // safepoint at pc=1
                Op::Ret,
            ],
            stackmap: Some(FunctionStackMap::new()), // Empty stackmap
        };

        // Should fail because Call is a safepoint but no StackMap entry exists
        let result = verifier.verify_function(&func);
        assert!(matches!(
            result,
            Err(VerifyError::MissingStackMap { pc: 1 })
        ));

        // Add StackMap entry for the safepoint
        let mut stackmap = FunctionStackMap::new();
        stackmap.add_entry(StackMapEntry::new(1, 1)); // pc=1, stack_height=1
        func.stackmap = Some(stackmap);

        // Should pass now
        assert!(verifier.verify_function(&func).is_ok());
    }

    #[test]
    fn test_no_stackmap_skips_verification() {
        let verifier = Verifier::new();
        let func = Function {
            name: "test".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::PushInt(1),
                Op::Call(0, 0), // safepoint, but no stackmap
                Op::Ret,
            ],
            stackmap: None, // No stackmap, verification skipped
        };

        // Should pass because stackmap is None
        assert!(verifier.verify_function(&func).is_ok());
    }

    // ============================================================
    // BCVM v0 Specification Compliance Tests
    // ============================================================

    /// Test: Spec 7.5 - Stack overflow detection
    #[test]
    fn test_spec_stack_overflow() {
        let verifier = Verifier { max_stack: 2 };
        let func = Function {
            name: "overflow".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::PushInt(1), // height: 1
                Op::PushInt(2), // height: 2
                Op::PushInt(3), // height: 3 -> OVERFLOW (max is 2)
                Op::Ret,
            ],
            stackmap: None,
        };

        let result = verifier.verify_function(&func);
        assert!(result.is_err());
        match result {
            Err(VerifyError::StackOverflow { pc, height, max }) => {
                assert_eq!(pc, 2); // Third instruction
                assert_eq!(height, 3);
                assert_eq!(max, 2);
            }
            other => panic!("Expected StackOverflow, got {:?}", other),
        }
    }

    /// Test: Spec 7.5 - Basic function with return
    /// (MissingReturn check is not yet implemented in verifier)
    #[test]
    fn test_spec_function_with_return() {
        let verifier = Verifier::new();
        let func = Function {
            name: "with_return".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::PushInt(1),
                Op::PushInt(2),
                Op::AddI64,
                Op::Ret, // Proper return
            ],
            stackmap: None,
        };

        // Should pass verification
        let result = verifier.verify_function(&func);
        assert!(result.is_ok());
    }

    /// Test: Spec 7.5 - Stack heights must match at control flow merge points
    #[test]
    fn test_spec_stack_height_at_merge() {
        let verifier = Verifier::new();

        // if-else with different stack heights at merge point
        let func = Function {
            name: "bad_merge".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::PushTrue,      // 0: height: 1
                Op::JmpIfFalse(5), // 1: height: 0, jumps to 5
                Op::PushInt(1),    // 2: height: 1
                Op::PushInt(2),    // 3: height: 2
                Op::Jmp(6),        // 4: jumps to 6 with height 2
                Op::PushInt(3),    // 5: height: 1 (from jump)
                Op::Ret,           // 6: merge point - heights don't match!
            ],
            stackmap: None,
        };

        let result = verifier.verify_function(&func);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(VerifyError::StackHeightMismatch { .. })
        ));
    }

    /// Test: Spec 7.5 - Valid control flow with matching heights
    #[test]
    fn test_spec_valid_control_flow() {
        let verifier = Verifier::new();

        // if-else with same stack heights at merge point
        let func = Function {
            name: "good_merge".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::PushTrue,      // 0: height: 1
                Op::JmpIfFalse(4), // 1: height: 0, jumps to 4
                Op::PushInt(1),    // 2: height: 1
                Op::Jmp(5),        // 3: jumps to 5 with height 1
                Op::PushInt(2),    // 4: height: 1 (from jump)
                Op::Ret,           // 5: merge point - heights match!
            ],
            stackmap: None,
        };

        let result = verifier.verify_function(&func);
        assert!(result.is_ok());
    }

    /// Test: Spec 7.3 - CALL is a safepoint requiring StackMap
    #[test]
    fn test_spec_call_safepoint() {
        use crate::vm::stackmap::{FunctionStackMap, StackMapEntry};

        let mut stackmap = FunctionStackMap::new();
        // Add entry for the CALL instruction at pc=1
        stackmap.add_entry(StackMapEntry::new(1, 1));

        let verifier = Verifier::new();
        let func = Function {
            name: "with_call".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::PushInt(0), // 0: function index
                Op::Call(0, 0), // 1: CALL is safepoint
                Op::Ret,        // 2: return
            ],
            stackmap: Some(stackmap),
        };

        assert!(verifier.verify_function(&func).is_ok());
    }

    /// Test: Spec 7.3 - NEW is a safepoint requiring StackMap
    #[test]
    fn test_spec_new_safepoint() {
        use crate::vm::stackmap::{FunctionStackMap, StackMapEntry};

        let mut stackmap = FunctionStackMap::new();
        // Add entry for the NEW instruction at pc=2
        stackmap.add_entry(StackMapEntry::new(2, 2));

        let verifier = Verifier::new();
        let func = Function {
            name: "with_new".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::PushString(0), // 0: field name
                Op::PushInt(42),   // 1: field value
                Op::New(1),        // 2: NEW is safepoint (allocates object)
                Op::Ret,           // 3: return
            ],
            stackmap: Some(stackmap),
        };

        assert!(verifier.verify_function(&func).is_ok());
    }

    /// Test: Spec 7.3 - Backward jump is a safepoint
    #[test]
    fn test_spec_backward_jump_safepoint() {
        use crate::vm::stackmap::{FunctionStackMap, StackMapEntry};

        let mut stackmap = FunctionStackMap::new();
        // Add entry for the backward jump at pc=2
        stackmap.add_entry(StackMapEntry::new(2, 0));

        let verifier = Verifier::new();
        let func = Function {
            name: "loop".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                // Simple infinite loop (for verification purposes)
                Op::PushTrue,      // 0: height 1
                Op::JmpIfFalse(3), // 1: height 0, jumps to 3 or falls through
                Op::Jmp(0),        // 2: backward jump is safepoint, height 0
                Op::PushInt(0),    // 3: exit point, height 1
                Op::Ret,           // 4: return
            ],
            stackmap: Some(stackmap),
        };

        assert!(verifier.verify_function(&func).is_ok());
    }

    /// Test: Spec 7.5 - Jump target must be valid instruction boundary
    #[test]
    fn test_spec_jump_to_invalid_pc() {
        let verifier = Verifier::new();
        let func = Function {
            name: "bad_jump".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::Jmp(100), // Jump to non-existent instruction
                Op::Ret,
            ],
            stackmap: None,
        };

        let result = verifier.verify_function(&func);
        assert!(result.is_err());
        assert!(matches!(result, Err(VerifyError::InvalidJumpTarget { .. })));
    }

    /// Test: Spec - All typed arithmetic operations have correct stack effects
    #[test]
    fn test_spec_typed_arithmetic() {
        let verifier = Verifier::new();

        // I64 arithmetic
        let func = Function {
            name: "i64_arith".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::PushInt(10),
                Op::PushInt(5),
                Op::AddI64, // 10 + 5 = 15
                Op::PushInt(3),
                Op::SubI64, // 15 - 3 = 12
                Op::PushInt(2),
                Op::MulI64, // 12 * 2 = 24
                Op::PushInt(4),
                Op::DivI64, // 24 / 4 = 6
                Op::Ret,
            ],
            stackmap: None,
        };
        assert!(verifier.verify_function(&func).is_ok());

        // F64 arithmetic
        let func = Function {
            name: "f64_arith".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::PushFloat(10.0),
                Op::PushFloat(5.0),
                Op::AddF64,
                Op::PushFloat(3.0),
                Op::SubF64,
                Op::PushFloat(2.0),
                Op::MulF64,
                Op::PushFloat(4.0),
                Op::DivF64,
                Op::Ret,
            ],
            stackmap: None,
        };
        assert!(verifier.verify_function(&func).is_ok());
    }

    /// Test: Spec - Comparison operations have correct stack effects
    #[test]
    fn test_spec_comparison_ops() {
        let verifier = Verifier::new();

        let func = Function {
            name: "compare".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![
                Op::PushInt(1),
                Op::PushInt(2),
                Op::LtI64, // 1 < 2 = true
                Op::PushFloat(1.5),
                Op::PushFloat(2.5),
                Op::LtF64, // 1.5 < 2.5 = true
                Op::Eq,    // true == true = true
                Op::Ret,
            ],
            stackmap: None,
        };
        assert!(verifier.verify_function(&func).is_ok());
    }
}
