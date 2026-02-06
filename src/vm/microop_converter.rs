use super::Function;
use super::microop::{ConvertedFunction, MicroOp, VReg};
use super::ops::Op;

/// Convert a function's Op bytecode to MicroOp sequence (Phase 1).
///
/// Phase 1 strategy:
/// - Control flow (Jmp, BrIf, BrIfFalse, Call, Ret) → native MicroOps
/// - All other ops → Raw fallback
/// - All branch targets remapped to MicroOp PC indices
/// - Ops with embedded PC targets (TryBegin) have targets remapped in Raw
pub fn convert(func: &Function) -> ConvertedFunction {
    let code = &func.code;
    let locals_count = func.locals_count;

    // Determine max argc across all Calls for temp allocation
    let max_argc = code
        .iter()
        .filter_map(|op| {
            if let Op::Call(_, argc) = op {
                Some(*argc)
            } else {
                None
            }
        })
        .max()
        .unwrap_or(0);

    // Temps needed: call args (max_argc) + 1 (for return value / branch cond)
    let temps_count = (max_argc + 1).max(1);
    let temp_base = locals_count;

    // Pass 1: Build old_pc → new_pc mapping
    let mut pc_map = Vec::with_capacity(code.len() + 1);
    let mut new_pc = 0;
    for op in code {
        pc_map.push(new_pc);
        new_pc += micro_op_size(op);
    }
    pc_map.push(new_pc); // sentinel for end-of-code

    // Pass 2: Emit MicroOps with resolved targets
    let mut micro_ops = Vec::with_capacity(new_pc);
    for op in code {
        emit_micro_ops(op, &pc_map, temp_base, &mut micro_ops);
    }

    ConvertedFunction {
        micro_ops,
        temps_count,
    }
}

/// Calculate how many MicroOps a single Op generates.
fn micro_op_size(op: &Op) -> usize {
    match op {
        Op::Jmp(_) => 1,
        Op::BrIf(_) | Op::BrIfFalse(_) => 2, // StackPop + Branch
        Op::Call(_, argc) => argc + 2,       // argc StackPops + Call + StackPush
        Op::Ret => 2,                        // StackPop + Ret
        _ => 1,                              // Raw
    }
}

/// Emit MicroOps for a single Op.
fn emit_micro_ops(op: &Op, pc_map: &[usize], temp_base: usize, out: &mut Vec<MicroOp>) {
    match op {
        // ---- Native control flow ----
        Op::Jmp(target) => {
            out.push(MicroOp::Jmp {
                target: pc_map[*target],
            });
        }
        Op::BrIf(target) => {
            let cond = VReg(temp_base);
            out.push(MicroOp::StackPop { dst: cond });
            out.push(MicroOp::BrIf {
                cond,
                target: pc_map[*target],
            });
        }
        Op::BrIfFalse(target) => {
            let cond = VReg(temp_base);
            out.push(MicroOp::StackPop { dst: cond });
            out.push(MicroOp::BrIfFalse {
                cond,
                target: pc_map[*target],
            });
        }
        Op::Call(func_id, argc) => {
            // Pop args from stack in reverse order (last arg first)
            let mut args = Vec::with_capacity(*argc);
            for i in (0..*argc).rev() {
                let vreg = VReg(temp_base + i);
                out.push(MicroOp::StackPop { dst: vreg });
                args.push(vreg);
            }
            args.reverse(); // args[0] = first arg, args[N-1] = last arg

            let ret = VReg(temp_base + argc);
            out.push(MicroOp::Call {
                func_id: *func_id,
                args,
                ret: Some(ret),
            });
            out.push(MicroOp::StackPush { src: ret });
        }
        Op::Ret => {
            let ret = VReg(temp_base);
            out.push(MicroOp::StackPop { dst: ret });
            out.push(MicroOp::Ret { src: Some(ret) });
        }

        // ---- Raw with PC target remapping ----
        Op::TryBegin(handler_pc) => {
            out.push(MicroOp::Raw {
                op: Op::TryBegin(pc_map[*handler_pc]),
            });
        }

        // ---- Raw fallback (no target) ----
        _ => {
            out.push(MicroOp::Raw { op: op.clone() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::microop::MicroOp;

    fn make_func(code: Vec<Op>) -> Function {
        Function {
            name: "test".to_string(),
            arity: 0,
            locals_count: 2,
            code,
            stackmap: None,
            local_types: vec![],
        }
    }

    #[test]
    fn test_empty_function() {
        let func = make_func(vec![]);
        let converted = convert(&func);
        assert!(converted.micro_ops.is_empty());
        assert_eq!(converted.temps_count, 1);
    }

    #[test]
    fn test_raw_only() {
        let func = make_func(vec![Op::I64Const(42), Op::LocalSet(0)]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 2);
        assert_eq!(
            converted.micro_ops[0],
            MicroOp::Raw {
                op: Op::I64Const(42)
            }
        );
        assert_eq!(
            converted.micro_ops[1],
            MicroOp::Raw {
                op: Op::LocalSet(0)
            }
        );
    }

    #[test]
    fn test_jmp_target_remapping() {
        // Op[0]: I64Const(0)     → MicroOp[0]: Raw
        // Op[1]: Jmp(0)          → MicroOp[1]: Jmp { target: 0 }
        let func = make_func(vec![Op::I64Const(0), Op::Jmp(0)]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 2);
        assert_eq!(converted.micro_ops[1], MicroOp::Jmp { target: 0 });
    }

    #[test]
    fn test_br_if_false_expansion() {
        // Op[0]: I64Const(1)     → MicroOp[0]: Raw
        // Op[1]: BrIfFalse(3)    → MicroOp[1]: StackPop, MicroOp[2]: BrIfFalse
        // Op[2]: I64Const(2)     → MicroOp[3]: Raw
        // Op[3]: Ret             → MicroOp[4]: StackPop, MicroOp[5]: Ret
        let func = make_func(vec![
            Op::I64Const(1),
            Op::BrIfFalse(3),
            Op::I64Const(2),
            Op::Ret,
        ]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 6);

        // StackPop + BrIfFalse targeting MicroOp[4] (which is Op[3] = Ret)
        let temp = VReg(2); // locals_count = 2, so temp_base = 2
        assert_eq!(converted.micro_ops[1], MicroOp::StackPop { dst: temp });
        assert_eq!(
            converted.micro_ops[2],
            MicroOp::BrIfFalse {
                cond: temp,
                target: 4
            }
        );
    }

    #[test]
    fn test_call_expansion() {
        // Op[0]: I64Const(10)    → MicroOp[0]: Raw
        // Op[1]: I64Const(20)    → MicroOp[1]: Raw
        // Op[2]: Call(0, 2)      → MicroOp[2..6]: StackPop x2 + Call + StackPush
        // Op[3]: Ret             → MicroOp[6..8]: StackPop + Ret
        let func = make_func(vec![
            Op::I64Const(10),
            Op::I64Const(20),
            Op::Call(0, 2),
            Op::Ret,
        ]);
        let converted = convert(&func);
        // 2 Raw + (2 StackPop + Call + StackPush) + (StackPop + Ret) = 8
        assert_eq!(converted.micro_ops.len(), 8);

        let temp_base = 2; // locals_count = 2
        // StackPop arg1 (last arg, popped first)
        assert_eq!(
            converted.micro_ops[2],
            MicroOp::StackPop {
                dst: VReg(temp_base + 1)
            }
        );
        // StackPop arg0
        assert_eq!(
            converted.micro_ops[3],
            MicroOp::StackPop {
                dst: VReg(temp_base)
            }
        );
        // Call
        assert_eq!(
            converted.micro_ops[4],
            MicroOp::Call {
                func_id: 0,
                args: vec![VReg(temp_base), VReg(temp_base + 1)],
                ret: Some(VReg(temp_base + 2)),
            }
        );
        // StackPush return value
        assert_eq!(
            converted.micro_ops[5],
            MicroOp::StackPush {
                src: VReg(temp_base + 2)
            }
        );

        // temps_count = max_argc(2) + 1 = 3
        assert_eq!(converted.temps_count, 3);
    }

    #[test]
    fn test_try_begin_target_remapping() {
        // Op[0]: TryBegin(2)     → MicroOp[0]: Raw(TryBegin(remapped))
        // Op[1]: I64Const(0)     → MicroOp[1]: Raw
        // Op[2]: TryEnd          → MicroOp[2]: Raw
        let func = make_func(vec![Op::TryBegin(2), Op::I64Const(0), Op::TryEnd]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 3);
        // TryBegin target: Op[2] → MicroOp[2]
        assert_eq!(
            converted.micro_ops[0],
            MicroOp::Raw {
                op: Op::TryBegin(2)
            }
        );
    }

    #[test]
    fn test_pc_mapping_with_mixed_ops() {
        // Op[0]: I64Const(0)     → MicroOp[0]: Raw           (size 1)
        // Op[1]: BrIfFalse(4)    → MicroOp[1,2]: StackPop+Br (size 2)
        // Op[2]: I64Const(1)     → MicroOp[3]: Raw           (size 1)
        // Op[3]: Jmp(0)          → MicroOp[4]: Jmp            (size 1)
        // Op[4]: Ret             → MicroOp[5,6]: StackPop+Ret (size 2)
        let func = make_func(vec![
            Op::I64Const(0),
            Op::BrIfFalse(4),
            Op::I64Const(1),
            Op::Jmp(0),
            Op::Ret,
        ]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 7);

        // BrIfFalse targets Op[4] → MicroOp[5]
        let temp = VReg(2);
        assert_eq!(
            converted.micro_ops[2],
            MicroOp::BrIfFalse {
                cond: temp,
                target: 5
            }
        );
        // Jmp targets Op[0] → MicroOp[0]
        assert_eq!(converted.micro_ops[4], MicroOp::Jmp { target: 0 });
    }
}
