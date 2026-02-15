use std::collections::HashSet;

use super::Function;
use super::microop::{CmpCond, ConvertedFunction, MicroOp, VReg};
use super::ops::Op;

/// Virtual stack entry: either a materialized VReg or a deferred i64 immediate.
#[derive(Clone, Copy)]
enum Vse {
    Reg(VReg),
    ImmI64(i64),
}

/// Convert a function's Op bytecode to MicroOp sequence.
///
/// Uses virtual-stack simulation to convert stack-based Ops to register-based
/// MicroOps. I64 constants are deferred on the vstack and absorbed into
/// AddI64Imm / CmpI64Imm when possible.
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
    let mut vstack: Vec<Vse> = Vec::new();
    let mut next_temp = locals_count;
    let mut max_temp = locals_count;

    for (old_pc, op) in code.iter().enumerate() {
        // At branch targets, flush vstack BEFORE recording pc_map.
        // This way branches skip the flush (which is only for the fall-through
        // path). Values from the branch path are already on the real stack.
        if branch_targets.contains(&old_pc) {
            flush_vstack(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
            // Reset temp allocator at basic block boundary
            next_temp = locals_count;
        }

        pc_map.push(micro_ops.len());

        match op {
            // ============================================================
            // Constants
            // ============================================================
            Op::I64Const(n) => {
                // Defer: push immediate onto vstack, materialize only if needed
                vstack.push(Vse::ImmI64(*n));
            }
            Op::I32Const(n) => {
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::ConstI32 { dst, imm: *n });
                vstack.push(Vse::Reg(dst));
            }
            Op::F64Const(f) => {
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::ConstF64 { dst, imm: *f });
                vstack.push(Vse::Reg(dst));
            }
            Op::F32Const(f) => {
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::ConstF32 { dst, imm: *f });
                vstack.push(Vse::Reg(dst));
            }
            Op::RefNull => {
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::RefNull { dst });
                vstack.push(Vse::Reg(dst));
            }

            // ============================================================
            // Locals → direct VReg push / materialize-on-write
            // ============================================================
            Op::LocalGet(slot) => {
                vstack.push(Vse::Reg(VReg(*slot)));
            }
            Op::LocalSet(slot) => {
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = VReg(*slot);
                if src != dst {
                    // Materialize any vstack references to this local before overwriting
                    for entry in vstack.iter_mut() {
                        if let Vse::Reg(v) = entry
                            && *v == dst
                        {
                            let temp = alloc_temp(&mut next_temp, &mut max_temp);
                            micro_ops.push(MicroOp::Mov {
                                dst: temp,
                                src: dst,
                            });
                            *entry = Vse::Reg(temp);
                        }
                    }
                    micro_ops.push(MicroOp::Mov { dst, src });
                }
            }

            // ============================================================
            // Stack manipulation
            // ============================================================
            Op::Drop => {
                let _ = pop_entry(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
            }
            Op::Dup => {
                // pop_entry + push twice: works for both Reg and ImmI64 (Copy)
                let top = pop_entry(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                vstack.push(top);
                vstack.push(top);
            }

            // ============================================================
            // i64 Arithmetic → register-based (with Imm variants)
            // ============================================================
            Op::I64Add => {
                let b = pop_entry(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let a = pop_entry(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                match (a, b) {
                    (_, Vse::ImmI64(imm)) => {
                        let a = mat(a, &mut micro_ops, &mut next_temp, &mut max_temp);
                        micro_ops.push(MicroOp::AddI64Imm { dst, a, imm });
                    }
                    (Vse::ImmI64(imm), _) => {
                        // commutative: a + b = b + a
                        let b = mat(b, &mut micro_ops, &mut next_temp, &mut max_temp);
                        micro_ops.push(MicroOp::AddI64Imm { dst, a: b, imm });
                    }
                    _ => {
                        let a = mat(a, &mut micro_ops, &mut next_temp, &mut max_temp);
                        let b = mat(b, &mut micro_ops, &mut next_temp, &mut max_temp);
                        micro_ops.push(MicroOp::AddI64 { dst, a, b });
                    }
                }
                vstack.push(Vse::Reg(dst));
            }
            Op::I64Sub => {
                let b = pop_entry(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let a = pop_entry(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                // a - imm = a + (-imm)
                if let Vse::ImmI64(imm) = b {
                    let a = mat(a, &mut micro_ops, &mut next_temp, &mut max_temp);
                    micro_ops.push(MicroOp::AddI64Imm {
                        dst,
                        a,
                        imm: imm.wrapping_neg(),
                    });
                } else {
                    let a = mat(a, &mut micro_ops, &mut next_temp, &mut max_temp);
                    let b = mat(b, &mut micro_ops, &mut next_temp, &mut max_temp);
                    micro_ops.push(MicroOp::SubI64 { dst, a, b });
                }
                vstack.push(Vse::Reg(dst));
            }
            Op::I64Mul => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::MulI64 { dst, a, b },
                );
            }
            Op::I64DivS => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::DivI64 { dst, a, b },
                );
            }
            Op::I64RemS => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    |dst, a, b| MicroOp::RemI64 { dst, a, b },
                );
            }
            Op::I64Neg => {
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::NegI64 { dst, src });
                vstack.push(Vse::Reg(dst));
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
                vstack.push(Vse::Reg(dst));
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
                vstack.push(Vse::Reg(dst));
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
                vstack.push(Vse::Reg(dst));
            }

            // ============================================================
            // i64 Comparisons → CmpI64 / CmpI64Imm
            // ============================================================
            // Eq/Ne are polymorphic (values_equal handles Ref/string), so no Imm opt
            Op::I64Eq => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::Eq,
                false,
            ),
            Op::I64Ne => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::Ne,
                false,
            ),
            // LtS/LeS/GtS/GeS are integer-only, safe for Imm opt
            Op::I64LtS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::LtS,
                true,
            ),
            Op::I64LeS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::LeS,
                true,
            ),
            Op::I64GtS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::GtS,
                true,
            ),
            Op::I64GeS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                CmpCond::GeS,
                true,
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
                vstack.push(Vse::Reg(dst));
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
                flush_vstack(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::Jmp {
                    target: *target,
                    old_pc,
                    old_target: *target,
                });
                next_temp = locals_count;
            }
            Op::BrIf(target) => {
                let cond = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                flush_vstack(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::BrIf {
                    cond,
                    target: *target,
                });
            }
            Op::BrIfFalse(target) => {
                let cond = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                flush_vstack(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::BrIfFalse {
                    cond,
                    target: *target,
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
                flush_vstack(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let ret = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::Call {
                    func_id: *func_id,
                    args,
                    ret: Some(ret),
                });
                vstack.push(Vse::Reg(ret));
            }
            Op::Ret => {
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::Ret { src: Some(src) });
                next_temp = locals_count;
            }

            // ============================================================
            // Heap operations → register-based
            // ============================================================
            Op::HeapLoad(offset) => {
                // pop ref, push ref[offset]
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::HeapLoad {
                    dst,
                    src,
                    offset: *offset,
                });
                vstack.push(Vse::Reg(dst));
            }
            Op::HeapLoadDyn => {
                // pop index, pop ref, push ref[index]
                let idx = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let obj = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::HeapLoadDyn { dst, obj, idx });
                vstack.push(Vse::Reg(dst));
            }
            Op::HeapStore(offset) => {
                // pop value, pop ref → ref[offset] = value
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst_obj = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::HeapStore {
                    dst_obj,
                    offset: *offset,
                    src,
                });
            }
            Op::HeapStoreDyn => {
                // pop value, pop index, pop ref → ref[index] = value
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let idx = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let obj = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::HeapStoreDyn { obj, idx, src });
            }
            Op::HeapLoad2 => {
                // pop index, pop ref → push heap[heap[ref][0]][idx]
                let idx = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let obj = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let dst = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::HeapLoad2 { dst, obj, idx });
                vstack.push(Vse::Reg(dst));
            }
            Op::HeapStore2 => {
                // pop value, pop index, pop ref → heap[heap[ref][0]][idx] = value
                let src = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let idx = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let obj = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::HeapStore2 { obj, idx, src });
            }

            // ============================================================
            // Raw with PC target remapping
            // ============================================================
            Op::TryBegin(handler_pc) => {
                flush_vstack(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::Raw {
                    op: Op::TryBegin(*handler_pc),
                });
            }

            // ============================================================
            // Closure operations → register-based
            // ============================================================
            Op::CallIndirect(argc) => {
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
                let callee = pop_vreg(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                flush_vstack(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                let ret = alloc_temp(&mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::CallIndirect {
                    callee,
                    args,
                    ret: Some(ret),
                });
                vstack.push(Vse::Reg(ret));
            }

            // ============================================================
            // Raw fallback (everything else)
            // ============================================================
            _ => {
                flush_vstack(&mut vstack, &mut micro_ops, &mut next_temp, &mut max_temp);
                micro_ops.push(MicroOp::Raw { op: op.clone() });
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

    // ─── Optimization passes ────────────────────────────────────────
    optimize_micro_ops(&mut micro_ops, &mut pc_map);

    ConvertedFunction {
        micro_ops,
        temps_count: temps_count.max(1),
        pc_map,
    }
}

// ─── Helper functions ───────────────────────────────────────────────

fn alloc_temp(next_temp: &mut usize, max_temp: &mut usize) -> VReg {
    let v = VReg(*next_temp);
    *next_temp += 1;
    if *next_temp > *max_temp {
        *max_temp = *next_temp;
    }
    v
}

/// Pop a raw entry from the virtual stack, or emit StackPop.
fn pop_entry(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
) -> Vse {
    if let Some(e) = vstack.pop() {
        e
    } else {
        let t = alloc_temp(next_temp, max_temp);
        micro_ops.push(MicroOp::StackPop { dst: t });
        Vse::Reg(t)
    }
}

/// Materialize a Vse into a VReg, emitting ConstI64 if needed.
fn mat(
    entry: Vse,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
) -> VReg {
    match entry {
        Vse::Reg(v) => v,
        Vse::ImmI64(n) => {
            let t = alloc_temp(next_temp, max_temp);
            micro_ops.push(MicroOp::ConstI64 { dst: t, imm: n });
            t
        }
    }
}

/// Pop from vstack and materialize to VReg.
fn pop_vreg(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
) -> VReg {
    let e = pop_entry(vstack, micro_ops, next_temp, max_temp);
    mat(e, micro_ops, next_temp, max_temp)
}

/// Flush all virtual stack entries to the real operand stack.
fn flush_vstack(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
) {
    for entry in vstack.drain(..) {
        let v = mat(entry, micro_ops, next_temp, max_temp);
        micro_ops.push(MicroOp::StackPush { src: v });
    }
}

/// Emit a binary op: pop 2, push 1 result. Always materializes both operands.
fn emit_binop(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    make_op: impl FnOnce(VReg, VReg, VReg) -> MicroOp,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    micro_ops.push(make_op(dst, a, b));
    vstack.push(Vse::Reg(dst));
}

/// Reverse a comparison condition (swap operands).
fn reverse_cond(cond: CmpCond) -> CmpCond {
    match cond {
        CmpCond::Eq => CmpCond::Eq,
        CmpCond::Ne => CmpCond::Ne,
        CmpCond::LtS => CmpCond::GtS,
        CmpCond::LeS => CmpCond::GeS,
        CmpCond::GtS => CmpCond::LtS,
        CmpCond::GeS => CmpCond::LeS,
    }
}

/// Emit i64 comparison, using CmpI64Imm when one operand is immediate.
fn emit_cmp_i64(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    cond: CmpCond,
    allow_imm: bool,
) {
    let b = pop_entry(vstack, micro_ops, next_temp, max_temp);
    let a = pop_entry(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    match (a, b) {
        (_, Vse::ImmI64(imm)) if allow_imm => {
            let a = mat(a, micro_ops, next_temp, max_temp);
            micro_ops.push(MicroOp::CmpI64Imm { dst, a, imm, cond });
        }
        (Vse::ImmI64(imm), _) if allow_imm => {
            // imm <cond> b → b <reverse_cond> imm
            let b = mat(b, micro_ops, next_temp, max_temp);
            micro_ops.push(MicroOp::CmpI64Imm {
                dst,
                a: b,
                imm,
                cond: reverse_cond(cond),
            });
        }
        _ => {
            let a = mat(a, micro_ops, next_temp, max_temp);
            let b = mat(b, micro_ops, next_temp, max_temp);
            micro_ops.push(MicroOp::CmpI64 { dst, a, b, cond });
        }
    }
    vstack.push(Vse::Reg(dst));
}

fn emit_cmp_i32(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    cond: CmpCond,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    micro_ops.push(MicroOp::CmpI32 { dst, a, b, cond });
    vstack.push(Vse::Reg(dst));
}

fn emit_cmp_f64(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    cond: CmpCond,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    micro_ops.push(MicroOp::CmpF64 { dst, a, b, cond });
    vstack.push(Vse::Reg(dst));
}

fn emit_cmp_f32(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    cond: CmpCond,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    micro_ops.push(MicroOp::CmpF32 { dst, a, b, cond });
    vstack.push(Vse::Reg(dst));
}

// ─── Optimization passes ────────────────────────────────────────────

/// Apply peephole optimizations on the MicroOp sequence.
fn optimize_micro_ops(ops: &mut Vec<MicroOp>, pc_map: &mut [usize]) {
    fuse_arith_mov(ops, pc_map);
    copy_propagation(ops, pc_map);
}

/// After removing instructions (marked by `removed`), fix up all branch targets
/// and update the pc_map (Op PC → MicroOp PC mapping).
fn fixup_branch_targets(ops: &mut [MicroOp], removed: &[bool], pc_map: &mut [usize]) {
    // Build old-to-new PC mapping
    let mut new_pc = vec![0usize; removed.len() + 1];
    let mut offset = 0;
    for i in 0..removed.len() {
        new_pc[i] = i - offset;
        if removed[i] {
            offset += 1;
        }
    }
    new_pc[removed.len()] = removed.len() - offset;

    for op in ops.iter_mut() {
        match op {
            MicroOp::Jmp { target, .. }
            | MicroOp::BrIf { target, .. }
            | MicroOp::BrIfFalse { target, .. } => {
                if *target < new_pc.len() {
                    *target = new_pc[*target];
                }
            }
            MicroOp::Raw {
                op: Op::TryBegin(handler_pc),
            } => {
                if *handler_pc < new_pc.len() {
                    *handler_pc = new_pc[*handler_pc];
                }
            }
            _ => {}
        }
    }

    // Update pc_map: each entry maps an Op PC to a MicroOp PC
    for entry in pc_map.iter_mut() {
        if *entry < new_pc.len() {
            *entry = new_pc[*entry];
        }
    }
}

/// Remove instructions marked for removal, preserving order.
fn remove_marked(ops: &mut Vec<MicroOp>, removed: &[bool]) {
    let mut write = 0;
    ops.retain(|_| {
        let keep = !removed[write];
        write += 1;
        keep
    });
}

/// Optimization #4: Fuse `ArithOp tmp, X, ... ; Mov X, tmp` into `ArithOp X, X, ...`
///
/// When an arithmetic instruction writes to a temp, and the very next instruction
/// is `Mov local, tmp` where `tmp` is not used anywhere else, we can rewrite the
/// arithmetic to target `local` directly and remove the Mov.
fn fuse_arith_mov(ops: &mut Vec<MicroOp>, pc_map: &mut [usize]) {
    let len = ops.len();
    if len < 2 {
        return;
    }

    // Count uses of each vreg (as a source/read operand) across all instructions
    let mut use_count = vec![0u32; vreg_max(ops) + 1];
    for op in ops.iter() {
        for_each_vreg_use(op, |v| use_count[v.0] += 1);
    }

    let mut removed = vec![false; ops.len()];
    let mut i = 0;
    while i + 1 < ops.len() {
        if removed[i] {
            i += 1;
            continue;
        }
        // Match: ops[i] is an op with dst=tmp, ops[i+1] is Mov { dst: local, src: tmp }
        if let MicroOp::Mov {
            dst: mov_dst,
            src: mov_src,
        } = ops[i + 1]
            && let Some(arith_dst) = get_dst(&ops[i])
            // tmp must match, and tmp must only be used once (in the Mov)
            && arith_dst == mov_src
            && use_count[mov_src.0] == 1
        {
            // Rewrite dst to mov_dst
            set_dst(&mut ops[i], mov_dst);
            removed[i + 1] = true;
            i += 2;
            continue;
        }
        i += 1;
    }

    if removed.iter().any(|&r| r) {
        fixup_branch_targets(ops, &removed, pc_map);
        remove_marked(ops, &removed);
    }
}

/// Optimization #1: Copy propagation within basic blocks.
///
/// For `Mov dst, src` within a basic block, replace subsequent uses of `dst` with `src`
/// (until `dst` or `src` is reassigned), then remove the dead Mov.
fn copy_propagation(ops: &mut Vec<MicroOp>, pc_map: &mut [usize]) {
    // Identify branch targets to determine basic block boundaries
    let mut is_block_start = vec![false; ops.len() + 1];
    is_block_start[0] = true;
    for op in ops.iter() {
        match op {
            MicroOp::Jmp { target, .. }
            | MicroOp::BrIf { target, .. }
            | MicroOp::BrIfFalse { target, .. } => {
                if *target < is_block_start.len() {
                    is_block_start[*target] = true;
                }
            }
            _ => {}
        }
    }
    // Instructions after branches also start new blocks
    for i in 0..ops.len() {
        if matches!(
            ops[i],
            MicroOp::Jmp { .. } | MicroOp::BrIf { .. } | MicroOp::BrIfFalse { .. }
        ) && i + 1 < ops.len()
        {
            is_block_start[i + 1] = true;
        }
    }

    let mut to_remove = vec![false; ops.len()];

    let mut i = 0;
    while i < ops.len() {
        if let MicroOp::Mov { dst, src } = ops[i] {
            if dst == src {
                to_remove[i] = true;
                i += 1;
                continue;
            }

            // Try to propagate: replace uses of `dst` with `src` in subsequent
            // instructions within the same basic block
            let mut j = i + 1;
            while j < ops.len() && !is_block_start[j] {
                // If dst or src is redefined, stop propagation
                if let Some(def) = get_dst(&ops[j])
                    && (def == dst || def == src)
                {
                    break;
                }
                // Check for StackPop/Raw which may implicitly define regs
                if matches!(ops[j], MicroOp::StackPop { .. } | MicroOp::Raw { .. }) {
                    break;
                }
                // Replace uses of dst with src
                replace_vreg_use(&mut ops[j], dst, src);
                j += 1;
            }

            // Check if dst is used anywhere outside the propagation range [i+1, j).
            // We must check ALL instructions outside that range, without
            // stopping at redefinitions -- branches can skip over redefinitions
            // causing a use to read the old value from the Mov.
            let mut dst_used_elsewhere = false;
            for (k, op) in ops.iter().enumerate() {
                // Skip the Mov itself and the propagation range
                if k == i || (k > i && k < j) {
                    continue;
                }
                let mut found = false;
                for_each_vreg_use(op, |v| {
                    if v == dst {
                        found = true;
                    }
                });
                if found {
                    dst_used_elsewhere = true;
                    break;
                }
            }

            if !dst_used_elsewhere {
                to_remove[i] = true;
            }
        }
        i += 1;
    }

    if to_remove.iter().any(|&r| r) {
        fixup_branch_targets(ops, &to_remove, pc_map);
        remove_marked(ops, &to_remove);
    }
}

/// Get the maximum VReg index used in any instruction.
fn vreg_max(ops: &[MicroOp]) -> usize {
    let mut max = 0;
    for op in ops {
        for_each_vreg_use(op, |v| max = max.max(v.0));
        if let Some(d) = get_dst(op) {
            max = max.max(d.0);
        }
    }
    max
}

/// Get the destination (def) VReg of an instruction, if any.
fn get_dst(op: &MicroOp) -> Option<VReg> {
    match op {
        MicroOp::Mov { dst, .. }
        | MicroOp::ConstI64 { dst, .. }
        | MicroOp::ConstI32 { dst, .. }
        | MicroOp::ConstF64 { dst, .. }
        | MicroOp::ConstF32 { dst, .. }
        | MicroOp::AddI64 { dst, .. }
        | MicroOp::AddI64Imm { dst, .. }
        | MicroOp::SubI64 { dst, .. }
        | MicroOp::MulI64 { dst, .. }
        | MicroOp::DivI64 { dst, .. }
        | MicroOp::RemI64 { dst, .. }
        | MicroOp::NegI64 { dst, .. }
        | MicroOp::AddI32 { dst, .. }
        | MicroOp::SubI32 { dst, .. }
        | MicroOp::MulI32 { dst, .. }
        | MicroOp::DivI32 { dst, .. }
        | MicroOp::RemI32 { dst, .. }
        | MicroOp::EqzI32 { dst, .. }
        | MicroOp::AddF64 { dst, .. }
        | MicroOp::SubF64 { dst, .. }
        | MicroOp::MulF64 { dst, .. }
        | MicroOp::DivF64 { dst, .. }
        | MicroOp::NegF64 { dst, .. }
        | MicroOp::AddF32 { dst, .. }
        | MicroOp::SubF32 { dst, .. }
        | MicroOp::MulF32 { dst, .. }
        | MicroOp::DivF32 { dst, .. }
        | MicroOp::NegF32 { dst, .. }
        | MicroOp::CmpI64 { dst, .. }
        | MicroOp::CmpI64Imm { dst, .. }
        | MicroOp::CmpI32 { dst, .. }
        | MicroOp::CmpF64 { dst, .. }
        | MicroOp::CmpF32 { dst, .. }
        | MicroOp::I32WrapI64 { dst, .. }
        | MicroOp::I64ExtendI32S { dst, .. }
        | MicroOp::I64ExtendI32U { dst, .. }
        | MicroOp::F64ConvertI64S { dst, .. }
        | MicroOp::I64TruncF64S { dst, .. }
        | MicroOp::F64ConvertI32S { dst, .. }
        | MicroOp::F32ConvertI32S { dst, .. }
        | MicroOp::F32ConvertI64S { dst, .. }
        | MicroOp::I32TruncF32S { dst, .. }
        | MicroOp::I32TruncF64S { dst, .. }
        | MicroOp::I64TruncF32S { dst, .. }
        | MicroOp::F32DemoteF64 { dst, .. }
        | MicroOp::F64PromoteF32 { dst, .. }
        | MicroOp::RefEq { dst, .. }
        | MicroOp::RefIsNull { dst, .. }
        | MicroOp::RefNull { dst }
        | MicroOp::HeapLoad { dst, .. }
        | MicroOp::HeapLoadDyn { dst, .. }
        | MicroOp::HeapLoad2 { dst, .. }
        | MicroOp::StackPop { dst } => Some(*dst),
        MicroOp::Call { ret, .. } | MicroOp::CallIndirect { ret, .. } => *ret,
        _ => None,
    }
}

/// Set the destination VReg of an instruction.
fn set_dst(op: &mut MicroOp, new_dst: VReg) {
    match op {
        MicroOp::Mov { dst, .. }
        | MicroOp::ConstI64 { dst, .. }
        | MicroOp::ConstI32 { dst, .. }
        | MicroOp::ConstF64 { dst, .. }
        | MicroOp::ConstF32 { dst, .. }
        | MicroOp::AddI64 { dst, .. }
        | MicroOp::AddI64Imm { dst, .. }
        | MicroOp::SubI64 { dst, .. }
        | MicroOp::MulI64 { dst, .. }
        | MicroOp::DivI64 { dst, .. }
        | MicroOp::RemI64 { dst, .. }
        | MicroOp::NegI64 { dst, .. }
        | MicroOp::AddI32 { dst, .. }
        | MicroOp::SubI32 { dst, .. }
        | MicroOp::MulI32 { dst, .. }
        | MicroOp::DivI32 { dst, .. }
        | MicroOp::RemI32 { dst, .. }
        | MicroOp::EqzI32 { dst, .. }
        | MicroOp::AddF64 { dst, .. }
        | MicroOp::SubF64 { dst, .. }
        | MicroOp::MulF64 { dst, .. }
        | MicroOp::DivF64 { dst, .. }
        | MicroOp::NegF64 { dst, .. }
        | MicroOp::AddF32 { dst, .. }
        | MicroOp::SubF32 { dst, .. }
        | MicroOp::MulF32 { dst, .. }
        | MicroOp::DivF32 { dst, .. }
        | MicroOp::NegF32 { dst, .. }
        | MicroOp::CmpI64 { dst, .. }
        | MicroOp::CmpI64Imm { dst, .. }
        | MicroOp::CmpI32 { dst, .. }
        | MicroOp::CmpF64 { dst, .. }
        | MicroOp::CmpF32 { dst, .. }
        | MicroOp::I32WrapI64 { dst, .. }
        | MicroOp::I64ExtendI32S { dst, .. }
        | MicroOp::I64ExtendI32U { dst, .. }
        | MicroOp::F64ConvertI64S { dst, .. }
        | MicroOp::I64TruncF64S { dst, .. }
        | MicroOp::F64ConvertI32S { dst, .. }
        | MicroOp::F32ConvertI32S { dst, .. }
        | MicroOp::F32ConvertI64S { dst, .. }
        | MicroOp::I32TruncF32S { dst, .. }
        | MicroOp::I32TruncF64S { dst, .. }
        | MicroOp::I64TruncF32S { dst, .. }
        | MicroOp::F32DemoteF64 { dst, .. }
        | MicroOp::F64PromoteF32 { dst, .. }
        | MicroOp::RefEq { dst, .. }
        | MicroOp::RefIsNull { dst, .. }
        | MicroOp::RefNull { dst }
        | MicroOp::HeapLoad { dst, .. }
        | MicroOp::HeapLoadDyn { dst, .. }
        | MicroOp::HeapLoad2 { dst, .. }
        | MicroOp::StackPop { dst } => *dst = new_dst,
        MicroOp::Call { ret, .. } | MicroOp::CallIndirect { ret, .. } => *ret = Some(new_dst),
        _ => {}
    }
}

/// Iterate over all VRegs used (read) by an instruction.
fn for_each_vreg_use(op: &MicroOp, mut f: impl FnMut(VReg)) {
    match op {
        MicroOp::Jmp { .. } => {}
        MicroOp::BrIf { cond, .. } | MicroOp::BrIfFalse { cond, .. } => f(*cond),
        MicroOp::Call { args, .. } => {
            for a in args {
                f(*a);
            }
        }
        MicroOp::CallIndirect { callee, args, .. } => {
            f(*callee);
            for a in args {
                f(*a);
            }
        }
        MicroOp::Ret { src } => {
            if let Some(s) = src {
                f(*s);
            }
        }
        MicroOp::Mov { src, .. } => f(*src),
        MicroOp::ConstI64 { .. }
        | MicroOp::ConstI32 { .. }
        | MicroOp::ConstF64 { .. }
        | MicroOp::ConstF32 { .. }
        | MicroOp::RefNull { .. } => {}
        MicroOp::AddI64 { a, b, .. }
        | MicroOp::SubI64 { a, b, .. }
        | MicroOp::MulI64 { a, b, .. }
        | MicroOp::DivI64 { a, b, .. }
        | MicroOp::RemI64 { a, b, .. } => {
            f(*a);
            f(*b);
        }
        MicroOp::AddI64Imm { a, .. } | MicroOp::CmpI64Imm { a, .. } => f(*a),
        MicroOp::NegI64 { src, .. } => f(*src),
        MicroOp::AddI32 { a, b, .. }
        | MicroOp::SubI32 { a, b, .. }
        | MicroOp::MulI32 { a, b, .. }
        | MicroOp::DivI32 { a, b, .. }
        | MicroOp::RemI32 { a, b, .. } => {
            f(*a);
            f(*b);
        }
        MicroOp::EqzI32 { src, .. } => f(*src),
        MicroOp::AddF64 { a, b, .. }
        | MicroOp::SubF64 { a, b, .. }
        | MicroOp::MulF64 { a, b, .. }
        | MicroOp::DivF64 { a, b, .. } => {
            f(*a);
            f(*b);
        }
        MicroOp::NegF64 { src, .. } => f(*src),
        MicroOp::AddF32 { a, b, .. }
        | MicroOp::SubF32 { a, b, .. }
        | MicroOp::MulF32 { a, b, .. }
        | MicroOp::DivF32 { a, b, .. } => {
            f(*a);
            f(*b);
        }
        MicroOp::NegF32 { src, .. } => f(*src),
        MicroOp::CmpI64 { a, b, .. }
        | MicroOp::CmpI32 { a, b, .. }
        | MicroOp::CmpF64 { a, b, .. }
        | MicroOp::CmpF32 { a, b, .. } => {
            f(*a);
            f(*b);
        }
        MicroOp::RefEq { a, b, .. } => {
            f(*a);
            f(*b);
        }
        MicroOp::I32WrapI64 { src, .. }
        | MicroOp::I64ExtendI32S { src, .. }
        | MicroOp::I64ExtendI32U { src, .. }
        | MicroOp::F64ConvertI64S { src, .. }
        | MicroOp::I64TruncF64S { src, .. }
        | MicroOp::F64ConvertI32S { src, .. }
        | MicroOp::F32ConvertI32S { src, .. }
        | MicroOp::F32ConvertI64S { src, .. }
        | MicroOp::I32TruncF32S { src, .. }
        | MicroOp::I32TruncF64S { src, .. }
        | MicroOp::I64TruncF32S { src, .. }
        | MicroOp::F32DemoteF64 { src, .. }
        | MicroOp::F64PromoteF32 { src, .. }
        | MicroOp::RefIsNull { src, .. } => f(*src),
        MicroOp::HeapLoad { src, .. } => f(*src),
        MicroOp::HeapLoadDyn { obj, idx, .. } | MicroOp::HeapLoad2 { obj, idx, .. } => {
            f(*obj);
            f(*idx);
        }
        MicroOp::HeapStore { dst_obj, src, .. } => {
            f(*dst_obj);
            f(*src);
        }
        MicroOp::HeapStoreDyn { obj, idx, src } | MicroOp::HeapStore2 { obj, idx, src } => {
            f(*obj);
            f(*idx);
            f(*src);
        }
        MicroOp::StackPush { src } => f(*src),
        MicroOp::StackPop { .. } => {}
        MicroOp::Raw { .. } => {}
    }
}

/// Replace uses of `old` vreg with `new` vreg in a single instruction.
fn replace_vreg_use(op: &mut MicroOp, old: VReg, new: VReg) {
    let r = |v: &mut VReg| {
        if *v == old {
            *v = new;
        }
    };
    match op {
        MicroOp::Jmp { .. } => {}
        MicroOp::BrIf { cond, .. } | MicroOp::BrIfFalse { cond, .. } => r(cond),
        MicroOp::Call { args, .. } => {
            for a in args {
                r(a);
            }
        }
        MicroOp::CallIndirect { callee, args, .. } => {
            r(callee);
            for a in args {
                r(a);
            }
        }
        MicroOp::Ret { src } => {
            if let Some(s) = src {
                r(s);
            }
        }
        MicroOp::Mov { src, .. } => r(src),
        MicroOp::ConstI64 { .. }
        | MicroOp::ConstI32 { .. }
        | MicroOp::ConstF64 { .. }
        | MicroOp::ConstF32 { .. }
        | MicroOp::RefNull { .. } => {}
        MicroOp::AddI64 { a, b, .. }
        | MicroOp::SubI64 { a, b, .. }
        | MicroOp::MulI64 { a, b, .. }
        | MicroOp::DivI64 { a, b, .. }
        | MicroOp::RemI64 { a, b, .. } => {
            r(a);
            r(b);
        }
        MicroOp::AddI64Imm { a, .. } | MicroOp::CmpI64Imm { a, .. } => r(a),
        MicroOp::NegI64 { src, .. } => r(src),
        MicroOp::AddI32 { a, b, .. }
        | MicroOp::SubI32 { a, b, .. }
        | MicroOp::MulI32 { a, b, .. }
        | MicroOp::DivI32 { a, b, .. }
        | MicroOp::RemI32 { a, b, .. } => {
            r(a);
            r(b);
        }
        MicroOp::EqzI32 { src, .. } => r(src),
        MicroOp::AddF64 { a, b, .. }
        | MicroOp::SubF64 { a, b, .. }
        | MicroOp::MulF64 { a, b, .. }
        | MicroOp::DivF64 { a, b, .. } => {
            r(a);
            r(b);
        }
        MicroOp::NegF64 { src, .. } => r(src),
        MicroOp::AddF32 { a, b, .. }
        | MicroOp::SubF32 { a, b, .. }
        | MicroOp::MulF32 { a, b, .. }
        | MicroOp::DivF32 { a, b, .. } => {
            r(a);
            r(b);
        }
        MicroOp::NegF32 { src, .. } => r(src),
        MicroOp::CmpI64 { a, b, .. }
        | MicroOp::CmpI32 { a, b, .. }
        | MicroOp::CmpF64 { a, b, .. }
        | MicroOp::CmpF32 { a, b, .. } => {
            r(a);
            r(b);
        }
        MicroOp::RefEq { a, b, .. } => {
            r(a);
            r(b);
        }
        MicroOp::I32WrapI64 { src, .. }
        | MicroOp::I64ExtendI32S { src, .. }
        | MicroOp::I64ExtendI32U { src, .. }
        | MicroOp::F64ConvertI64S { src, .. }
        | MicroOp::I64TruncF64S { src, .. }
        | MicroOp::F64ConvertI32S { src, .. }
        | MicroOp::F32ConvertI32S { src, .. }
        | MicroOp::F32ConvertI64S { src, .. }
        | MicroOp::I32TruncF32S { src, .. }
        | MicroOp::I32TruncF64S { src, .. }
        | MicroOp::I64TruncF32S { src, .. }
        | MicroOp::F32DemoteF64 { src, .. }
        | MicroOp::F64PromoteF32 { src, .. }
        | MicroOp::RefIsNull { src, .. } => r(src),
        MicroOp::HeapLoad { src, .. } => r(src),
        MicroOp::HeapLoadDyn { obj, idx, .. } | MicroOp::HeapLoad2 { obj, idx, .. } => {
            r(obj);
            r(idx);
        }
        MicroOp::HeapStore { dst_obj, src, .. } => {
            r(dst_obj);
            r(src);
        }
        MicroOp::HeapStoreDyn { obj, idx, src } | MicroOp::HeapStore2 { obj, idx, src } => {
            r(obj);
            r(idx);
            r(src);
        }
        MicroOp::StackPush { src } => r(src),
        MicroOp::StackPop { .. } => {}
        MicroOp::Raw { .. } => {}
    }
}

/// Emit a unary type conversion: pop 1, push 1.
fn emit_unary_conv(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    make_op: impl FnOnce(VReg, VReg) -> MicroOp,
) {
    let src = pop_vreg(vstack, micro_ops, next_temp, max_temp);
    let dst = alloc_temp(next_temp, max_temp);
    micro_ops.push(make_op(dst, src));
    vstack.push(Vse::Reg(dst));
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
        // I64Const(42) is deferred, then materialized by LocalSet
        // Optimization fuses ConstI64 tmp + Mov local, tmp → ConstI64 local
        let func = make_func(vec![Op::I64Const(42), Op::LocalSet(0)]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 1);
        assert_eq!(
            converted.micro_ops[0],
            MicroOp::ConstI64 {
                dst: VReg(0),
                imm: 42
            }
        );
    }

    #[test]
    fn test_local_get_and_add() {
        // LocalGet(0) + LocalGet(1) + I64Add → AddI64 directly
        // Optimization fuses AddI64 tmp + Mov local, tmp → AddI64 local
        let func = make_func(vec![
            Op::LocalGet(0),
            Op::LocalGet(1),
            Op::I64Add,
            Op::LocalSet(0),
        ]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 1);
        assert_eq!(
            converted.micro_ops[0],
            MicroOp::AddI64 {
                dst: VReg(0),
                a: VReg(0),
                b: VReg(1)
            }
        );
    }

    #[test]
    fn test_add_imm() {
        // LocalGet(0) + I64Const(1) + I64Add → AddI64Imm
        let func = make_func(vec![Op::LocalGet(0), Op::I64Const(1), Op::I64Add]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 1);
        assert_eq!(
            converted.micro_ops[0],
            MicroOp::AddI64Imm {
                dst: VReg(2),
                a: VReg(0),
                imm: 1
            }
        );
    }

    #[test]
    fn test_cmp_imm() {
        // LocalGet(1) + I64Const(100) + I64LeS → CmpI64Imm
        let func = make_func(vec![Op::LocalGet(1), Op::I64Const(100), Op::I64LeS]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 1);
        assert_eq!(
            converted.micro_ops[0],
            MicroOp::CmpI64Imm {
                dst: VReg(2),
                a: VReg(1),
                imm: 100,
                cond: CmpCond::LeS
            }
        );
    }

    #[test]
    fn test_jmp_target_remapping() {
        let func = make_func(vec![Op::I64Const(0), Op::LocalSet(0), Op::Jmp(0)]);
        let converted = convert(&func);
        // Optimization fuses ConstI64 + Mov → ConstI64, so ConstI64 + Jmp = 2 MicroOps
        assert_eq!(converted.micro_ops.len(), 2);
        assert_eq!(
            converted.micro_ops[1],
            MicroOp::Jmp {
                target: 0,
                old_pc: 2,
                old_target: 0
            }
        );
    }

    #[test]
    fn test_br_if_false_with_comparison() {
        let func = make_func(vec![
            Op::LocalGet(0),
            Op::I64Const(100),
            Op::I64LeS,
            Op::BrIfFalse(5),
            Op::I64Const(1),
            Op::Ret,
        ]);
        let converted = convert(&func);
        // Should have CmpI64Imm (absorbed const) and BrIfFalse
        assert!(converted.micro_ops.iter().any(|m| matches!(
            m,
            MicroOp::CmpI64Imm {
                cond: CmpCond::LeS,
                ..
            }
        )));
        assert!(
            converted
                .micro_ops
                .iter()
                .any(|m| matches!(m, MicroOp::BrIfFalse { .. }))
        );
    }

    #[test]
    fn test_call_with_vstack() {
        let func = make_func(vec![
            Op::I64Const(10),
            Op::I64Const(20),
            Op::Call(0, 2),
            Op::Ret,
        ]);
        let converted = convert(&func);
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
        // I64Const values are deferred, then materialized+flushed before Raw
        let func = make_func(vec![Op::I64Const(1), Op::I64Const(2), Op::HeapAlloc(2)]);
        let converted = convert(&func);
        // ConstI64, StackPush, ConstI64, StackPush, Raw(HeapAlloc)
        assert_eq!(converted.micro_ops.len(), 5);
        assert!(matches!(
            converted.micro_ops[4],
            MicroOp::Raw {
                op: Op::HeapAlloc(2)
            }
        ));
    }

    #[test]
    fn test_raw_result_pop() {
        let func = make_func(vec![Op::HeapAlloc(0), Op::LocalSet(0)]);
        let converted = convert(&func);
        // Optimization fuses StackPop tmp + Mov local, tmp → StackPop local
        // Raw, StackPop = 2 MicroOps
        assert_eq!(converted.micro_ops.len(), 2);
        assert!(matches!(
            converted.micro_ops[0],
            MicroOp::Raw {
                op: Op::HeapAlloc(0)
            }
        ));
        assert!(matches!(
            converted.micro_ops[1],
            MicroOp::StackPop { dst: VReg(0) }
        ));
    }

    #[test]
    fn test_try_begin_target_remapping() {
        let func = make_func(vec![Op::TryBegin(2), Op::I64Const(0), Op::TryEnd]);
        let converted = convert(&func);
        if let MicroOp::Raw {
            op: Op::TryBegin(target),
        } = &converted.micro_ops[0]
        {
            assert_eq!(*target, converted.pc_map[2]);
        } else {
            panic!("expected Raw TryBegin");
        }
    }

    #[test]
    fn test_drop_consumes_vstack() {
        let func = make_func(vec![
            Op::I64Const(1),
            Op::I64Const(2),
            Op::Drop,
            Op::LocalSet(0),
        ]);
        let converted = convert(&func);
        // I64Const(1) deferred, I64Const(2) deferred, Drop consumes 2,
        // LocalSet materializes 1 → ConstI64 + Mov, optimized to ConstI64 = 1
        assert_eq!(converted.micro_ops.len(), 1);
    }
}
