/// AArch64 instruction encoding for JIT compilation.
///
/// This module provides functions for encoding AArch64 instructions
/// as machine code bytes.

use super::codebuf::CodeBuffer;

/// AArch64 general-purpose registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Reg {
    X0 = 0, X1 = 1, X2 = 2, X3 = 3,
    X4 = 4, X5 = 5, X6 = 6, X7 = 7,
    X8 = 8, X9 = 9, X10 = 10, X11 = 11,
    X12 = 12, X13 = 13, X14 = 14, X15 = 15,
    X16 = 16, X17 = 17, X18 = 18, X19 = 19,
    X20 = 20, X21 = 21, X22 = 22, X23 = 23,
    X24 = 24, X25 = 25, X26 = 26, X27 = 27,
    X28 = 28,
    Fp = 29,  // Frame pointer
    Lr = 30,  // Link register
    Sp = 31,  // Stack pointer / Zero register (XZR in some contexts)
}

impl Reg {
    /// Alias for SP when used as zero register
    pub const XZR: Reg = Reg::Sp;

    pub fn code(self) -> u8 {
        self as u8
    }
}

/// AArch64 condition codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Cond {
    Eq = 0b0000,  // Equal
    Ne = 0b0001,  // Not equal
    Cs = 0b0010,  // Carry set / unsigned higher or same
    Cc = 0b0011,  // Carry clear / unsigned lower
    Mi = 0b0100,  // Minus / negative
    Pl = 0b0101,  // Plus / positive or zero
    Vs = 0b0110,  // Overflow set
    Vc = 0b0111,  // Overflow clear
    Hi = 0b1000,  // Unsigned higher
    Ls = 0b1001,  // Unsigned lower or same
    Ge = 0b1010,  // Signed greater than or equal
    Lt = 0b1011,  // Signed less than
    Gt = 0b1100,  // Signed greater than
    Le = 0b1101,  // Signed less than or equal
    Al = 0b1110,  // Always
}

/// AArch64 assembler.
pub struct AArch64Assembler<'a> {
    buf: &'a mut CodeBuffer,
}

impl<'a> AArch64Assembler<'a> {
    pub fn new(buf: &'a mut CodeBuffer) -> Self {
        Self { buf }
    }

    /// Emit a raw 32-bit instruction.
    pub fn emit_raw(&mut self, inst: u32) {
        self.buf.emit_u32(inst);
    }

    // ==================== Data Processing ====================

    /// ADD Xd, Xn, Xm (64-bit add)
    pub fn add(&mut self, rd: Reg, rn: Reg, rm: Reg) {
        // 1000 1011 000m mmmm 0000 00nn nnnd dddd
        let inst = 0x8B000000
            | ((rm.code() as u32) << 16)
            | ((rn.code() as u32) << 5)
            | (rd.code() as u32);
        self.emit_raw(inst);
    }

    /// ADD Xd, Xn, #imm12 (64-bit add immediate)
    pub fn add_imm(&mut self, rd: Reg, rn: Reg, imm12: u16) {
        // 1001 0001 00ii iiii iiii iinn nnnd dddd
        let inst = 0x91000000
            | (((imm12 as u32) & 0xFFF) << 10)
            | ((rn.code() as u32) << 5)
            | (rd.code() as u32);
        self.emit_raw(inst);
    }

    /// SUB Xd, Xn, Xm (64-bit subtract)
    pub fn sub(&mut self, rd: Reg, rn: Reg, rm: Reg) {
        // 1100 1011 000m mmmm 0000 00nn nnnd dddd
        let inst = 0xCB000000
            | ((rm.code() as u32) << 16)
            | ((rn.code() as u32) << 5)
            | (rd.code() as u32);
        self.emit_raw(inst);
    }

    /// SUB Xd, Xn, #imm12 (64-bit subtract immediate)
    pub fn sub_imm(&mut self, rd: Reg, rn: Reg, imm12: u16) {
        // 1101 0001 00ii iiii iiii iinn nnnd dddd
        let inst = 0xD1000000
            | (((imm12 as u32) & 0xFFF) << 10)
            | ((rn.code() as u32) << 5)
            | (rd.code() as u32);
        self.emit_raw(inst);
    }

    /// MUL Xd, Xn, Xm (64-bit multiply)
    pub fn mul(&mut self, rd: Reg, rn: Reg, rm: Reg) {
        // MADD Xd, Xn, Xm, XZR
        // 1001 1011 000m mmmm 0111 11nn nnnd dddd
        let inst = 0x9B007C00
            | ((rm.code() as u32) << 16)
            | ((rn.code() as u32) << 5)
            | (rd.code() as u32);
        self.emit_raw(inst);
    }

    /// SDIV Xd, Xn, Xm (64-bit signed divide)
    pub fn sdiv(&mut self, rd: Reg, rn: Reg, rm: Reg) {
        // 1001 1010 110m mmmm 0000 11nn nnnd dddd
        let inst = 0x9AC00C00
            | ((rm.code() as u32) << 16)
            | ((rn.code() as u32) << 5)
            | (rd.code() as u32);
        self.emit_raw(inst);
    }

    /// AND Xd, Xn, Xm
    pub fn and(&mut self, rd: Reg, rn: Reg, rm: Reg) {
        // 1000 1010 000m mmmm 0000 00nn nnnd dddd
        let inst = 0x8A000000
            | ((rm.code() as u32) << 16)
            | ((rn.code() as u32) << 5)
            | (rd.code() as u32);
        self.emit_raw(inst);
    }

    /// ORR Xd, Xn, Xm
    pub fn orr(&mut self, rd: Reg, rn: Reg, rm: Reg) {
        // 1010 1010 000m mmmm 0000 00nn nnnd dddd
        let inst = 0xAA000000
            | ((rm.code() as u32) << 16)
            | ((rn.code() as u32) << 5)
            | (rd.code() as u32);
        self.emit_raw(inst);
    }

    /// EOR Xd, Xn, Xm (XOR)
    pub fn eor(&mut self, rd: Reg, rn: Reg, rm: Reg) {
        // 1100 1010 000m mmmm 0000 00nn nnnd dddd
        let inst = 0xCA000000
            | ((rm.code() as u32) << 16)
            | ((rn.code() as u32) << 5)
            | (rd.code() as u32);
        self.emit_raw(inst);
    }

    /// MOV Xd, Xm (register to register)
    pub fn mov(&mut self, rd: Reg, rm: Reg) {
        // ORR Xd, XZR, Xm
        self.orr(rd, Reg::XZR, rm);
    }

    /// MOV Xd, #imm16 (move immediate)
    pub fn mov_imm(&mut self, rd: Reg, imm16: u16) {
        // MOVZ Xd, #imm16
        // 1101 0010 100i iiii iiii iiii iiid dddd
        let inst = 0xD2800000
            | ((imm16 as u32) << 5)
            | (rd.code() as u32);
        self.emit_raw(inst);
    }

    // ==================== Comparison ====================

    /// CMP Xn, Xm (compare registers)
    pub fn cmp(&mut self, rn: Reg, rm: Reg) {
        // SUBS XZR, Xn, Xm
        // 1110 1011 000m mmmm 0000 00nn nnnd dddd
        let inst = 0xEB000000
            | ((rm.code() as u32) << 16)
            | ((rn.code() as u32) << 5)
            | (Reg::XZR.code() as u32);
        self.emit_raw(inst);
    }

    /// CMP Xn, #imm12 (compare immediate)
    pub fn cmp_imm(&mut self, rn: Reg, imm12: u16) {
        // SUBS XZR, Xn, #imm12
        // 1111 0001 00ii iiii iiii iinn nnnd dddd
        let inst = 0xF1000000
            | (((imm12 as u32) & 0xFFF) << 10)
            | ((rn.code() as u32) << 5)
            | (Reg::XZR.code() as u32);
        self.emit_raw(inst);
    }

    // ==================== Loads and Stores ====================

    /// LDR Xt, [Xn, #imm12] (load 64-bit, unsigned offset)
    pub fn ldr(&mut self, rt: Reg, rn: Reg, imm12: u16) {
        // 1111 1001 01ii iiii iiii iinn nnnt tttt
        // imm12 is scaled by 8 (bytes)
        let scaled = (imm12 / 8) as u32;
        let inst = 0xF9400000
            | ((scaled & 0xFFF) << 10)
            | ((rn.code() as u32) << 5)
            | (rt.code() as u32);
        self.emit_raw(inst);
    }

    /// STR Xt, [Xn, #imm12] (store 64-bit, unsigned offset)
    pub fn str(&mut self, rt: Reg, rn: Reg, imm12: u16) {
        // 1111 1001 00ii iiii iiii iinn nnnt tttt
        let scaled = (imm12 / 8) as u32;
        let inst = 0xF9000000
            | ((scaled & 0xFFF) << 10)
            | ((rn.code() as u32) << 5)
            | (rt.code() as u32);
        self.emit_raw(inst);
    }

    /// LDR Xt, [Xn], #imm9 (load with post-increment)
    pub fn ldr_post(&mut self, rt: Reg, rn: Reg, imm9: i16) {
        // 1111 1000 010i iiii iiii 01nn nnnt tttt
        let inst = 0xF8400400
            | (((imm9 as u32) & 0x1FF) << 12)
            | ((rn.code() as u32) << 5)
            | (rt.code() as u32);
        self.emit_raw(inst);
    }

    /// STR Xt, [Xn, #imm9]! (store with pre-increment)
    pub fn str_pre(&mut self, rt: Reg, rn: Reg, imm9: i16) {
        // 1111 1000 000i iiii iiii 11nn nnnt tttt
        let inst = 0xF8000C00
            | (((imm9 as u32) & 0x1FF) << 12)
            | ((rn.code() as u32) << 5)
            | (rt.code() as u32);
        self.emit_raw(inst);
    }

    // ==================== Branches ====================

    /// B label (unconditional branch)
    pub fn b(&mut self, offset: i32) {
        // 0001 01ii iiii iiii iiii iiii iiii iiii
        let inst = 0x14000000 | ((offset as u32 / 4) & 0x03FFFFFF);
        self.emit_raw(inst);
    }

    /// BL label (branch and link)
    pub fn bl(&mut self, offset: i32) {
        // 1001 01ii iiii iiii iiii iiii iiii iiii
        let inst = 0x94000000 | ((offset as u32 / 4) & 0x03FFFFFF);
        self.emit_raw(inst);
    }

    /// B.cond label (conditional branch)
    pub fn b_cond(&mut self, cond: Cond, offset: i32) {
        // 0101 0100 iiii iiii iiii iiii iii0 cccc
        let inst = 0x54000000
            | (((offset as u32 / 4) & 0x7FFFF) << 5)
            | (cond as u32);
        self.emit_raw(inst);
    }

    /// CBZ Xn, label (compare and branch if zero)
    pub fn cbz(&mut self, rn: Reg, offset: i32) {
        // 1011 0100 iiii iiii iiii iiii iiit tttt
        let inst = 0xB4000000
            | (((offset as u32 / 4) & 0x7FFFF) << 5)
            | (rn.code() as u32);
        self.emit_raw(inst);
    }

    /// CBNZ Xn, label (compare and branch if not zero)
    pub fn cbnz(&mut self, rn: Reg, offset: i32) {
        // 1011 0101 iiii iiii iiii iiii iiit tttt
        let inst = 0xB5000000
            | (((offset as u32 / 4) & 0x7FFFF) << 5)
            | (rn.code() as u32);
        self.emit_raw(inst);
    }

    /// RET (return to link register)
    pub fn ret(&mut self) {
        // 1101 0110 0101 1111 0000 00nn nnn0 0000
        // Default: RET X30 (LR)
        let inst = 0xD65F03C0;
        self.emit_raw(inst);
    }

    /// BR Xn (branch to register)
    pub fn br(&mut self, rn: Reg) {
        // 1101 0110 0001 1111 0000 00nn nnn0 0000
        let inst = 0xD61F0000 | ((rn.code() as u32) << 5);
        self.emit_raw(inst);
    }

    /// BLR Xn (branch and link to register)
    pub fn blr(&mut self, rn: Reg) {
        // 1101 0110 0011 1111 0000 00nn nnn0 0000
        let inst = 0xD63F0000 | ((rn.code() as u32) << 5);
        self.emit_raw(inst);
    }

    // ==================== Stack operations ====================

    /// STP X1, X2, [SP, #imm]! (store pair with pre-index)
    pub fn stp_pre(&mut self, rt1: Reg, rt2: Reg, imm: i16) {
        // 1010 1001 11ii iiii it2t 2tnn nnnt 1t1t1
        let scaled = ((imm / 8) as u32) & 0x7F;
        let inst = 0xA9800000
            | (scaled << 15)
            | ((rt2.code() as u32) << 10)
            | ((Reg::Sp.code() as u32) << 5)
            | (rt1.code() as u32);
        self.emit_raw(inst);
    }

    /// LDP X1, X2, [SP], #imm (load pair with post-index)
    pub fn ldp_post(&mut self, rt1: Reg, rt2: Reg, imm: i16) {
        // 1010 1000 11ii iiii it2t 2tnn nnnt 1t1t1
        let scaled = ((imm / 8) as u32) & 0x7F;
        let inst = 0xA8C00000
            | (scaled << 15)
            | ((rt2.code() as u32) << 10)
            | ((Reg::Sp.code() as u32) << 5)
            | (rt1.code() as u32);
        self.emit_raw(inst);
    }

    // ==================== NOP ====================

    /// NOP (no operation)
    pub fn nop(&mut self) {
        // 1101 0101 0000 0011 0010 0000 0001 1111
        self.emit_raw(0xD503201F);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        let mut buf = CodeBuffer::new();
        let mut asm = AArch64Assembler::new(&mut buf);
        asm.add(Reg::X0, Reg::X1, Reg::X2);

        // ADD X0, X1, X2 should be 0x8B020020
        assert_eq!(buf.code(), &[0x20, 0x00, 0x02, 0x8B]);
    }

    #[test]
    fn test_mov_imm() {
        let mut buf = CodeBuffer::new();
        let mut asm = AArch64Assembler::new(&mut buf);
        asm.mov_imm(Reg::X0, 42);

        // MOVZ X0, #42 should encode 42 in bits 20:5
        let inst = u32::from_le_bytes([buf.code()[0], buf.code()[1], buf.code()[2], buf.code()[3]]);
        assert_eq!(inst & 0x1F, 0); // X0
        assert_eq!((inst >> 5) & 0xFFFF, 42); // imm16
    }

    #[test]
    fn test_ret() {
        let mut buf = CodeBuffer::new();
        let mut asm = AArch64Assembler::new(&mut buf);
        asm.ret();

        // RET should be 0xD65F03C0
        assert_eq!(buf.code(), &[0xC0, 0x03, 0x5F, 0xD6]);
    }
}
