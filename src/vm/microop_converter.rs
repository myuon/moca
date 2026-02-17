use std::collections::HashSet;

use super::microop::{CmpCond, ConvertedFunction, MicroOp, VReg};
use super::ops::Op;
use super::{Function, ValueType};

/// Virtual stack entry: either a materialized VReg or a deferred i64 immediate.
#[derive(Clone, Copy)]
enum Vse {
    Reg(VReg),
    ImmI64(i64),
    /// Float VReg (produced by F64Const, F64 operations, etc.)
    RegF64(VReg),
    /// Deferred float immediate (not yet materialized into a VReg)
    ImmF64(f64),
    /// Ref VReg (produced by StringConst, ToString, etc.)
    RegRef(VReg),
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

    // vreg_types: starts with local types, extended as temps are allocated.
    // Locals come from func.local_types; pad with I64 if local_types is shorter.
    let mut vreg_types: Vec<ValueType> = Vec::with_capacity(locals_count + 16);
    for i in 0..locals_count {
        vreg_types.push(func.local_types.get(i).copied().unwrap_or(ValueType::I64));
    }

    for (old_pc, op) in code.iter().enumerate() {
        // At branch targets, flush vstack BEFORE recording pc_map.
        // This way branches skip the flush (which is only for the fall-through
        // path). Values from the branch path are already on the real stack.
        if branch_targets.contains(&old_pc) {
            flush_vstack(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
            );
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
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I32,
                );
                micro_ops.push(MicroOp::ConstI32 { dst, imm: *n });
                vstack.push(Vse::Reg(dst));
            }
            Op::F64Const(f) => {
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::F64,
                );
                micro_ops.push(MicroOp::ConstF64 { dst, imm: *f });
                vstack.push(Vse::RegF64(dst));
            }
            Op::F32Const(f) => {
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::F32,
                );
                micro_ops.push(MicroOp::ConstF32 { dst, imm: *f });
                vstack.push(Vse::RegF64(dst));
            }
            Op::RefNull => {
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::Ref,
                );
                micro_ops.push(MicroOp::RefNull { dst });
                vstack.push(Vse::Reg(dst));
            }

            // ============================================================
            // Locals → direct VReg push / materialize-on-write
            // ============================================================
            Op::LocalGet(slot) => {
                let vreg = VReg(*slot);
                let vse = match func.local_types.get(*slot).copied() {
                    Some(ValueType::F64) => Vse::RegF64(vreg),
                    Some(ValueType::Ref) => Vse::RegRef(vreg),
                    _ => Vse::Reg(vreg),
                };
                vstack.push(vse);
            }
            Op::LocalSet(slot) => {
                let dst_local = VReg(*slot);

                // === Dst forwarding optimization ===
                // If vstack top is a temp produced by the last micro_op,
                // patch that op's dst to write directly to the target local.
                if let Some(Vse::Reg(temp) | Vse::RegF64(temp) | Vse::RegRef(temp)) =
                    vstack.last().copied()
                    && temp.0 >= locals_count
                    && !micro_ops.is_empty()
                    && !vstack[..vstack.len() - 1].iter().any(|e| {
                        matches!(e, Vse::Reg(v) | Vse::RegF64(v) | Vse::RegRef(v) if *v == temp)
                    })
                    && !vstack[..vstack.len() - 1].iter().any(|e| {
                        matches!(e, Vse::Reg(v) | Vse::RegF64(v) | Vse::RegRef(v) if *v == dst_local)
                    })
                {
                    let last_idx = micro_ops.len() - 1;
                    if let Some(old_dst) = try_patch_dst(&mut micro_ops[last_idx], dst_local) {
                        if old_dst == temp {
                            vstack.pop();
                            continue;
                        } else {
                            // Unexpected: revert patch
                            try_patch_dst(&mut micro_ops[last_idx], old_dst);
                        }
                    }
                }

                // === Normal path (patch not applicable) ===
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                if src != dst_local {
                    // Materialize any vstack references to this local before overwriting
                    for entry in vstack.iter_mut() {
                        if let Vse::Reg(v) = entry
                            && *v == dst_local
                        {
                            let temp = alloc_temp(
                                &mut next_temp,
                                &mut max_temp,
                                &mut vreg_types,
                                ValueType::I64,
                            );
                            micro_ops.push(MicroOp::Mov {
                                dst: temp,
                                src: dst_local,
                            });
                            *entry = Vse::Reg(temp);
                        }
                    }
                    micro_ops.push(MicroOp::Mov {
                        dst: dst_local,
                        src,
                    });
                }
            }

            // ============================================================
            // Stack manipulation
            // ============================================================
            Op::Drop => {
                let _ = pop_entry(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
            }
            Op::Dup => {
                // pop_entry + push twice: works for both Reg and ImmI64 (Copy)
                let top = pop_entry(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                vstack.push(top);
                vstack.push(top);
            }

            // ============================================================
            // i64 Arithmetic → register-based (with Imm variants)
            // ============================================================
            Op::I64Add => {
                let b = pop_entry(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let a = pop_entry(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                // Promote to float if either operand is float
                if is_float(&a) || is_float(&b) {
                    let dst = alloc_temp(
                        &mut next_temp,
                        &mut max_temp,
                        &mut vreg_types,
                        ValueType::F64,
                    );
                    let a = mat(
                        a,
                        &mut micro_ops,
                        &mut next_temp,
                        &mut max_temp,
                        &mut vreg_types,
                    );
                    let b = mat(
                        b,
                        &mut micro_ops,
                        &mut next_temp,
                        &mut max_temp,
                        &mut vreg_types,
                    );
                    micro_ops.push(MicroOp::AddF64 { dst, a, b });
                    vstack.push(Vse::RegF64(dst));
                } else {
                    let dst = alloc_temp(
                        &mut next_temp,
                        &mut max_temp,
                        &mut vreg_types,
                        ValueType::I64,
                    );
                    match (a, b) {
                        (_, Vse::ImmI64(imm)) => {
                            let a = mat(
                                a,
                                &mut micro_ops,
                                &mut next_temp,
                                &mut max_temp,
                                &mut vreg_types,
                            );
                            micro_ops.push(MicroOp::AddI64Imm { dst, a, imm });
                        }
                        (Vse::ImmI64(imm), _) => {
                            // commutative: a + b = b + a
                            let b = mat(
                                b,
                                &mut micro_ops,
                                &mut next_temp,
                                &mut max_temp,
                                &mut vreg_types,
                            );
                            micro_ops.push(MicroOp::AddI64Imm { dst, a: b, imm });
                        }
                        _ => {
                            let a = mat(
                                a,
                                &mut micro_ops,
                                &mut next_temp,
                                &mut max_temp,
                                &mut vreg_types,
                            );
                            let b = mat(
                                b,
                                &mut micro_ops,
                                &mut next_temp,
                                &mut max_temp,
                                &mut vreg_types,
                            );
                            micro_ops.push(MicroOp::AddI64 { dst, a, b });
                        }
                    }
                    vstack.push(Vse::Reg(dst));
                }
            }
            Op::I64Sub => {
                let b = pop_entry(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let a = pop_entry(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                if is_float(&a) || is_float(&b) {
                    let dst = alloc_temp(
                        &mut next_temp,
                        &mut max_temp,
                        &mut vreg_types,
                        ValueType::F64,
                    );
                    let a = mat(
                        a,
                        &mut micro_ops,
                        &mut next_temp,
                        &mut max_temp,
                        &mut vreg_types,
                    );
                    let b = mat(
                        b,
                        &mut micro_ops,
                        &mut next_temp,
                        &mut max_temp,
                        &mut vreg_types,
                    );
                    micro_ops.push(MicroOp::SubF64 { dst, a, b });
                    vstack.push(Vse::RegF64(dst));
                } else {
                    let dst = alloc_temp(
                        &mut next_temp,
                        &mut max_temp,
                        &mut vreg_types,
                        ValueType::I64,
                    );
                    // a - imm = a + (-imm)
                    if let Vse::ImmI64(imm) = b {
                        let a = mat(
                            a,
                            &mut micro_ops,
                            &mut next_temp,
                            &mut max_temp,
                            &mut vreg_types,
                        );
                        micro_ops.push(MicroOp::AddI64Imm {
                            dst,
                            a,
                            imm: imm.wrapping_neg(),
                        });
                    } else {
                        let a = mat(
                            a,
                            &mut micro_ops,
                            &mut next_temp,
                            &mut max_temp,
                            &mut vreg_types,
                        );
                        let b = mat(
                            b,
                            &mut micro_ops,
                            &mut next_temp,
                            &mut max_temp,
                            &mut vreg_types,
                        );
                        micro_ops.push(MicroOp::SubI64 { dst, a, b });
                    }
                    vstack.push(Vse::Reg(dst));
                }
            }
            Op::I64Mul => {
                let b = pop_entry(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let a = pop_entry(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let float = is_float(&a) || is_float(&b);
                let ty = if float {
                    ValueType::F64
                } else {
                    ValueType::I64
                };
                let dst = alloc_temp(&mut next_temp, &mut max_temp, &mut vreg_types, ty);
                let a = mat(
                    a,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let b = mat(
                    b,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                if float {
                    micro_ops.push(MicroOp::MulF64 { dst, a, b });
                    vstack.push(Vse::RegF64(dst));
                } else {
                    micro_ops.push(MicroOp::MulI64 { dst, a, b });
                    vstack.push(Vse::Reg(dst));
                }
            }
            Op::I64DivS => {
                let b = pop_entry(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let a = pop_entry(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let float = is_float(&a) || is_float(&b);
                let ty = if float {
                    ValueType::F64
                } else {
                    ValueType::I64
                };
                let dst = alloc_temp(&mut next_temp, &mut max_temp, &mut vreg_types, ty);
                let a = mat(
                    a,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let b = mat(
                    b,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                if float {
                    micro_ops.push(MicroOp::DivF64 { dst, a, b });
                    vstack.push(Vse::RegF64(dst));
                } else {
                    micro_ops.push(MicroOp::DivI64 { dst, a, b });
                    vstack.push(Vse::Reg(dst));
                }
            }
            Op::I64RemS => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I64,
                    |dst, a, b| MicroOp::RemI64 { dst, a, b },
                );
            }
            Op::I64Neg => {
                let entry = pop_entry(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let float = is_float(&entry);
                let src = mat(
                    entry,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let ty = if float {
                    ValueType::F64
                } else {
                    ValueType::I64
                };
                let dst = alloc_temp(&mut next_temp, &mut max_temp, &mut vreg_types, ty);
                if float {
                    micro_ops.push(MicroOp::NegF64 { dst, src });
                    vstack.push(Vse::RegF64(dst));
                } else {
                    micro_ops.push(MicroOp::NegI64 { dst, src });
                    vstack.push(Vse::Reg(dst));
                }
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
                    &mut vreg_types,
                    ValueType::I32,
                    |dst, a, b| MicroOp::AddI32 { dst, a, b },
                );
            }
            Op::I32Sub => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I32,
                    |dst, a, b| MicroOp::SubI32 { dst, a, b },
                );
            }
            Op::I32Mul => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I32,
                    |dst, a, b| MicroOp::MulI32 { dst, a, b },
                );
            }
            Op::I32DivS => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I32,
                    |dst, a, b| MicroOp::DivI32 { dst, a, b },
                );
            }
            Op::I32RemS => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I32,
                    |dst, a, b| MicroOp::RemI32 { dst, a, b },
                );
            }
            Op::I32Eqz => {
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I32,
                );
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
                    &mut vreg_types,
                    ValueType::F64,
                    |dst, a, b| MicroOp::AddF64 { dst, a, b },
                );
            }
            Op::F64Sub => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::F64,
                    |dst, a, b| MicroOp::SubF64 { dst, a, b },
                );
            }
            Op::F64Mul => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::F64,
                    |dst, a, b| MicroOp::MulF64 { dst, a, b },
                );
            }
            Op::F64Div => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::F64,
                    |dst, a, b| MicroOp::DivF64 { dst, a, b },
                );
            }
            Op::F64Neg => {
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::F64,
                );
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
                    &mut vreg_types,
                    ValueType::F32,
                    |dst, a, b| MicroOp::AddF32 { dst, a, b },
                );
            }
            Op::F32Sub => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::F32,
                    |dst, a, b| MicroOp::SubF32 { dst, a, b },
                );
            }
            Op::F32Mul => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::F32,
                    |dst, a, b| MicroOp::MulF32 { dst, a, b },
                );
            }
            Op::F32Div => {
                emit_binop(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::F32,
                    |dst, a, b| MicroOp::DivF32 { dst, a, b },
                );
            }
            Op::F32Neg => {
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::F32,
                );
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
                &mut vreg_types,
                CmpCond::Eq,
                false,
            ),
            Op::I64Ne => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::Ne,
                false,
            ),
            // LtS/LeS/GtS/GeS are integer-only, safe for Imm opt
            Op::I64LtS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::LtS,
                true,
            ),
            Op::I64LeS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::LeS,
                true,
            ),
            Op::I64GtS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::GtS,
                true,
            ),
            Op::I64GeS => emit_cmp_i64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
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
                &mut vreg_types,
                CmpCond::Eq,
            ),
            Op::I32Ne => emit_cmp_i32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::Ne,
            ),
            Op::I32LtS => emit_cmp_i32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::LtS,
            ),
            Op::I32LeS => emit_cmp_i32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::LeS,
            ),
            Op::I32GtS => emit_cmp_i32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::GtS,
            ),
            Op::I32GeS => emit_cmp_i32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
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
                &mut vreg_types,
                CmpCond::Eq,
            ),
            Op::F64Ne => emit_cmp_f64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::Ne,
            ),
            Op::F64Lt => emit_cmp_f64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::LtS,
            ),
            Op::F64Le => emit_cmp_f64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::LeS,
            ),
            Op::F64Gt => emit_cmp_f64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::GtS,
            ),
            Op::F64Ge => emit_cmp_f64(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
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
                &mut vreg_types,
                CmpCond::Eq,
            ),
            Op::F32Ne => emit_cmp_f32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::Ne,
            ),
            Op::F32Lt => emit_cmp_f32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::LtS,
            ),
            Op::F32Le => emit_cmp_f32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::LeS,
            ),
            Op::F32Gt => emit_cmp_f32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                CmpCond::GtS,
            ),
            Op::F32Ge => emit_cmp_f32(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
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
                    &mut vreg_types,
                    ValueType::I64,
                    |dst, a, b| MicroOp::RefEq { dst, a, b },
                );
            }
            Op::RefIsNull => {
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I64,
                );
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
                &mut vreg_types,
                ValueType::I32,
                |dst, src| MicroOp::I32WrapI64 { dst, src },
            ),
            Op::I64ExtendI32S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::I64,
                |dst, src| MicroOp::I64ExtendI32S { dst, src },
            ),
            Op::I64ExtendI32U => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::I64,
                |dst, src| MicroOp::I64ExtendI32U { dst, src },
            ),
            Op::F64ConvertI64S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::F64,
                |dst, src| MicroOp::F64ConvertI64S { dst, src },
            ),
            Op::I64TruncF64S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::I64,
                |dst, src| MicroOp::I64TruncF64S { dst, src },
            ),
            Op::F64ConvertI32S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::F64,
                |dst, src| MicroOp::F64ConvertI32S { dst, src },
            ),
            Op::F32ConvertI32S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::F32,
                |dst, src| MicroOp::F32ConvertI32S { dst, src },
            ),
            Op::F32ConvertI64S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::F32,
                |dst, src| MicroOp::F32ConvertI64S { dst, src },
            ),
            Op::I32TruncF32S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::I32,
                |dst, src| MicroOp::I32TruncF32S { dst, src },
            ),
            Op::I32TruncF64S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::I32,
                |dst, src| MicroOp::I32TruncF64S { dst, src },
            ),
            Op::I64TruncF32S => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::I64,
                |dst, src| MicroOp::I64TruncF32S { dst, src },
            ),
            Op::F32DemoteF64 => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::F32,
                |dst, src| MicroOp::F32DemoteF64 { dst, src },
            ),
            Op::F64PromoteF32 => emit_unary_conv(
                &mut vstack,
                &mut micro_ops,
                &mut next_temp,
                &mut max_temp,
                &mut vreg_types,
                ValueType::F64,
                |dst, src| MicroOp::F64PromoteF32 { dst, src },
            ),

            // ============================================================
            // Control Flow → native MicroOps
            // ============================================================
            Op::Jmp(target) => {
                flush_vstack(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                micro_ops.push(MicroOp::Jmp {
                    target: *target,
                    old_pc,
                    old_target: *target,
                });
                next_temp = locals_count;
            }
            Op::BrIf(target) => {
                let cond = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                flush_vstack(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                micro_ops.push(MicroOp::BrIf {
                    cond,
                    target: *target,
                });
            }
            Op::BrIfFalse(target) => {
                let cond = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                flush_vstack(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
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
                        &mut vreg_types,
                    ));
                }
                args.reverse();
                flush_vstack(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let ret = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I64,
                );
                micro_ops.push(MicroOp::Call {
                    func_id: *func_id,
                    args,
                    ret: Some(ret),
                });
                vstack.push(Vse::Reg(ret));
            }
            Op::Ret => {
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                micro_ops.push(MicroOp::Ret { src: Some(src) });
                next_temp = locals_count;
            }

            // ============================================================
            // Heap operations → register-based
            // ============================================================
            Op::HeapLoad(offset) => {
                // pop ref, push ref[offset]
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I64,
                );
                micro_ops.push(MicroOp::HeapLoad {
                    dst,
                    src,
                    offset: *offset,
                });
                vstack.push(Vse::Reg(dst));
            }
            Op::HeapLoadDyn => {
                // pop index, pop ref, push ref[index]
                let idx = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let obj = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I64,
                );
                micro_ops.push(MicroOp::HeapLoadDyn { dst, obj, idx });
                vstack.push(Vse::Reg(dst));
            }
            Op::HeapStore(offset) => {
                // pop value, pop ref → ref[offset] = value
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst_obj = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                micro_ops.push(MicroOp::HeapStore {
                    dst_obj,
                    offset: *offset,
                    src,
                });
            }
            Op::HeapStoreDyn => {
                // pop value, pop index, pop ref → ref[index] = value
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let idx = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let obj = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                micro_ops.push(MicroOp::HeapStoreDyn { obj, idx, src });
            }
            Op::HeapLoad2 => {
                // pop index, pop ref → push heap[heap[ref][0]][idx]
                let idx = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let obj = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I64,
                );
                micro_ops.push(MicroOp::HeapLoad2 { dst, obj, idx });
                vstack.push(Vse::Reg(dst));
            }
            Op::HeapStore2 => {
                // pop value, pop index, pop ref → heap[heap[ref][0]][idx] = value
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let idx = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let obj = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                micro_ops.push(MicroOp::HeapStore2 { obj, idx, src });
            }

            // ============================================================
            // Raw with PC target remapping
            // ============================================================
            Op::TryBegin(handler_pc) => {
                flush_vstack(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
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
                        &mut vreg_types,
                    ));
                }
                args.reverse();
                let callee = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                flush_vstack(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let ret = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I64,
                );
                micro_ops.push(MicroOp::CallIndirect {
                    callee,
                    args,
                    ret: Some(ret),
                });
                vstack.push(Vse::Reg(ret));
            }

            // ============================================================
            // String operations → register-based
            // ============================================================
            Op::StringConst(idx) => {
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::Ref,
                );
                micro_ops.push(MicroOp::StringConst { dst, idx: *idx });
                vstack.push(Vse::RegRef(dst));
            }
            Op::FloatToString => {
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::Ref,
                );
                micro_ops.push(MicroOp::FloatToString { dst, src });
                vstack.push(Vse::RegRef(dst));
            }
            Op::FloatDigitCount => {
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I64,
                );
                micro_ops.push(MicroOp::FloatDigitCount { dst, src });
                vstack.push(Vse::Reg(dst));
            }
            Op::FloatWriteTo => {
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let offset = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let buf = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I64,
                );
                micro_ops.push(MicroOp::FloatWriteTo {
                    dst,
                    buf,
                    offset,
                    src,
                });
                vstack.push(Vse::Reg(dst));
            }
            Op::PrintDebug => {
                let src = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::I64,
                );
                micro_ops.push(MicroOp::PrintDebug { dst, src });
                vstack.push(Vse::Reg(dst));
            }

            // ============================================================
            // Heap allocation operations
            // ============================================================
            Op::HeapAllocDynSimple => {
                let size = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::Ref,
                );
                micro_ops.push(MicroOp::HeapAllocDynSimple { dst, size });
                vstack.push(Vse::RegRef(dst));
            }
            Op::HeapAllocArray(2, kind) if *kind == 1 || *kind == 2 => {
                // Typed 2-slot alloc (String or Array with ptr+len layout)
                let len = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let data_ref = pop_vreg(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
                let dst = alloc_temp(
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                    ValueType::Ref,
                );
                micro_ops.push(MicroOp::HeapAllocTyped {
                    dst,
                    data_ref,
                    len,
                    kind: *kind,
                });
                vstack.push(Vse::RegRef(dst));
            }

            // ============================================================
            // Raw fallback (everything else)
            // ============================================================
            _ => {
                flush_vstack(
                    &mut vstack,
                    &mut micro_ops,
                    &mut next_temp,
                    &mut max_temp,
                    &mut vreg_types,
                );
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

    ConvertedFunction {
        micro_ops,
        temps_count: temps_count.max(1),
        pc_map,
        vreg_types,
    }
}

// ─── Helper functions ───────────────────────────────────────────────

fn alloc_temp(
    next_temp: &mut usize,
    max_temp: &mut usize,
    vreg_types: &mut Vec<ValueType>,
    ty: ValueType,
) -> VReg {
    let v = VReg(*next_temp);
    *next_temp += 1;
    if *next_temp > *max_temp {
        *max_temp = *next_temp;
        // Extend vreg_types to cover the new slot
        while vreg_types.len() < *max_temp {
            vreg_types.push(ValueType::I64);
        }
    }
    // Set the type for this specific slot
    vreg_types[v.0] = ty;
    v
}

/// Pop a raw entry from the virtual stack, or emit StackPop.
fn pop_entry(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    vreg_types: &mut Vec<ValueType>,
) -> Vse {
    if let Some(e) = vstack.pop() {
        e
    } else {
        let t = alloc_temp(next_temp, max_temp, vreg_types, ValueType::I64);
        micro_ops.push(MicroOp::StackPop { dst: t });
        Vse::Reg(t)
    }
}

/// Materialize a Vse into a VReg, emitting ConstI64/ConstF64 if needed.
fn mat(
    entry: Vse,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    vreg_types: &mut Vec<ValueType>,
) -> VReg {
    match entry {
        Vse::Reg(v) | Vse::RegF64(v) | Vse::RegRef(v) => v,
        Vse::ImmI64(n) => {
            let t = alloc_temp(next_temp, max_temp, vreg_types, ValueType::I64);
            micro_ops.push(MicroOp::ConstI64 { dst: t, imm: n });
            t
        }
        Vse::ImmF64(n) => {
            let t = alloc_temp(next_temp, max_temp, vreg_types, ValueType::F64);
            micro_ops.push(MicroOp::ConstF64 { dst: t, imm: n });
            t
        }
    }
}

/// Check if a Vse holds a float value.
fn is_float(entry: &Vse) -> bool {
    matches!(entry, Vse::RegF64(_) | Vse::ImmF64(_))
}

/// Pop from vstack and materialize to VReg.
fn pop_vreg(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    vreg_types: &mut Vec<ValueType>,
) -> VReg {
    let e = pop_entry(vstack, micro_ops, next_temp, max_temp, vreg_types);
    mat(e, micro_ops, next_temp, max_temp, vreg_types)
}

/// Flush all virtual stack entries to the real operand stack.
fn flush_vstack(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    vreg_types: &mut Vec<ValueType>,
) {
    for entry in vstack.drain(..) {
        let v = mat(entry, micro_ops, next_temp, max_temp, vreg_types);
        micro_ops.push(MicroOp::StackPush { src: v });
    }
}

/// Emit a binary op: pop 2, push 1 result. Always materializes both operands.
fn emit_binop(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    vreg_types: &mut Vec<ValueType>,
    dst_type: ValueType,
    make_op: impl FnOnce(VReg, VReg, VReg) -> MicroOp,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp, vreg_types);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp, vreg_types);
    let dst = alloc_temp(next_temp, max_temp, vreg_types, dst_type);
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
/// Promotes to CmpF64 if either operand is float.
fn emit_cmp_i64(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    vreg_types: &mut Vec<ValueType>,
    cond: CmpCond,
    allow_imm: bool,
) {
    let b = pop_entry(vstack, micro_ops, next_temp, max_temp, vreg_types);
    let a = pop_entry(vstack, micro_ops, next_temp, max_temp, vreg_types);
    let dst = alloc_temp(next_temp, max_temp, vreg_types, ValueType::I64);
    if is_float(&a) || is_float(&b) {
        let a = mat(a, micro_ops, next_temp, max_temp, vreg_types);
        let b = mat(b, micro_ops, next_temp, max_temp, vreg_types);
        micro_ops.push(MicroOp::CmpF64 { dst, a, b, cond });
    } else {
        match (a, b) {
            (_, Vse::ImmI64(imm)) if allow_imm => {
                let a = mat(a, micro_ops, next_temp, max_temp, vreg_types);
                micro_ops.push(MicroOp::CmpI64Imm { dst, a, imm, cond });
            }
            (Vse::ImmI64(imm), _) if allow_imm => {
                // imm <cond> b → b <reverse_cond> imm
                let b = mat(b, micro_ops, next_temp, max_temp, vreg_types);
                micro_ops.push(MicroOp::CmpI64Imm {
                    dst,
                    a: b,
                    imm,
                    cond: reverse_cond(cond),
                });
            }
            _ => {
                let a = mat(a, micro_ops, next_temp, max_temp, vreg_types);
                let b = mat(b, micro_ops, next_temp, max_temp, vreg_types);
                micro_ops.push(MicroOp::CmpI64 { dst, a, b, cond });
            }
        }
    }
    vstack.push(Vse::Reg(dst));
}

fn emit_cmp_i32(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    vreg_types: &mut Vec<ValueType>,
    cond: CmpCond,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp, vreg_types);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp, vreg_types);
    let dst = alloc_temp(next_temp, max_temp, vreg_types, ValueType::I32);
    micro_ops.push(MicroOp::CmpI32 { dst, a, b, cond });
    vstack.push(Vse::Reg(dst));
}

fn emit_cmp_f64(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    vreg_types: &mut Vec<ValueType>,
    cond: CmpCond,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp, vreg_types);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp, vreg_types);
    let dst = alloc_temp(next_temp, max_temp, vreg_types, ValueType::I64);
    micro_ops.push(MicroOp::CmpF64 { dst, a, b, cond });
    vstack.push(Vse::Reg(dst));
}

fn emit_cmp_f32(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    vreg_types: &mut Vec<ValueType>,
    cond: CmpCond,
) {
    let b = pop_vreg(vstack, micro_ops, next_temp, max_temp, vreg_types);
    let a = pop_vreg(vstack, micro_ops, next_temp, max_temp, vreg_types);
    let dst = alloc_temp(next_temp, max_temp, vreg_types, ValueType::I64);
    micro_ops.push(MicroOp::CmpF32 { dst, a, b, cond });
    vstack.push(Vse::Reg(dst));
}

/// Try to patch the dst field of a MicroOp to `new_dst`.
/// Returns `Some(old_dst)` if the op is a patchable arithmetic/comparison/conversion op,
/// or `None` if the op is not patchable.
fn try_patch_dst(mop: &mut MicroOp, new_dst: VReg) -> Option<VReg> {
    match mop {
        // Binary i64
        MicroOp::AddI64 { dst, .. }
        | MicroOp::AddI64Imm { dst, .. }
        | MicroOp::SubI64 { dst, .. }
        | MicroOp::MulI64 { dst, .. }
        | MicroOp::DivI64 { dst, .. }
        | MicroOp::RemI64 { dst, .. }
        // Binary i32
        | MicroOp::AddI32 { dst, .. }
        | MicroOp::SubI32 { dst, .. }
        | MicroOp::MulI32 { dst, .. }
        | MicroOp::DivI32 { dst, .. }
        | MicroOp::RemI32 { dst, .. }
        // Binary f64
        | MicroOp::AddF64 { dst, .. }
        | MicroOp::SubF64 { dst, .. }
        | MicroOp::MulF64 { dst, .. }
        | MicroOp::DivF64 { dst, .. }
        // Binary f32
        | MicroOp::AddF32 { dst, .. }
        | MicroOp::SubF32 { dst, .. }
        | MicroOp::MulF32 { dst, .. }
        | MicroOp::DivF32 { dst, .. }
        // Comparisons
        | MicroOp::CmpI64 { dst, .. }
        | MicroOp::CmpI64Imm { dst, .. }
        | MicroOp::CmpI32 { dst, .. }
        | MicroOp::CmpF64 { dst, .. }
        | MicroOp::CmpF32 { dst, .. }
        // Ref binary
        | MicroOp::RefEq { dst, .. }
        // Unary ops
        | MicroOp::NegI64 { dst, .. }
        | MicroOp::NegF64 { dst, .. }
        | MicroOp::NegF32 { dst, .. }
        | MicroOp::EqzI32 { dst, .. }
        | MicroOp::RefIsNull { dst, .. }
        // Type conversions
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
        | MicroOp::F64PromoteF32 { dst, .. } => {
            let old = *dst;
            *dst = new_dst;
            Some(old)
        }
        _ => None,
    }
}

/// Emit a unary type conversion: pop 1, push 1.
fn emit_unary_conv(
    vstack: &mut Vec<Vse>,
    micro_ops: &mut Vec<MicroOp>,
    next_temp: &mut usize,
    max_temp: &mut usize,
    vreg_types: &mut Vec<ValueType>,
    dst_type: ValueType,
    make_op: impl FnOnce(VReg, VReg) -> MicroOp,
) {
    let src = pop_vreg(vstack, micro_ops, next_temp, max_temp, vreg_types);
    let dst = alloc_temp(next_temp, max_temp, vreg_types, dst_type);
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
        let func = make_func(vec![Op::I64Const(42), Op::LocalSet(0)]);
        let converted = convert(&func);
        // ConstI64 + Mov = 2 MicroOps
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
        // LocalGet(0) + LocalGet(1) + I64Add + LocalSet(0)
        // → dst forwarding: AddI64 writes directly to v0 (no Mov)
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
    fn test_dst_forwarding_add_imm() {
        // LocalGet(0) + I64Const(1) + I64Add + LocalSet(0)
        // → dst forwarding: AddI64Imm writes directly to VReg(0)
        let func = make_func(vec![
            Op::LocalGet(0),
            Op::I64Const(1),
            Op::I64Add,
            Op::LocalSet(0),
        ]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 1);
        assert_eq!(
            converted.micro_ops[0],
            MicroOp::AddI64Imm {
                dst: VReg(0),
                a: VReg(0),
                imm: 1
            }
        );
    }

    #[test]
    fn test_dst_forwarding_not_applied_when_vstack_has_ref() {
        // LocalGet(0) + LocalGet(0) + I64Const(1) + I64Add + LocalSet(0)
        // vstack still has a reference to VReg(0) below the temp,
        // so dst forwarding should NOT apply (condition 4)
        let func = make_func(vec![
            Op::LocalGet(0),
            Op::LocalGet(0),
            Op::I64Const(1),
            Op::I64Add,
            Op::LocalSet(0),
        ]);
        let converted = convert(&func);
        // AddI64Imm(temp) + Mov(v0←temp) remain, plus vstack still has v0
        assert!(converted.micro_ops.len() >= 2);
        assert!(matches!(
            converted.micro_ops.last(),
            Some(MicroOp::Mov { .. })
        ));
    }

    #[test]
    fn test_dst_forwarding_to_different_local() {
        // LocalGet(0) + LocalGet(1) + I64Add + LocalSet(1)
        // → dst forwarding: AddI64 writes directly to VReg(1)
        let func = make_func(vec![
            Op::LocalGet(0),
            Op::LocalGet(1),
            Op::I64Add,
            Op::LocalSet(1),
        ]);
        let converted = convert(&func);
        assert_eq!(converted.micro_ops.len(), 1);
        assert_eq!(
            converted.micro_ops[0],
            MicroOp::AddI64 {
                dst: VReg(1),
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
        // LocalSet materializes 1 → ConstI64 + Mov = 2
        assert_eq!(converted.micro_ops.len(), 2);
    }

    #[test]
    fn test_vreg_types_populated() {
        let func = Function {
            name: "test".to_string(),
            arity: 2,
            locals_count: 3,
            code: vec![
                Op::LocalGet(0),
                Op::LocalGet(1),
                Op::I64Add,
                Op::LocalSet(2),
            ],
            stackmap: None,
            local_types: vec![ValueType::I64, ValueType::I64, ValueType::I64],
        };
        let converted = convert(&func);
        // 3 locals + at least 1 temp
        assert!(converted.vreg_types.len() >= 3);
        assert_eq!(converted.vreg_types[0], ValueType::I64);
        assert_eq!(converted.vreg_types[1], ValueType::I64);
        assert_eq!(converted.vreg_types[2], ValueType::I64);
    }

    #[test]
    fn test_vreg_types_float_temp() {
        let func = Function {
            name: "test".to_string(),
            arity: 0,
            locals_count: 1,
            code: vec![Op::F64Const(3.14), Op::LocalSet(0)],
            stackmap: None,
            local_types: vec![ValueType::F64],
        };
        let converted = convert(&func);
        assert_eq!(converted.vreg_types[0], ValueType::F64);
        // temp for F64Const should be F64
        assert_eq!(converted.vreg_types[1], ValueType::F64);
    }
}
