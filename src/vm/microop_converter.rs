use std::collections::HashSet;

use super::Function;
use super::microop::{CmpCond, ConvertedFunction, MicroOp, VReg};
use super::ops::Op;

/// Convert a function's Op bytecode to MicroOp sequence (Phase 2).
///
/// Uses virtual-stack simulation to convert stack-based Ops to register-based
/// MicroOps. Control flow and arithmetic/comparison/constants/locals are
/// converted to native MicroOps. All other ops use Raw fallback with
/// StackPush/StackPop bridge.
///
/// Algorithm:
/// 1. Identify branch targets (join points)
/// 2. Walk ops linearly, maintaining a virtual stack of VReg indices
/// 3. At join points, flush virtual stack (ensure it's empty for consistency)
/// 4. Allocate temp VRegs for intermediate results
/// 5. Store old Op PC as branch target, fix up in final pass
pub fn convert(func: &Function) -> ConvertedFunction {
    let code = &func.code;
    let locals_count = func.locals_count;

    // Identify branch targets (PCs that are targets of jumps/branches)
    let mut branch_targets = HashSet::new();
    for op in code {
        match op {
            Op::Jmp(t) | Op::BrIf(t) | Op::BrIfFalse(t) => {
                branch_targets.insert(*t);
            }
            _ => {}
        }
    }

    let mut micro_ops: Vec<MicroOp> = Vec::new();
    let mut pc_map: Vec<usize> = Vec::with_capacity(code.len() + 1);
    let mut vstack: Vec<VReg> = Vec::new();
    let mut next_temp = locals_count;
    let mut max_temp = locals_count;

    for (old_pc, op) in code.iter().enumerate() {
        // At branch targets, flush vstack BEFORE recording pc_map.
        // This way branches skip the flush (which is only for the fall-through
        // path). Values from the branch path are already on the real stack.
        if branch_targets.contains(&old_pc) {
            flush_vstack(&mut vstack, &mut micro_ops);
            // Reset temp allocator at basic block boundary
            next_temp = locals_count;
        }

        pc_map.push(micro_ops.len());

        match op {
            // ============================================================
            // Constants → register-based
            // ============================================================
            Op::I64Const(n) => {
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::ConstI64 { dst, imm: *n });
                vstack.push(dst);
            }
            Op::I32Const(n) => {
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::ConstI32 { dst, imm: *n });
                vstack.push(dst);
            }
            Op::F64Const(f) => {
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::ConstF64 { dst, imm: *f });
                vstack.push(dst);
            }
            Op::F32Const(f) => {
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::ConstF32 { dst, imm: *f });
                vstack.push(dst);
            }
            Op::RefNull => {
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::RefNull { dst });
                vstack.push(dst);
            }

            // ============================================================
            // Locals → direct VReg push / materialize-on-write
            // ============================================================
            Op::LocalGet(slot) => {
                // Push the local's VReg directly (no Mov).
                // If LocalSet overwrites this local while it's still on vstack,
                // LocalSet will materialize the stale references first.
                vstack.push(VReg(*slot));
            }
            Op::LocalSet(slot) => {
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = VReg(*slot);
                if src != dst {
                    // Materialize any vstack references to this local before overwriting
                    for entry in vstack.iter_mut() {
                        if *entry == dst {
                            let temp = alloc_temp(&mut next_temp, &mut max_temp);
                            micro_ops.push(MicroOp::Mov {
                                dst: temp,
                                src: dst,
                            });
                            *entry = temp;
                        }
                    }
                    micro_ops.push(MicroOp::Mov { dst, src });
                }
            }

            // ============================================================
            // Stack manipulation
            // ============================================================
            Op::Drop => {
                // Pop and discard
                let _ = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
            }
            Op::Dup => {
                let top = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                vstack.push(top);
                vstack.push(top);
            }

            // ============================================================
            // i64 Arithmetic → register-based
            // ============================================================
            Op::I64Add => {
                let b = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let a = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::AddI64 { dst, a, b });
                vstack.push(dst);
            }
            Op::I64Sub => {
                let b = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let a = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::SubI64 { dst, a, b });
                vstack.push(dst);
            }
            Op::I64Mul => {
                let b = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let a = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::MulI64 { dst, a, b });
                vstack.push(dst);
            }
            Op::I64DivS => {
                let b = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let a = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::DivI64 { dst, a, b });
                vstack.push(dst);
            }
            Op::I64RemS => {
                let b = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let a = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::RemI64 { dst, a, b });
                vstack.push(dst);
            }
            Op::I64Neg => {
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::NegI64 { dst, src });
                vstack.push(dst);
            }

            // ============================================================
            // i32 Arithmetic → register-based
            // ============================================================
            Op::I32Add => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::AddI32 { dst, a, b },
                );
            }
            Op::I32Sub => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::SubI32 { dst, a, b },
                );
            }
            Op::I32Mul => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::MulI32 { dst, a, b },
                );
            }
            Op::I32DivS => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::DivI32 { dst, a, b },
                );
            }
            Op::I32RemS => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::RemI32 { dst, a, b },
                );
            }
            Op::I32Eqz => {
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::EqzI32 { dst, src });
                vstack.push(dst);
            }

            // ============================================================
            // f64 Arithmetic → register-based
            // ============================================================
            Op::F64Add => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::AddF64 { dst, a, b },
                );
            }
            Op::F64Sub => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::SubF64 { dst, a, b },
                );
            }
            Op::F64Mul => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::MulF64 { dst, a, b },
                );
            }
            Op::F64Div => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::DivF64 { dst, a, b },
                );
            }
            Op::F64Neg => {
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::NegF64 { dst, src });
                vstack.push(dst);
            }

            // ============================================================
            // f32 Arithmetic → register-based
            // ============================================================
            Op::F32Add => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::AddF32 { dst, a, b },
                );
            }
            Op::F32Sub => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::SubF32 { dst, a, b },
                );
            }
            Op::F32Mul => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::MulF32 { dst, a, b },
                );
            }
            Op::F32Div => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::DivF32 { dst, a, b },
                );
            }
            Op::F32Neg => {
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::NegF32 { dst, src });
                vstack.push(dst);
            }

            // ============================================================
            // i64 Comparisons → CmpI64
            // ============================================================
            Op::I64Eq => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::Eq,
            ),
            Op::I64Ne => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::Ne,
            ),
            Op::I64LtS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::LtS,
            ),
            Op::I64LeS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::LeS,
            ),
            Op::I64GtS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::GtS,
            ),
            Op::I64GeS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::GeS,
            ),

            // ============================================================
            // i32 Comparisons → CmpI32
            // ============================================================
            Op::I32Eq => emit_cmp_i32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::Eq,
            ),
            Op::I32Ne => emit_cmp_i32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::Ne,
            ),
            Op::I32LtS => emit_cmp_i32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::LtS,
            ),
            Op::I32LeS => emit_cmp_i32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::LeS,
            ),
            Op::I32GtS => emit_cmp_i32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::GtS,
            ),
            Op::I32GeS => emit_cmp_i32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::GeS,
            ),

            // ============================================================
            // f64 Comparisons → CmpF64
            // ============================================================
            Op::F64Eq => emit_cmp_f64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::Eq,
            ),
            Op::F64Ne => emit_cmp_f64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::Ne,
            ),
            Op::F64Lt => emit_cmp_f64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::LtS,
            ),
            Op::F64Le => emit_cmp_f64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::LeS,
            ),
            Op::F64Gt => emit_cmp_f64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::GtS,
            ),
            Op::F64Ge => emit_cmp_f64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::GeS,
            ),

            // ============================================================
            // f32 Comparisons → CmpF32
            // ============================================================
            Op::F32Eq => emit_cmp_f32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::Eq,
            ),
            Op::F32Ne => emit_cmp_f32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::Ne,
            ),
            Op::F32Lt => emit_cmp_f32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::LtS,
            ),
            Op::F32Le => emit_cmp_f32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::LeS,
            ),
            Op::F32Gt => emit_cmp_f32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::GtS,
            ),
            Op::F32Ge => emit_cmp_f32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::GeS,
            ),

            // ============================================================
            // Ref operations → register-based
            // ============================================================
            Op::RefEq => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::RefEq { dst, a, b },
                );
            }
            Op::RefIsNull => {
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::RefIsNull { dst, src });
                vstack.push(dst);
            }

            // ============================================================
            // Type conversions → register-based (all unary: pop 1, push 1)
            // ============================================================
            Op::I32WrapI64 => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::I32WrapI64 { dst, src },
            ),
            Op::I64ExtendI32S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::I64ExtendI32S { dst, src },
            ),
            Op::I64ExtendI32U => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::I64ExtendI32U { dst, src },
            ),
            Op::F64ConvertI64S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::F64ConvertI64S { dst, src },
            ),
            Op::I64TruncF64S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::I64TruncF64S { dst, src },
            ),
            Op::F64ConvertI32S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::F64ConvertI32S { dst, src },
            ),
            Op::F32ConvertI32S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::F32ConvertI32S { dst, src },
            ),
            Op::F32ConvertI64S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::F32ConvertI64S { dst, src },
            ),
            Op::I32TruncF32S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::I32TruncF32S { dst, src },
            ),
            Op::I32TruncF64S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::I32TruncF64S { dst, src },
            ),
            Op::I64TruncF32S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::I64TruncF32S { dst, src },
            ),
            Op::F32DemoteF64 => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::F32DemoteF64 { dst, src },
            ),
            Op::F64PromoteF32 => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                |dst, src| MicroOp::F64PromoteF32 { dst, src },
            ),

            // ============================================================
            // Control Flow → native MicroOps
            // ============================================================
            Op::Jmp(target) => {
                flush_vstack(&mut vstack, &mut micro_ops);
                micro_ops.push(MicroOp::Jmp {
                    target: *target, // old Op PC, fixed up later
                    old_pc,
                    old_target: *target,
                });
                // After unconditional jump, reset temps (dead code until next target)
                next_temp = locals_count;
            }
            Op::BrIf(target) => {
                let cond = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                flush_vstack(&mut vstack, &mut micro_ops);
                micro_ops.push(MicroOp::BrIf {
                    cond,
                    target: *target, // old Op PC, fixed up later
                });
            }
            Op::BrIfFalse(target) => {
                let cond = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                flush_vstack(&mut vstack, &mut micro_ops);
                micro_ops.push(MicroOp::BrIfFalse {
                    cond,
                    target: *target, // old Op PC, fixed up later
                });
            }
            Op::Call(func_id, argc) => {
                let mut args = Vec::with_capacity(*argc);
                for _ in 0..*argc {
                    args.push(pop_vreg(
                        &mut vstack,
                        &mut micro_ops,
                        &mut next_temp,
                        &mut max_temp,
                    ));
                }
                args.reverse();
                flush_vstack(&mut vstack, &mut micro_ops);
                let ret = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::Call {
                    func_id: *func_id,
                    args,
                    ret: Some(ret),
                });
                vstack.push(ret);
            }
            Op::Ret => {
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::Ret { src: Some(src) });
                next_temp = locals_count;
            }

            // ============================================================
            // Raw with PC target remapping
            // ============================================================
            Op::TryBegin(handler_pc) => {
                flush_vstack(&mut vstack, &mut micro_ops);
                micro_ops.push(MicroOp::Raw {
                    op: Op::TryBegin(*handler_pc), // old Op PC, fixed up later
                });
            }

            // ============================================================
            // Raw fallback (everything else)
            // ============================================================
            _ => {
                flush_vstack(&mut vstack, &mut micro_ops);
                micro_ops.push(MicroOp::Raw { op: op.clone() });
                // vstack stays empty; any values the Raw op pushes remain
                // on the real operand stack and will be StackPop-ed by
                // subsequent register-based ops.
            }
        }
    }
    pc_map.push(micro_ops.len()); // sentinel for end-of-code

    // Fix up branch targets: currently store old Op PC, resolve to MicroOp PC
    for mop in &mut micro_ops {
        match mop {
            MicroOp::Jmp { target, .. } => *target = pc_map[*target],
            MicroOp::BrIf { target, .. } => *target = pc_map[*target],
            MicroOp::BrIfFalse { target, .. } => *target = pc_map[*target],
            MicroOp::Raw {
                op: Op::TryBegin(handler_pc),
            } => *handler_pc = pc_map[*handler_pc],
            _ => {}
        }
    }

    let temps_count = max_temp - locals_count;

    ConvertedFunction {
        micro_ops,
        temps_count: temps_count.max(1), // at least 1 for safety
        pc_map,
    }
}

// ============================================================
// Helpers
// ============================================================

fn alloc_temp(next_temp: &mut usize, max_temp: &mut usize) -> VReg {
    let v = VReg(*next_temp);
    *next_temp += 1;
    if *next_temp > *max_temp {
        *max_temp = *next_temp;
    }
    v
}

/// Pop a VReg from the virtual stack, or emit StackPop from the real stack.
fn pop_vreg(
    vstack: &mut Vec<VReg>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
) -> VReg {
    if let Some(v) = vstack.pop() {
        v
    } else {
        let t = alloc_temp(next_temp, max_temp);
        micro_ops.push(MicroOp::StackPop { dst: t });
        t
    }
}

/// Flush all virtual stack entries to the real operand stack.
fn flush_vstack(vstack: &mut Vec<VReg>, micro_ops: &mut Vec<MicroOp>) {
    for v in vstack.drain(..) {
        micro_ops.push(MicroOp::StackPush { src: v });
    }
}

/// Emit a binary op: pop 2, push 1 result.
fn emit_binop(
    vstack: &mut Vec<VReg>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    make_op: impl FnOnce(VReg, VReg, VReg) -> MicroOp,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    micro_ops.push(make_op(dst, a, b));
    vstack.push(dst);
}

/// Emit a comparison: pop 2, push 1 boolean result.
fn emit_cmp_i64(
    vstack: &mut Vec<VReg>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    cond: CmpCond,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    micro_ops.push(MicroOp::CmpI64 { dst, a, b, cond });
    vstack.push(dst);
}

fn emit_cmp_i32(
    vstack: &mut Vec<VReg>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    cond: CmpCond,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    micro_ops.push(MicroOp::CmpI32 { dst, a, b, cond });
    vstack.push(dst);
}

fn emit_cmp_f64(
    vstack: &mut Vec<VReg>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    cond: CmpCond,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    micro_ops.push(MicroOp::CmpF64 { dst, a, b, cond });
    vstack.push(dst);
}

fn emit_cmp_f32(
    vstack: &mut Vec<VReg>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    cond: CmpCond,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    micro_ops.push(MicroOp::CmpF32 { dst, a, b, cond });
    vstack.push(dst);
}

/// Emit a unary type conversion: pop 1, push 1.
fn emit_unary_conv(
    vstack: &mut Vec<VReg>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    make_op: impl FnOnce(VReg, VReg) -> MicroOp,
) {
    let src = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    micro_ops.push(make_op(dst, src));
    vstack.push(dst);
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
    }

    #[test]
    fn test_const_and_local_set() {
        // I64Const(42) → ConstI64 { dst: t0, imm: 42 }
        // LocalSet(0)  → Mov { dst: VReg(0), src: t0 }
        let func = make_func(vec![Op::I64Const(42), Op::LocalSet(0)]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 2);
        assert_eq!(
            converted.micro_ops[0],
            MicroOp::ConstI64 {
                dst: VReg(2),
                imm: 42
            }
        );
        assert_eq!(
            converted.micro_ops[1],
            MicroOp::Mov {
                dst: VReg(0),
                src: VReg(2)
            }
        );
    }

    #[test]
    fn test_local_get_and_add() {
        // LocalGet(0) → push VReg(0) directly (no Mov)
        // LocalGet(1) → push VReg(1) directly (no Mov)
        // I64Add      → AddI64 { dst: t0, a: VReg(0), b: VReg(1) }
        // LocalSet(0) → Mov { dst: VReg(0), src: t0 }
        let func = make_func(vec![
            Op::LocalGet(0),
            Op::LocalGet(1),
            Op::I64Add,
            Op::LocalSet(0),
        ]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 2);

        let t0 = VReg(2);
        assert_eq!(
            converted.micro_ops[0],
            MicroOp::AddI64 {
                dst: t0,
                a: VReg(0),
                b: VReg(1)
            }
        );
        assert_eq!(
            converted.micro_ops[1],
            MicroOp::Mov {
                dst: VReg(0),
                src: t0
            }
        );
    }

    #[test]
    fn test_jmp_target_remapping() {
        // Op[0]: I64Const(0)     → ConstI64
        // Op[1]: LocalSet(0)     → Mov
        // Op[2]: Jmp(0)          → Jmp (target = MicroOp PC of Op[0])
        let func = make_func(vec![Op::I64Const(0), Op::LocalSet(0), Op::Jmp(0)]);
        let converted = convert(&func);
        // ConstI64 + Mov + Jmp = 3 MicroOps
        assert_eq!(converted.micro_ops.len(), 3);
        assert_eq!(
            converted.micro_ops[2],
            MicroOp::Jmp {
                target: 0,
                old_pc: 2,
                old_target: 0
            }
        );
    }

    #[test]
    fn test_br_if_false_with_comparison() {
        // Op[0]: LocalGet(0)    → Mov t0 <- VReg(0)
        // Op[1]: I64Const(100)  → ConstI64 t1, 100
        // Op[2]: I64LeS         → CmpI64 t2, t0, t1, LeS
        // Op[3]: BrIfFalse(5)   → BrIfFalse { cond: t2, target }
        // Op[4]: I64Const(1)    → ConstI64 t0, 1  (temps reset after branch target flush)
        // Op[5]: Ret            → Ret { src: t0 }  (target of branch)
        let func = make_func(vec![
            Op::LocalGet(0),
            Op::I64Const(100),
            Op::I64LeS,
            Op::BrIfFalse(5),
            Op::I64Const(1),
            Op::Ret,
        ]);
        let converted = convert(&func);

        // Check that CmpI64 was emitted
        assert!(converted.micro_ops.iter().any(|m| matches!(
            m,
            MicroOp::CmpI64 {
                cond: CmpCond::LeS,
                ..
            }
        )));
        // Check BrIfFalse exists
        assert!(
            converted
                .micro_ops
                .iter()
                .any(|m| matches!(m, MicroOp::BrIfFalse { .. }))
        );
    }

    #[test]
    fn test_call_with_vstack() {
        // Op[0]: I64Const(10) → ConstI64 t0, 10
        // Op[1]: I64Const(20) → ConstI64 t1, 20
        // Op[2]: Call(0, 2)   → Call { args: [t0, t1], ret: t2 }
        //                       (vstack flushed before Call, but args come from vstack)
        // Op[3]: Ret          → Ret
        let func = make_func(vec![
            Op::I64Const(10),
            Op::I64Const(20),
            Op::Call(0, 2),
            Op::Ret,
        ]);
        let converted = convert(&func);

        // Find the Call instruction
        let call = converted
            .micro_ops
            .iter()
            .find(|m| matches!(m, MicroOp::Call { .. }));
        assert!(call.is_some());
        if let Some(MicroOp::Call { args, .. }) = call {
            assert_eq!(args.len(), 2);
        }
    }

    #[test]
    fn test_raw_fallback_with_flush() {
        // Op[0]: I64Const(1) → ConstI64 t0, 1
        // Op[1]: I64Const(2) → ConstI64 t1, 2
        // Op[2]: HeapAlloc(2) → StackPush(t0), StackPush(t1), Raw(HeapAlloc(2))
        let func = make_func(vec![Op::I64Const(1), Op::I64Const(2), Op::HeapAlloc(2)]);
        let converted = convert(&func);

        // Should have: ConstI64, ConstI64, StackPush, StackPush, Raw(HeapAlloc)
        assert_eq!(converted.micro_ops.len(), 5);
        assert!(matches!(converted.micro_ops[2], MicroOp::StackPush { .. }));
        assert!(matches!(converted.micro_ops[3], MicroOp::StackPush { .. }));
        assert_eq!(
            converted.micro_ops[4],
            MicroOp::Raw {
                op: Op::HeapAlloc(2)
            }
        );
    }

    #[test]
    fn test_raw_result_pop() {
        // Op[0]: HeapAlloc(0)  → Raw (pushes ref to real stack)
        // Op[1]: LocalSet(0)   → StackPop into temp, Mov to local
        let func = make_func(vec![Op::HeapAlloc(0), Op::LocalSet(0)]);
        let converted = convert(&func);

        // Raw, StackPop, Mov
        assert_eq!(converted.micro_ops.len(), 3);
        assert!(matches!(
            converted.micro_ops[0],
            MicroOp::Raw {
                op: Op::HeapAlloc(0)
            }
        ));
        assert!(matches!(converted.micro_ops[1], MicroOp::StackPop { .. }));
        assert!(matches!(converted.micro_ops[2], MicroOp::Mov { .. }));
    }

    #[test]
    fn test_try_begin_target_remapping() {
        let func = make_func(vec![Op::TryBegin(2), Op::I64Const(0), Op::TryEnd]);
        let converted = convert(&func);
        // TryBegin target should be remapped
        if let MicroOp::Raw {
            op: Op::TryBegin(target),
        } = &converted.micro_ops[0]
        {
            // Op[2] maps to some MicroOp PC
            assert_eq!(*target, converted.pc_map[2]);
        } else {
            panic!("expected Raw TryBegin");
        }
    }

    #[test]
    fn test_drop_consumes_vstack() {
        // I64Const(1) → ConstI64 t0
        // I64Const(2) → ConstI64 t1
        // Drop         → (consumes t1 from vstack, no instruction)
        // LocalSet(0) → Mov VReg(0) <- t0
        let func = make_func(vec![
            Op::I64Const(1),
            Op::I64Const(2),
            Op::Drop,
            Op::LocalSet(0),
        ]);
        let converted = convert(&func);
        // ConstI64 + ConstI64 + Mov (no Drop instruction needed)
        assert_eq!(converted.micro_ops.len(), 3);
    }
}
