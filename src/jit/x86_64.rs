//! x86-64 instruction encoding for JIT compilation.
//!
//! This module provides functions for encoding x86-64 instructions
//! as machine code bytes. Uses System V AMD64 ABI conventions.

use super::codebuf::CodeBuffer;

/// x86-64 general-purpose registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Reg {
    // Caller-saved (scratch) registers
    Rax = 0,  // Return value
    Rcx = 1,  // 4th argument
    Rdx = 2,  // 3rd argument
    Rbx = 3,  // Callee-saved
    Rsp = 4,  // Stack pointer
    Rbp = 5,  // Frame pointer (callee-saved)
    Rsi = 6,  // 2nd argument
    Rdi = 7,  // 1st argument
    R8 = 8,   // 5th argument
    R9 = 9,   // 6th argument
    R10 = 10, // Caller-saved
    R11 = 11, // Caller-saved
    R12 = 12, // Callee-saved
    R13 = 13, // Callee-saved
    R14 = 14, // Callee-saved
    R15 = 15, // Callee-saved
}

impl Reg {
    /// Get the register code (lower 3 bits).
    pub fn code(self) -> u8 {
        (self as u8) & 0x7
    }

    /// Check if this register requires REX.B or REX.R extension.
    pub fn needs_rex_ext(self) -> bool {
        (self as u8) >= 8
    }

    /// Get the REX.B bit for this register (when used as base/rm).
    pub fn rex_b(self) -> u8 {
        if self.needs_rex_ext() { 0x01 } else { 0x00 }
    }

    /// Get the REX.R bit for this register (when used as reg).
    pub fn rex_r(self) -> u8 {
        if self.needs_rex_ext() { 0x04 } else { 0x00 }
    }
}

/// x86-64 condition codes (for Jcc, SETcc, CMOVcc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Cond {
    O = 0x0,  // Overflow
    No = 0x1, // Not overflow
    B = 0x2,  // Below (unsigned <)
    Ae = 0x3, // Above or equal (unsigned >=)
    E = 0x4,  // Equal
    Ne = 0x5, // Not equal
    Be = 0x6, // Below or equal (unsigned <=)
    A = 0x7,  // Above (unsigned >)
    S = 0x8,  // Sign (negative)
    Ns = 0x9, // Not sign (non-negative)
    P = 0xA,  // Parity even
    Np = 0xB, // Parity odd
    L = 0xC,  // Less (signed <)
    Ge = 0xD, // Greater or equal (signed >=)
    Le = 0xE, // Less or equal (signed <=)
    G = 0xF,  // Greater (signed >)
}

impl Cond {
    /// Invert the condition.
    pub fn invert(self) -> Self {
        match self {
            Cond::O => Cond::No,
            Cond::No => Cond::O,
            Cond::B => Cond::Ae,
            Cond::Ae => Cond::B,
            Cond::E => Cond::Ne,
            Cond::Ne => Cond::E,
            Cond::Be => Cond::A,
            Cond::A => Cond::Be,
            Cond::S => Cond::Ns,
            Cond::Ns => Cond::S,
            Cond::P => Cond::Np,
            Cond::Np => Cond::P,
            Cond::L => Cond::Ge,
            Cond::Ge => Cond::L,
            Cond::Le => Cond::G,
            Cond::G => Cond::Le,
        }
    }
}

/// x86-64 assembler.
pub struct X86_64Assembler<'a> {
    buf: &'a mut CodeBuffer,
}

impl<'a> X86_64Assembler<'a> {
    pub fn new(buf: &'a mut CodeBuffer) -> Self {
        Self { buf }
    }

    // ==================== REX prefix helpers ====================

    /// Emit REX.W prefix for 64-bit operations.
    fn emit_rex_w(&mut self, reg: Reg, rm: Reg) {
        let rex = 0x48 | reg.rex_r() | rm.rex_b();
        self.buf.emit_u8(rex);
    }

    /// Emit REX.W prefix for single register operations.
    fn emit_rex_w_single(&mut self, rm: Reg) {
        let rex = 0x48 | rm.rex_b();
        self.buf.emit_u8(rex);
    }

    /// Emit REX prefix if needed (without W bit).
    fn emit_rex_if_needed(&mut self, reg: Reg, rm: Reg) {
        let rex = 0x40 | reg.rex_r() | rm.rex_b();
        if rex != 0x40 {
            self.buf.emit_u8(rex);
        }
    }

    // ==================== ModR/M helpers ====================

    /// Encode ModR/M byte.
    /// mod: 2 bits, reg: 3 bits, rm: 3 bits
    fn modrm(mode: u8, reg: u8, rm: u8) -> u8 {
        ((mode & 0x3) << 6) | ((reg & 0x7) << 3) | (rm & 0x7)
    }

    // ==================== Data Movement ====================

    /// MOV r64, r64 (register to register)
    pub fn mov_rr(&mut self, dst: Reg, src: Reg) {
        self.emit_rex_w(src, dst);
        self.buf.emit_u8(0x89); // MOV r/m64, r64
        self.buf.emit_u8(Self::modrm(0b11, src.code(), dst.code()));
    }

    /// MOV r64, imm64 (move 64-bit immediate to register)
    pub fn mov_ri64(&mut self, dst: Reg, imm: i64) {
        self.emit_rex_w_single(dst);
        self.buf.emit_u8(0xB8 + dst.code()); // MOV r64, imm64
        self.buf.emit_u64(imm as u64);
    }

    /// MOV r64, imm32 (sign-extended 32-bit immediate to register)
    pub fn mov_ri32(&mut self, dst: Reg, imm: i32) {
        self.emit_rex_w_single(dst);
        self.buf.emit_u8(0xC7); // MOV r/m64, imm32
        self.buf.emit_u8(Self::modrm(0b11, 0, dst.code()));
        self.buf.emit_u32(imm as u32);
    }

    /// MOV r64, [r64 + disp32] (load from memory)
    pub fn mov_rm(&mut self, dst: Reg, base: Reg, disp: i32) {
        self.emit_rex_w(dst, base);
        self.buf.emit_u8(0x8B); // MOV r64, r/m64

        if base == Reg::Rsp || base == Reg::R12 {
            // Need SIB byte for RSP/R12 as base
            if disp == 0 && base != Reg::Rbp && base != Reg::R13 {
                self.buf.emit_u8(Self::modrm(0b00, dst.code(), 0b100));
                self.buf.emit_u8(0x24); // SIB: scale=0, index=RSP(none), base=RSP
            } else if (-128..=127).contains(&disp) {
                self.buf.emit_u8(Self::modrm(0b01, dst.code(), 0b100));
                self.buf.emit_u8(0x24); // SIB
                self.buf.emit_u8(disp as u8);
            } else {
                self.buf.emit_u8(Self::modrm(0b10, dst.code(), 0b100));
                self.buf.emit_u8(0x24); // SIB
                self.buf.emit_u32(disp as u32);
            }
        } else if disp == 0 && base != Reg::Rbp && base != Reg::R13 {
            self.buf.emit_u8(Self::modrm(0b00, dst.code(), base.code()));
        } else if (-128..=127).contains(&disp) {
            self.buf.emit_u8(Self::modrm(0b01, dst.code(), base.code()));
            self.buf.emit_u8(disp as u8);
        } else {
            self.buf.emit_u8(Self::modrm(0b10, dst.code(), base.code()));
            self.buf.emit_u32(disp as u32);
        }
    }

    /// MOV r64, [base + index*8] (load with SIB, scale=8, no displacement)
    pub fn mov_rm_sib_scale8(&mut self, dst: Reg, base: Reg, index: Reg) {
        // REX.W prefix: need to encode dst (REX.R), index (REX.X), base (REX.B)
        let rex = 0x48 | dst.rex_r() | if index.needs_rex_ext() { 0x02 } else { 0 } | base.rex_b();
        self.buf.emit_u8(rex);
        self.buf.emit_u8(0x8B); // MOV r64, r/m64
        // ModRM: mod=00, reg=dst, rm=100 (SIB follows)
        self.buf.emit_u8(Self::modrm(0b00, dst.code(), 0b100));
        // SIB: scale=11 (×8), index=index, base=base
        let sib = (0b11 << 6) | ((index.code() & 0x7) << 3) | (base.code() & 0x7);
        self.buf.emit_u8(sib);
    }

    /// MOV [r64 + disp32], r64 (store to memory)
    pub fn mov_mr(&mut self, base: Reg, disp: i32, src: Reg) {
        self.emit_rex_w(src, base);
        self.buf.emit_u8(0x89); // MOV r/m64, r64

        if base == Reg::Rsp || base == Reg::R12 {
            // Need SIB byte for RSP/R12 as base
            if disp == 0 && base != Reg::Rbp && base != Reg::R13 {
                self.buf.emit_u8(Self::modrm(0b00, src.code(), 0b100));
                self.buf.emit_u8(0x24); // SIB
            } else if (-128..=127).contains(&disp) {
                self.buf.emit_u8(Self::modrm(0b01, src.code(), 0b100));
                self.buf.emit_u8(0x24); // SIB
                self.buf.emit_u8(disp as u8);
            } else {
                self.buf.emit_u8(Self::modrm(0b10, src.code(), 0b100));
                self.buf.emit_u8(0x24); // SIB
                self.buf.emit_u32(disp as u32);
            }
        } else if disp == 0 && base != Reg::Rbp && base != Reg::R13 {
            self.buf.emit_u8(Self::modrm(0b00, src.code(), base.code()));
        } else if (-128..=127).contains(&disp) {
            self.buf.emit_u8(Self::modrm(0b01, src.code(), base.code()));
            self.buf.emit_u8(disp as u8);
        } else {
            self.buf.emit_u8(Self::modrm(0b10, src.code(), base.code()));
            self.buf.emit_u32(disp as u32);
        }
    }

    // ==================== Arithmetic Operations ====================

    /// ADD r64, r64
    pub fn add_rr(&mut self, dst: Reg, src: Reg) {
        self.emit_rex_w(src, dst);
        self.buf.emit_u8(0x01); // ADD r/m64, r64
        self.buf.emit_u8(Self::modrm(0b11, src.code(), dst.code()));
    }

    /// ADD r64, imm32 (sign-extended)
    pub fn add_ri32(&mut self, dst: Reg, imm: i32) {
        self.emit_rex_w_single(dst);
        if (-128..=127).contains(&imm) {
            self.buf.emit_u8(0x83); // ADD r/m64, imm8
            self.buf.emit_u8(Self::modrm(0b11, 0, dst.code()));
            self.buf.emit_u8(imm as u8);
        } else {
            self.buf.emit_u8(0x81); // ADD r/m64, imm32
            self.buf.emit_u8(Self::modrm(0b11, 0, dst.code()));
            self.buf.emit_u32(imm as u32);
        }
    }

    /// SUB r64, r64
    pub fn sub_rr(&mut self, dst: Reg, src: Reg) {
        self.emit_rex_w(src, dst);
        self.buf.emit_u8(0x29); // SUB r/m64, r64
        self.buf.emit_u8(Self::modrm(0b11, src.code(), dst.code()));
    }

    /// SUB r64, imm32 (sign-extended)
    pub fn sub_ri32(&mut self, dst: Reg, imm: i32) {
        self.emit_rex_w_single(dst);
        if (-128..=127).contains(&imm) {
            self.buf.emit_u8(0x83); // SUB r/m64, imm8
            self.buf.emit_u8(Self::modrm(0b11, 5, dst.code()));
            self.buf.emit_u8(imm as u8);
        } else {
            self.buf.emit_u8(0x81); // SUB r/m64, imm32
            self.buf.emit_u8(Self::modrm(0b11, 5, dst.code()));
            self.buf.emit_u32(imm as u32);
        }
    }

    /// IMUL r64, r64 (signed multiply, result in first operand)
    pub fn imul_rr(&mut self, dst: Reg, src: Reg) {
        self.emit_rex_w(dst, src);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0xAF); // IMUL r64, r/m64
        self.buf.emit_u8(Self::modrm(0b11, dst.code(), src.code()));
    }

    /// IMUL r64, r64, imm32 (signed multiply with immediate)
    pub fn imul_rri32(&mut self, dst: Reg, src: Reg, imm: i32) {
        self.emit_rex_w(dst, src);
        if (-128..=127).contains(&imm) {
            self.buf.emit_u8(0x6B); // IMUL r64, r/m64, imm8
            self.buf.emit_u8(Self::modrm(0b11, dst.code(), src.code()));
            self.buf.emit_u8(imm as u8);
        } else {
            self.buf.emit_u8(0x69); // IMUL r64, r/m64, imm32
            self.buf.emit_u8(Self::modrm(0b11, dst.code(), src.code()));
            self.buf.emit_u32(imm as u32);
        }
    }

    /// IDIV r64 (signed divide RDX:RAX by r64, quotient in RAX, remainder in RDX)
    pub fn idiv(&mut self, src: Reg) {
        self.emit_rex_w_single(src);
        self.buf.emit_u8(0xF7); // IDIV r/m64
        self.buf.emit_u8(Self::modrm(0b11, 7, src.code()));
    }

    /// CQO (sign-extend RAX into RDX:RAX, needed before IDIV)
    pub fn cqo(&mut self) {
        self.buf.emit_u8(0x48); // REX.W
        self.buf.emit_u8(0x99); // CQO
    }

    /// CMP r64, r64
    pub fn cmp_rr(&mut self, dst: Reg, src: Reg) {
        self.emit_rex_w(src, dst);
        self.buf.emit_u8(0x39); // CMP r/m64, r64
        self.buf.emit_u8(Self::modrm(0b11, src.code(), dst.code()));
    }

    /// CMP r64, imm32 (sign-extended)
    pub fn cmp_ri32(&mut self, dst: Reg, imm: i32) {
        self.emit_rex_w_single(dst);
        if (-128..=127).contains(&imm) {
            self.buf.emit_u8(0x83); // CMP r/m64, imm8
            self.buf.emit_u8(Self::modrm(0b11, 7, dst.code()));
            self.buf.emit_u8(imm as u8);
        } else {
            self.buf.emit_u8(0x81); // CMP r/m64, imm32
            self.buf.emit_u8(Self::modrm(0b11, 7, dst.code()));
            self.buf.emit_u32(imm as u32);
        }
    }

    /// AND r64, r64
    pub fn and_rr(&mut self, dst: Reg, src: Reg) {
        self.emit_rex_w(src, dst);
        self.buf.emit_u8(0x21); // AND r/m64, r64
        self.buf.emit_u8(Self::modrm(0b11, src.code(), dst.code()));
    }

    /// AND r64, imm32 (sign-extended)
    pub fn and_ri32(&mut self, dst: Reg, imm: i32) {
        self.emit_rex_w_single(dst);
        if (-128..=127).contains(&imm) {
            self.buf.emit_u8(0x83); // AND r/m64, imm8
            self.buf.emit_u8(Self::modrm(0b11, 4, dst.code()));
            self.buf.emit_u8(imm as u8);
        } else {
            self.buf.emit_u8(0x81); // AND r/m64, imm32
            self.buf.emit_u8(Self::modrm(0b11, 4, dst.code()));
            self.buf.emit_u32(imm as u32);
        }
    }

    /// OR r64, r64
    pub fn or_rr(&mut self, dst: Reg, src: Reg) {
        self.emit_rex_w(src, dst);
        self.buf.emit_u8(0x09); // OR r/m64, r64
        self.buf.emit_u8(Self::modrm(0b11, src.code(), dst.code()));
    }

    /// OR r64, imm32 (sign-extended)
    pub fn or_ri32(&mut self, dst: Reg, imm: i32) {
        self.emit_rex_w_single(dst);
        if (-128..=127).contains(&imm) {
            self.buf.emit_u8(0x83); // OR r/m64, imm8
            self.buf.emit_u8(Self::modrm(0b11, 1, dst.code()));
            self.buf.emit_u8(imm as u8);
        } else {
            self.buf.emit_u8(0x81); // OR r/m64, imm32
            self.buf.emit_u8(Self::modrm(0b11, 1, dst.code()));
            self.buf.emit_u32(imm as u32);
        }
    }

    /// SHL r64, imm8 (logical left shift)
    pub fn shl_ri(&mut self, dst: Reg, imm: u8) {
        self.emit_rex_w_single(dst);
        self.buf.emit_u8(0xC1); // SHL r/m64, imm8
        self.buf.emit_u8(Self::modrm(0b11, 4, dst.code()));
        self.buf.emit_u8(imm);
    }

    /// SHR r64, imm8 (logical right shift)
    pub fn shr_ri(&mut self, dst: Reg, imm: u8) {
        self.emit_rex_w_single(dst);
        self.buf.emit_u8(0xC1); // SHR r/m64, imm8
        self.buf.emit_u8(Self::modrm(0b11, 5, dst.code()));
        self.buf.emit_u8(imm);
    }

    /// SHL r64, CL (left shift by CL register)
    pub fn shl_cl(&mut self, dst: Reg) {
        self.emit_rex_w_single(dst);
        self.buf.emit_u8(0xD3); // SHL r/m64, CL
        self.buf.emit_u8(Self::modrm(0b11, 4, dst.code()));
    }

    /// SAR r64, CL (arithmetic right shift by CL register)
    pub fn sar_cl(&mut self, dst: Reg) {
        self.emit_rex_w_single(dst);
        self.buf.emit_u8(0xD3); // SAR r/m64, CL
        self.buf.emit_u8(Self::modrm(0b11, 7, dst.code()));
    }

    /// SAR r64, imm8 (arithmetic right shift by immediate)
    pub fn sar_ri(&mut self, dst: Reg, imm: u8) {
        self.emit_rex_w_single(dst);
        self.buf.emit_u8(0xC1); // SAR r/m64, imm8
        self.buf.emit_u8(Self::modrm(0b11, 7, dst.code()));
        self.buf.emit_u8(imm);
    }

    /// SHR r64, CL (logical right shift by CL register)
    pub fn shr_cl(&mut self, dst: Reg) {
        self.emit_rex_w_single(dst);
        self.buf.emit_u8(0xD3); // SHR r/m64, CL
        self.buf.emit_u8(Self::modrm(0b11, 5, dst.code()));
    }

    /// MUL r/m64 (unsigned multiply: RDX:RAX = RAX * r/m64)
    pub fn mul_r(&mut self, src: Reg) {
        self.emit_rex_w_single(src);
        self.buf.emit_u8(0xF7); // MUL r/m64
        self.buf.emit_u8(Self::modrm(0b11, 4, src.code()));
    }

    /// XOR r64, r64
    pub fn xor_rr(&mut self, dst: Reg, src: Reg) {
        self.emit_rex_w(src, dst);
        self.buf.emit_u8(0x31); // XOR r/m64, r64
        self.buf.emit_u8(Self::modrm(0b11, src.code(), dst.code()));
    }

    /// XOR r64, imm32 (sign-extended)
    pub fn xor_ri32(&mut self, dst: Reg, imm: i32) {
        self.emit_rex_w_single(dst);
        if (-128..=127).contains(&imm) {
            self.buf.emit_u8(0x83); // XOR r/m64, imm8
            self.buf.emit_u8(Self::modrm(0b11, 6, dst.code()));
            self.buf.emit_u8(imm as u8);
        } else {
            self.buf.emit_u8(0x81); // XOR r/m64, imm32
            self.buf.emit_u8(Self::modrm(0b11, 6, dst.code()));
            self.buf.emit_u32(imm as u32);
        }
    }

    /// NEG r64 (two's complement negation)
    pub fn neg(&mut self, dst: Reg) {
        self.emit_rex_w_single(dst);
        self.buf.emit_u8(0xF7); // NEG r/m64
        self.buf.emit_u8(Self::modrm(0b11, 3, dst.code()));
    }

    /// TEST r64, r64 (bitwise AND, set flags, discard result)
    pub fn test_rr(&mut self, dst: Reg, src: Reg) {
        self.emit_rex_w(src, dst);
        self.buf.emit_u8(0x85); // TEST r/m64, r64
        self.buf.emit_u8(Self::modrm(0b11, src.code(), dst.code()));
    }

    // ==================== Stack Operations ====================

    /// PUSH r64
    pub fn push(&mut self, reg: Reg) {
        if reg.needs_rex_ext() {
            self.buf.emit_u8(0x41); // REX.B
        }
        self.buf.emit_u8(0x50 + reg.code());
    }

    /// POP r64
    pub fn pop(&mut self, reg: Reg) {
        if reg.needs_rex_ext() {
            self.buf.emit_u8(0x41); // REX.B
        }
        self.buf.emit_u8(0x58 + reg.code());
    }

    // ==================== Control Flow ====================

    /// JMP rel32 (relative jump, near)
    pub fn jmp_rel32(&mut self, offset: i32) {
        self.buf.emit_u8(0xE9); // JMP rel32
        self.buf.emit_u32(offset as u32);
    }

    /// JMP rel8 (short jump)
    pub fn jmp_rel8(&mut self, offset: i8) {
        self.buf.emit_u8(0xEB); // JMP rel8
        self.buf.emit_u8(offset as u8);
    }

    /// JE rel32 (jump if equal/zero)
    pub fn je_rel32(&mut self, offset: i32) {
        self.buf.emit_u8(0x0F); // Two-byte opcode prefix
        self.buf.emit_u8(0x84); // JE rel32
        self.buf.emit_u32(offset as u32);
    }

    /// JNE rel32 (jump if not equal/not zero)
    pub fn jne_rel32(&mut self, offset: i32) {
        self.buf.emit_u8(0x0F); // Two-byte opcode prefix
        self.buf.emit_u8(0x85); // JNE rel32
        self.buf.emit_u32(offset as u32);
    }

    /// Jcc rel32 (conditional jump, near)
    pub fn jcc_rel32(&mut self, cond: Cond, offset: i32) {
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x80 + cond as u8); // Jcc rel32
        self.buf.emit_u32(offset as u32);
    }

    /// Jcc rel8 (conditional short jump)
    pub fn jcc_rel8(&mut self, cond: Cond, offset: i8) {
        self.buf.emit_u8(0x70 + cond as u8); // Jcc rel8
        self.buf.emit_u8(offset as u8);
    }

    /// CALL rel32 (relative call)
    pub fn call_rel32(&mut self, offset: i32) {
        self.buf.emit_u8(0xE8); // CALL rel32
        self.buf.emit_u32(offset as u32);
    }

    /// CALL r64 (indirect call through register)
    pub fn call_r(&mut self, reg: Reg) {
        if reg.needs_rex_ext() {
            self.buf.emit_u8(0x41); // REX.B
        }
        self.buf.emit_u8(0xFF); // CALL r/m64
        self.buf.emit_u8(Self::modrm(0b11, 2, reg.code()));
    }

    /// JMP r64 (indirect jump through register)
    pub fn jmp_r(&mut self, reg: Reg) {
        if reg.needs_rex_ext() {
            self.buf.emit_u8(0x41); // REX.B
        }
        self.buf.emit_u8(0xFF); // JMP r/m64
        self.buf.emit_u8(Self::modrm(0b11, 4, reg.code()));
    }

    /// RET (return)
    pub fn ret(&mut self) {
        self.buf.emit_u8(0xC3);
    }

    /// NOP (no operation)
    pub fn nop(&mut self) {
        self.buf.emit_u8(0x90);
    }

    // ==================== Conditional Set ====================

    /// SETcc r8 (set byte based on condition)
    pub fn setcc(&mut self, cond: Cond, dst: Reg) {
        if dst.needs_rex_ext()
            || dst == Reg::Rsp
            || dst == Reg::Rbp
            || dst == Reg::Rsi
            || dst == Reg::Rdi
        {
            // Need REX prefix to access SPL, BPL, SIL, DIL or R8B-R15B
            self.buf.emit_u8(0x40 | dst.rex_b());
        }
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x90 + cond as u8); // SETcc r/m8
        self.buf.emit_u8(Self::modrm(0b11, 0, dst.code()));
    }

    /// MOVZX r64, r8 (zero-extend byte to qword)
    pub fn movzx_r64_r8(&mut self, dst: Reg, src: Reg) {
        self.emit_rex_w(dst, src);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0xB6); // MOVZX r64, r/m8
        self.buf.emit_u8(Self::modrm(0b11, dst.code(), src.code()));
    }

    /// MOVZX r64, BYTE PTR [base + disp32] (load byte with zero-extension to 64-bit)
    pub fn movzx_rm_byte(&mut self, dst: Reg, base: Reg, disp: i32) {
        self.emit_rex_w(dst, base);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0xB6); // MOVZX r64, r/m8

        if base == Reg::Rsp || base == Reg::R12 {
            if disp == 0 && base != Reg::Rbp && base != Reg::R13 {
                self.buf.emit_u8(Self::modrm(0b00, dst.code(), 0b100));
                self.buf.emit_u8(0x24);
            } else if (-128..=127).contains(&disp) {
                self.buf.emit_u8(Self::modrm(0b01, dst.code(), 0b100));
                self.buf.emit_u8(0x24);
                self.buf.emit_u8(disp as u8);
            } else {
                self.buf.emit_u8(Self::modrm(0b10, dst.code(), 0b100));
                self.buf.emit_u8(0x24);
                self.buf.emit_u32(disp as u32);
            }
        } else if disp == 0 && base != Reg::Rbp && base != Reg::R13 {
            self.buf.emit_u8(Self::modrm(0b00, dst.code(), base.code()));
        } else if (-128..=127).contains(&disp) {
            self.buf.emit_u8(Self::modrm(0b01, dst.code(), base.code()));
            self.buf.emit_u8(disp as u8);
        } else {
            self.buf.emit_u8(Self::modrm(0b10, dst.code(), base.code()));
            self.buf.emit_u32(disp as u32);
        }
    }

    /// MOV BYTE PTR [base + disp32], r8 (store low byte of register to memory)
    pub fn mov_mr_byte(&mut self, base: Reg, disp: i32, src: Reg) {
        // Always emit REX to ensure SPL/BPL/SIL/DIL encoding (avoid AH/CH/DH/BH trap)
        let rex = 0x40 | src.rex_r() | base.rex_b();
        self.buf.emit_u8(rex);
        self.buf.emit_u8(0x88); // MOV r/m8, r8

        if base == Reg::Rsp || base == Reg::R12 {
            if disp == 0 && base != Reg::Rbp && base != Reg::R13 {
                self.buf.emit_u8(Self::modrm(0b00, src.code(), 0b100));
                self.buf.emit_u8(0x24);
            } else if (-128..=127).contains(&disp) {
                self.buf.emit_u8(Self::modrm(0b01, src.code(), 0b100));
                self.buf.emit_u8(0x24);
                self.buf.emit_u8(disp as u8);
            } else {
                self.buf.emit_u8(Self::modrm(0b10, src.code(), 0b100));
                self.buf.emit_u8(0x24);
                self.buf.emit_u32(disp as u32);
            }
        } else if disp == 0 && base != Reg::Rbp && base != Reg::R13 {
            self.buf.emit_u8(Self::modrm(0b00, src.code(), base.code()));
        } else if (-128..=127).contains(&disp) {
            self.buf.emit_u8(Self::modrm(0b01, src.code(), base.code()));
            self.buf.emit_u8(disp as u8);
        } else {
            self.buf.emit_u8(Self::modrm(0b10, src.code(), base.code()));
            self.buf.emit_u32(disp as u32);
        }
    }

    // ==================== SSE2 Floating Point ====================

    /// MOVQ xmm, r64 (move quadword from GP register to XMM)
    pub fn movq_xmm_r64(&mut self, xmm: u8, src: Reg) {
        // 66 REX.W 0F 6E /r - MOVQ xmm, r/m64
        self.buf.emit_u8(0x66);
        let rex = 0x48 | src.rex_b();
        self.buf.emit_u8(rex);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x6E);
        self.buf.emit_u8(Self::modrm(0b11, xmm, src.code()));
    }

    /// MOVQ r64, xmm (move quadword from XMM to GP register)
    pub fn movq_r64_xmm(&mut self, dst: Reg, xmm: u8) {
        // 66 REX.W 0F 7E /r - MOVQ r/m64, xmm
        self.buf.emit_u8(0x66);
        let rex = 0x48 | dst.rex_b();
        self.buf.emit_u8(rex);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x7E);
        self.buf.emit_u8(Self::modrm(0b11, xmm, dst.code()));
    }

    /// ADDSD xmm1, xmm2 (add scalar double-precision)
    pub fn addsd(&mut self, dst: u8, src: u8) {
        // F2 0F 58 /r - ADDSD xmm1, xmm2/m64
        self.buf.emit_u8(0xF2);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x58);
        self.buf.emit_u8(Self::modrm(0b11, dst, src));
    }

    /// SUBSD xmm1, xmm2 (subtract scalar double-precision)
    pub fn subsd(&mut self, dst: u8, src: u8) {
        // F2 0F 5C /r - SUBSD xmm1, xmm2/m64
        self.buf.emit_u8(0xF2);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x5C);
        self.buf.emit_u8(Self::modrm(0b11, dst, src));
    }

    /// MULSD xmm1, xmm2 (multiply scalar double-precision)
    pub fn mulsd(&mut self, dst: u8, src: u8) {
        // F2 0F 59 /r - MULSD xmm1, xmm2/m64
        self.buf.emit_u8(0xF2);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x59);
        self.buf.emit_u8(Self::modrm(0b11, dst, src));
    }

    /// DIVSD xmm1, xmm2 (divide scalar double-precision)
    pub fn divsd(&mut self, dst: u8, src: u8) {
        // F2 0F 5E /r - DIVSD xmm1, xmm2/m64
        self.buf.emit_u8(0xF2);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x5E);
        self.buf.emit_u8(Self::modrm(0b11, dst, src));
    }

    /// UCOMISD xmm1, xmm2 (compare scalar double-precision and set EFLAGS)
    pub fn ucomisd(&mut self, xmm1: u8, xmm2: u8) {
        // 66 0F 2E /r - UCOMISD xmm1, xmm2/m64
        self.buf.emit_u8(0x66);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x2E);
        self.buf.emit_u8(Self::modrm(0b11, xmm1, xmm2));
    }

    /// CVTSI2SD xmm, r64 (convert signed 64-bit integer to scalar double)
    pub fn cvtsi2sd_xmm_r64(&mut self, xmm: u8, src: Reg) {
        // F2 REX.W 0F 2A /r - CVTSI2SD xmm, r/m64
        self.buf.emit_u8(0xF2);
        let rex = 0x48 | src.rex_b();
        self.buf.emit_u8(rex);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x2A);
        self.buf.emit_u8(Self::modrm(0b11, xmm, src.code()));
    }

    /// CVTTSD2SI r64, xmm (convert scalar double to signed 64-bit integer, truncated)
    pub fn cvttsd2si_r64_xmm(&mut self, dst: Reg, xmm: u8) {
        // F2 REX.W 0F 2C /r - CVTTSD2SI r64, xmm/m64
        self.buf.emit_u8(0xF2);
        let rex = 0x48 | dst.rex_r();
        self.buf.emit_u8(rex);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x2C);
        self.buf.emit_u8(Self::modrm(0b11, dst.code(), xmm));
    }

    /// MOVSXD r64, r32 (sign-extend 32-bit to 64-bit)
    pub fn movsxd(&mut self, dst: Reg, src: Reg) {
        // REX.W 63 /r - MOVSXD r64, r/m32
        self.emit_rex_w(dst, src);
        self.buf.emit_u8(0x63);
        self.buf.emit_u8(Self::modrm(0b11, dst.code(), src.code()));
    }

    /// MOV r32, r32 (zero-extends to 64-bit)
    pub fn mov_r32_r32(&mut self, dst: Reg, src: Reg) {
        // No REX.W prefix — 32-bit mov zero-extends the upper 32 bits
        self.emit_rex_if_needed(src, dst);
        self.buf.emit_u8(0x89); // MOV r/m32, r32
        self.buf.emit_u8(Self::modrm(0b11, src.code(), dst.code()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mov_rr() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.mov_rr(Reg::Rax, Reg::Rbx);

        // MOV RAX, RBX = 48 89 D8
        assert_eq!(buf.code(), &[0x48, 0x89, 0xD8]);
    }

    #[test]
    fn test_mov_rr_r8_to_r9() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.mov_rr(Reg::R9, Reg::R8);

        // MOV R9, R8 = 4D 89 C1
        assert_eq!(buf.code(), &[0x4D, 0x89, 0xC1]);
    }

    #[test]
    fn test_mov_ri64() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.mov_ri64(Reg::Rax, 0x123456789ABCDEF0u64 as i64);

        // MOV RAX, imm64 = 48 B8 F0 DE BC 9A 78 56 34 12
        assert_eq!(
            buf.code(),
            &[0x48, 0xB8, 0xF0, 0xDE, 0xBC, 0x9A, 0x78, 0x56, 0x34, 0x12]
        );
    }

    #[test]
    fn test_mov_ri64_r15() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.mov_ri64(Reg::R15, 42);

        // MOV R15, 42 = 49 BF 2A 00 00 00 00 00 00 00
        assert_eq!(
            buf.code(),
            &[0x49, 0xBF, 0x2A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn test_ret() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.ret();

        assert_eq!(buf.code(), &[0xC3]);
    }

    #[test]
    fn test_push_pop() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.push(Reg::Rbx);
        asm.push(Reg::R12);
        asm.pop(Reg::R12);
        asm.pop(Reg::Rbx);

        // PUSH RBX = 53
        // PUSH R12 = 41 54
        // POP R12 = 41 5C
        // POP RBX = 5B
        assert_eq!(buf.code(), &[0x53, 0x41, 0x54, 0x41, 0x5C, 0x5B]);
    }

    #[test]
    fn test_mov_rm_simple() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.mov_rm(Reg::Rax, Reg::Rbx, 0);

        // MOV RAX, [RBX] = 48 8B 03
        assert_eq!(buf.code(), &[0x48, 0x8B, 0x03]);
    }

    #[test]
    fn test_mov_rm_disp8() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.mov_rm(Reg::Rax, Reg::Rbx, 16);

        // MOV RAX, [RBX+16] = 48 8B 43 10
        assert_eq!(buf.code(), &[0x48, 0x8B, 0x43, 0x10]);
    }

    #[test]
    fn test_mov_mr_simple() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.mov_mr(Reg::Rbx, 0, Reg::Rax);

        // MOV [RBX], RAX = 48 89 03
        assert_eq!(buf.code(), &[0x48, 0x89, 0x03]);
    }

    #[test]
    fn test_add_rr() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.add_rr(Reg::Rax, Reg::Rbx);

        // ADD RAX, RBX = 48 01 D8
        assert_eq!(buf.code(), &[0x48, 0x01, 0xD8]);
    }

    #[test]
    fn test_add_ri32_imm8() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.add_ri32(Reg::Rax, 16);

        // ADD RAX, 16 = 48 83 C0 10
        assert_eq!(buf.code(), &[0x48, 0x83, 0xC0, 0x10]);
    }

    #[test]
    fn test_add_ri32_imm32() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.add_ri32(Reg::Rax, 256);

        // ADD RAX, 256 = 48 81 C0 00 01 00 00
        assert_eq!(buf.code(), &[0x48, 0x81, 0xC0, 0x00, 0x01, 0x00, 0x00]);
    }

    #[test]
    fn test_sub_rr() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.sub_rr(Reg::Rax, Reg::Rbx);

        // SUB RAX, RBX = 48 29 D8
        assert_eq!(buf.code(), &[0x48, 0x29, 0xD8]);
    }

    #[test]
    fn test_sub_ri32_imm8() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.sub_ri32(Reg::Rsp, 32);

        // SUB RSP, 32 = 48 83 EC 20
        assert_eq!(buf.code(), &[0x48, 0x83, 0xEC, 0x20]);
    }

    #[test]
    fn test_imul_rr() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.imul_rr(Reg::Rax, Reg::Rbx);

        // IMUL RAX, RBX = 48 0F AF C3
        assert_eq!(buf.code(), &[0x48, 0x0F, 0xAF, 0xC3]);
    }

    #[test]
    fn test_idiv() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.idiv(Reg::Rcx);

        // IDIV RCX = 48 F7 F9
        assert_eq!(buf.code(), &[0x48, 0xF7, 0xF9]);
    }

    #[test]
    fn test_cqo() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.cqo();

        // CQO = 48 99
        assert_eq!(buf.code(), &[0x48, 0x99]);
    }

    #[test]
    fn test_cmp_rr() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.cmp_rr(Reg::Rax, Reg::Rbx);

        // CMP RAX, RBX = 48 39 D8
        assert_eq!(buf.code(), &[0x48, 0x39, 0xD8]);
    }

    #[test]
    fn test_cmp_ri32_imm8() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.cmp_ri32(Reg::Rax, 0);

        // CMP RAX, 0 = 48 83 F8 00
        assert_eq!(buf.code(), &[0x48, 0x83, 0xF8, 0x00]);
    }

    #[test]
    fn test_and_rr() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.and_rr(Reg::Rax, Reg::Rbx);

        // AND RAX, RBX = 48 21 D8
        assert_eq!(buf.code(), &[0x48, 0x21, 0xD8]);
    }

    #[test]
    fn test_or_rr() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.or_rr(Reg::Rax, Reg::Rbx);

        // OR RAX, RBX = 48 09 D8
        assert_eq!(buf.code(), &[0x48, 0x09, 0xD8]);
    }

    #[test]
    fn test_xor_rr() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.xor_rr(Reg::Rax, Reg::Rax);

        // XOR RAX, RAX = 48 31 C0
        assert_eq!(buf.code(), &[0x48, 0x31, 0xC0]);
    }

    #[test]
    fn test_neg() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.neg(Reg::Rax);

        // NEG RAX = 48 F7 D8
        assert_eq!(buf.code(), &[0x48, 0xF7, 0xD8]);
    }

    #[test]
    fn test_test_rr() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.test_rr(Reg::Rax, Reg::Rax);

        // TEST RAX, RAX = 48 85 C0
        assert_eq!(buf.code(), &[0x48, 0x85, 0xC0]);
    }

    #[test]
    fn test_jmp_rel32() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.jmp_rel32(0x10);

        // JMP +16 = E9 10 00 00 00
        assert_eq!(buf.code(), &[0xE9, 0x10, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_jmp_rel8() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.jmp_rel8(0x10);

        // JMP +16 = EB 10
        assert_eq!(buf.code(), &[0xEB, 0x10]);
    }

    #[test]
    fn test_jcc_rel32_je() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.jcc_rel32(Cond::E, 0x10);

        // JE +16 = 0F 84 10 00 00 00
        assert_eq!(buf.code(), &[0x0F, 0x84, 0x10, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_jcc_rel32_jne() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.jcc_rel32(Cond::Ne, 0x10);

        // JNE +16 = 0F 85 10 00 00 00
        assert_eq!(buf.code(), &[0x0F, 0x85, 0x10, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_jcc_rel32_jl() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.jcc_rel32(Cond::L, 0x10);

        // JL +16 = 0F 8C 10 00 00 00
        assert_eq!(buf.code(), &[0x0F, 0x8C, 0x10, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_jcc_rel32_jg() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.jcc_rel32(Cond::G, 0x10);

        // JG +16 = 0F 8F 10 00 00 00
        assert_eq!(buf.code(), &[0x0F, 0x8F, 0x10, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_jcc_rel8() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.jcc_rel8(Cond::E, 0x10);

        // JE +16 = 74 10
        assert_eq!(buf.code(), &[0x74, 0x10]);
    }

    #[test]
    fn test_call_rel32() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.call_rel32(0x10);

        // CALL +16 = E8 10 00 00 00
        assert_eq!(buf.code(), &[0xE8, 0x10, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_call_r() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.call_r(Reg::Rax);

        // CALL RAX = FF D0
        assert_eq!(buf.code(), &[0xFF, 0xD0]);
    }

    #[test]
    fn test_call_r_r12() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.call_r(Reg::R12);

        // CALL R12 = 41 FF D4
        assert_eq!(buf.code(), &[0x41, 0xFF, 0xD4]);
    }

    #[test]
    fn test_jmp_r() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.jmp_r(Reg::Rax);

        // JMP RAX = FF E0
        assert_eq!(buf.code(), &[0xFF, 0xE0]);
    }

    #[test]
    fn test_setcc_sete() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.setcc(Cond::E, Reg::Rax);

        // SETE AL = 0F 94 C0
        assert_eq!(buf.code(), &[0x0F, 0x94, 0xC0]);
    }

    #[test]
    fn test_setcc_setl() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.setcc(Cond::L, Reg::Rax);

        // SETL AL = 0F 9C C0
        assert_eq!(buf.code(), &[0x0F, 0x9C, 0xC0]);
    }

    #[test]
    fn test_movzx_r64_r8() {
        let mut buf = CodeBuffer::new();
        let mut asm = X86_64Assembler::new(&mut buf);
        asm.movzx_r64_r8(Reg::Rax, Reg::Rax);

        // MOVZX RAX, AL = 48 0F B6 C0
        assert_eq!(buf.code(), &[0x48, 0x0F, 0xB6, 0xC0]);
    }
}
