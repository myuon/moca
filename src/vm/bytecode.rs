//! Bytecode serialization/deserialization for moca.
//!
//! Binary format:
//! - Magic: "MOCA" (4 bytes)
//! - Version: u32 (little-endian)
//! - String pool: length + strings
//! - Functions: count + function data
//! - Main function
//! - Debug info (optional)

use super::stackmap::{FunctionStackMap, RefBitset, StackMapEntry};
use super::{Chunk, Function, Op};
use std::io::{self, Read, Write};

/// Magic bytes for moca bytecode files
pub const MAGIC: &[u8; 4] = b"MOCA";

/// Current bytecode format version
pub const VERSION: u32 = 1;

/// Error type for bytecode operations
#[derive(Debug)]
pub enum BytecodeError {
    /// Invalid magic number
    InvalidMagic,
    /// Unsupported version
    UnsupportedVersion(u32),
    /// Truncated data
    UnexpectedEof,
    /// Invalid opcode
    InvalidOpcode(u8),
    /// I/O error
    Io(io::Error),
    /// Invalid UTF-8 in string
    InvalidUtf8,
}

impl From<io::Error> for BytecodeError {
    fn from(e: io::Error) -> Self {
        BytecodeError::Io(e)
    }
}

impl std::fmt::Display for BytecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BytecodeError::InvalidMagic => write!(f, "invalid magic number"),
            BytecodeError::UnsupportedVersion(v) => write!(f, "unsupported version: {}", v),
            BytecodeError::UnexpectedEof => write!(f, "unexpected end of file"),
            BytecodeError::InvalidOpcode(op) => write!(f, "invalid opcode: {}", op),
            BytecodeError::Io(e) => write!(f, "I/O error: {}", e),
            BytecodeError::InvalidUtf8 => write!(f, "invalid UTF-8 string"),
        }
    }
}

impl std::error::Error for BytecodeError {}

/// Serialize a Chunk to bytes
pub fn serialize(chunk: &Chunk) -> Vec<u8> {
    let mut buf = Vec::new();
    write_chunk(&mut buf, chunk).expect("writing to Vec cannot fail");
    buf
}

/// Deserialize a Chunk from bytes
pub fn deserialize(data: &[u8]) -> Result<Chunk, BytecodeError> {
    let mut cursor = std::io::Cursor::new(data);
    read_chunk(&mut cursor)
}

/// Write a Chunk to a writer
pub fn write_chunk<W: Write>(w: &mut W, chunk: &Chunk) -> io::Result<()> {
    // Magic
    w.write_all(MAGIC)?;

    // Version
    w.write_all(&VERSION.to_le_bytes())?;

    // String pool
    write_u32(w, chunk.strings.len() as u32)?;
    for s in &chunk.strings {
        write_string(w, s)?;
    }

    // Functions
    write_u32(w, chunk.functions.len() as u32)?;
    for func in &chunk.functions {
        write_function(w, func)?;
    }

    // Main function
    write_function(w, &chunk.main)?;

    // Debug info (not serialized for now)
    w.write_all(&[0u8])?; // has_debug = false

    Ok(())
}

/// Read a Chunk from a reader
pub fn read_chunk<R: Read>(r: &mut R) -> Result<Chunk, BytecodeError> {
    // Magic
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)
        .map_err(|_| BytecodeError::UnexpectedEof)?;
    if &magic != MAGIC {
        return Err(BytecodeError::InvalidMagic);
    }

    // Version
    let version = read_u32(r)?;
    if version != VERSION {
        return Err(BytecodeError::UnsupportedVersion(version));
    }

    // String pool
    let string_count = read_u32(r)? as usize;
    let mut strings = Vec::with_capacity(string_count);
    for _ in 0..string_count {
        strings.push(read_string(r)?);
    }

    // Functions
    let func_count = read_u32(r)? as usize;
    let mut functions = Vec::with_capacity(func_count);
    for _ in 0..func_count {
        functions.push(read_function(r)?);
    }

    // Main function
    let main = read_function(r)?;

    // Debug info
    let has_debug = read_u8(r)?;
    let debug = if has_debug != 0 {
        // TODO: Implement debug info deserialization
        None
    } else {
        None
    };

    Ok(Chunk {
        functions,
        main,
        strings,
        debug,
    })
}

fn write_function<W: Write>(w: &mut W, func: &Function) -> io::Result<()> {
    write_string(w, &func.name)?;
    write_u32(w, func.arity as u32)?;
    write_u32(w, func.locals_count as u32)?;

    // Code
    write_u32(w, func.code.len() as u32)?;
    for op in &func.code {
        write_op(w, op)?;
    }

    // StackMap
    if let Some(ref stackmap) = func.stackmap {
        w.write_all(&[1u8])?;
        write_stackmap(w, stackmap)?;
    } else {
        w.write_all(&[0u8])?;
    }

    Ok(())
}

fn read_function<R: Read>(r: &mut R) -> Result<Function, BytecodeError> {
    let name = read_string(r)?;
    let arity = read_u32(r)? as usize;
    let locals_count = read_u32(r)? as usize;

    // Code
    let code_len = read_u32(r)? as usize;
    let mut code = Vec::with_capacity(code_len);
    for _ in 0..code_len {
        code.push(read_op(r)?);
    }

    // StackMap
    let has_stackmap = read_u8(r)?;
    let stackmap = if has_stackmap != 0 {
        Some(read_stackmap(r)?)
    } else {
        None
    };

    Ok(Function {
        name,
        arity,
        locals_count,
        code,
        stackmap,
    })
}

fn write_stackmap<W: Write>(w: &mut W, stackmap: &FunctionStackMap) -> io::Result<()> {
    write_u32(w, stackmap.len() as u32)?;
    for entry in stackmap.entries() {
        write_u32(w, entry.pc)?;
        write_u16(w, entry.stack_height)?;
        write_u64(w, entry.stack_ref_bits.bits())?;
        write_u64(w, entry.locals_ref_bits.bits())?;
    }
    Ok(())
}

fn read_stackmap<R: Read>(r: &mut R) -> Result<FunctionStackMap, BytecodeError> {
    let entry_count = read_u32(r)? as usize;
    let mut fsm = FunctionStackMap::new();
    for _ in 0..entry_count {
        let pc = read_u32(r)?;
        let stack_height = read_u16(r)?;
        let stack_ref_bits = RefBitset::from_bits(read_u64(r)?);
        let locals_ref_bits = RefBitset::from_bits(read_u64(r)?);
        let mut entry = StackMapEntry::new(pc, stack_height);
        // Set the ref bits by accessing the fields
        // We need to reconstruct the entry with the bits set
        for i in 0..64 {
            if stack_ref_bits.is_set(i) {
                entry.mark_stack_ref(i);
            }
            if locals_ref_bits.is_set(i) {
                entry.mark_local_ref(i);
            }
        }
        fsm.add_entry(entry);
    }
    Ok(fsm)
}

// Opcode tags
const OP_PUSH_INT: u8 = 0;
const OP_PUSH_FLOAT: u8 = 1;
const OP_PUSH_TRUE: u8 = 2;
const OP_PUSH_FALSE: u8 = 3;
const OP_PUSH_NULL: u8 = 4;
const OP_PUSH_STRING: u8 = 5;
const OP_POP: u8 = 6;
const OP_DUP: u8 = 7;
const OP_GET_L: u8 = 8;
const OP_SET_L: u8 = 9;
const OP_ADD: u8 = 10;
const OP_SUB: u8 = 11;
const OP_MUL: u8 = 12;
const OP_DIV: u8 = 13;
const OP_MOD: u8 = 14;
const OP_NEG: u8 = 15;
const OP_ADD_I64: u8 = 16;
const OP_SUB_I64: u8 = 17;
const OP_MUL_I64: u8 = 18;
const OP_DIV_I64: u8 = 19;
const OP_ADD_F64: u8 = 20;
const OP_SUB_F64: u8 = 21;
const OP_MUL_F64: u8 = 22;
const OP_DIV_F64: u8 = 23;
const OP_EQ: u8 = 24;
const OP_NE: u8 = 25;
const OP_LT: u8 = 26;
const OP_LE: u8 = 27;
const OP_GT: u8 = 28;
const OP_GE: u8 = 29;
const OP_LT_I64: u8 = 30;
const OP_LE_I64: u8 = 31;
const OP_GT_I64: u8 = 32;
const OP_GE_I64: u8 = 33;
const OP_LT_F64: u8 = 34;
const OP_NOT: u8 = 35;
const OP_JMP: u8 = 36;
const OP_JMP_IF_FALSE: u8 = 37;
const OP_JMP_IF_TRUE: u8 = 38;
const OP_CALL: u8 = 39;
const OP_RET: u8 = 40;
const OP_NEW: u8 = 41;
const OP_GET_F: u8 = 42;
const OP_SET_F: u8 = 43;
const OP_GET_F_CACHED: u8 = 44;
const OP_SET_F_CACHED: u8 = 45;
const OP_ALLOC_ARRAY: u8 = 46;
const OP_ARRAY_LEN: u8 = 47;
const OP_ARRAY_GET: u8 = 48;
const OP_ARRAY_SET: u8 = 49;
const OP_ARRAY_PUSH: u8 = 50;
const OP_ARRAY_POP: u8 = 51;
const OP_ARRAY_GET_INT: u8 = 52;
const OP_STRING_LEN: u8 = 53;
const OP_STRING_CONCAT: u8 = 54;
const OP_TYPE_OF: u8 = 55;
const OP_TO_STRING: u8 = 56;
const OP_PARSE_INT: u8 = 57;
const OP_THROW: u8 = 58;
const OP_TRY_BEGIN: u8 = 59;
const OP_TRY_END: u8 = 60;
const OP_PRINT: u8 = 61;
const OP_GC_HINT: u8 = 62;
const OP_THREAD_SPAWN: u8 = 63;
const OP_CHANNEL_CREATE: u8 = 64;
const OP_CHANNEL_SEND: u8 = 65;
const OP_CHANNEL_RECV: u8 = 66;
const OP_THREAD_JOIN: u8 = 67;

fn write_op<W: Write>(w: &mut W, op: &Op) -> io::Result<()> {
    match op {
        Op::PushInt(v) => {
            w.write_all(&[OP_PUSH_INT])?;
            write_i64(w, *v)?;
        }
        Op::PushFloat(v) => {
            w.write_all(&[OP_PUSH_FLOAT])?;
            write_f64(w, *v)?;
        }
        Op::PushTrue => w.write_all(&[OP_PUSH_TRUE])?,
        Op::PushFalse => w.write_all(&[OP_PUSH_FALSE])?,
        Op::PushNull => w.write_all(&[OP_PUSH_NULL])?,
        Op::PushString(idx) => {
            w.write_all(&[OP_PUSH_STRING])?;
            write_u32(w, *idx as u32)?;
        }
        Op::Pop => w.write_all(&[OP_POP])?,
        Op::Dup => w.write_all(&[OP_DUP])?,
        Op::GetL(idx) => {
            w.write_all(&[OP_GET_L])?;
            write_u32(w, *idx as u32)?;
        }
        Op::SetL(idx) => {
            w.write_all(&[OP_SET_L])?;
            write_u32(w, *idx as u32)?;
        }
        Op::Add => w.write_all(&[OP_ADD])?,
        Op::Sub => w.write_all(&[OP_SUB])?,
        Op::Mul => w.write_all(&[OP_MUL])?,
        Op::Div => w.write_all(&[OP_DIV])?,
        Op::Mod => w.write_all(&[OP_MOD])?,
        Op::Neg => w.write_all(&[OP_NEG])?,
        Op::AddI64 => w.write_all(&[OP_ADD_I64])?,
        Op::SubI64 => w.write_all(&[OP_SUB_I64])?,
        Op::MulI64 => w.write_all(&[OP_MUL_I64])?,
        Op::DivI64 => w.write_all(&[OP_DIV_I64])?,
        Op::AddF64 => w.write_all(&[OP_ADD_F64])?,
        Op::SubF64 => w.write_all(&[OP_SUB_F64])?,
        Op::MulF64 => w.write_all(&[OP_MUL_F64])?,
        Op::DivF64 => w.write_all(&[OP_DIV_F64])?,
        Op::Eq => w.write_all(&[OP_EQ])?,
        Op::Ne => w.write_all(&[OP_NE])?,
        Op::Lt => w.write_all(&[OP_LT])?,
        Op::Le => w.write_all(&[OP_LE])?,
        Op::Gt => w.write_all(&[OP_GT])?,
        Op::Ge => w.write_all(&[OP_GE])?,
        Op::LtI64 => w.write_all(&[OP_LT_I64])?,
        Op::LeI64 => w.write_all(&[OP_LE_I64])?,
        Op::GtI64 => w.write_all(&[OP_GT_I64])?,
        Op::GeI64 => w.write_all(&[OP_GE_I64])?,
        Op::LtF64 => w.write_all(&[OP_LT_F64])?,
        Op::Not => w.write_all(&[OP_NOT])?,
        Op::Jmp(target) => {
            w.write_all(&[OP_JMP])?;
            write_u32(w, *target as u32)?;
        }
        Op::JmpIfFalse(target) => {
            w.write_all(&[OP_JMP_IF_FALSE])?;
            write_u32(w, *target as u32)?;
        }
        Op::JmpIfTrue(target) => {
            w.write_all(&[OP_JMP_IF_TRUE])?;
            write_u32(w, *target as u32)?;
        }
        Op::Call(func_idx, argc) => {
            w.write_all(&[OP_CALL])?;
            write_u32(w, *func_idx as u32)?;
            write_u32(w, *argc as u32)?;
        }
        Op::Ret => w.write_all(&[OP_RET])?,
        Op::New(size) => {
            w.write_all(&[OP_NEW])?;
            write_u32(w, *size as u32)?;
        }
        Op::GetF(idx) => {
            w.write_all(&[OP_GET_F])?;
            write_u32(w, *idx as u32)?;
        }
        Op::SetF(idx) => {
            w.write_all(&[OP_SET_F])?;
            write_u32(w, *idx as u32)?;
        }
        Op::GetFCached(idx, offset) => {
            w.write_all(&[OP_GET_F_CACHED])?;
            write_u32(w, *idx as u32)?;
            write_u16(w, *offset)?;
        }
        Op::SetFCached(idx, offset) => {
            w.write_all(&[OP_SET_F_CACHED])?;
            write_u32(w, *idx as u32)?;
            write_u16(w, *offset)?;
        }
        Op::AllocArray(size) => {
            w.write_all(&[OP_ALLOC_ARRAY])?;
            write_u32(w, *size as u32)?;
        }
        Op::ArrayLen => w.write_all(&[OP_ARRAY_LEN])?,
        Op::ArrayGet => w.write_all(&[OP_ARRAY_GET])?,
        Op::ArraySet => w.write_all(&[OP_ARRAY_SET])?,
        Op::ArrayPush => w.write_all(&[OP_ARRAY_PUSH])?,
        Op::ArrayPop => w.write_all(&[OP_ARRAY_POP])?,
        Op::ArrayGetInt => w.write_all(&[OP_ARRAY_GET_INT])?,
        Op::StringLen => w.write_all(&[OP_STRING_LEN])?,
        Op::StringConcat => w.write_all(&[OP_STRING_CONCAT])?,
        Op::TypeOf => w.write_all(&[OP_TYPE_OF])?,
        Op::ToString => w.write_all(&[OP_TO_STRING])?,
        Op::ParseInt => w.write_all(&[OP_PARSE_INT])?,
        Op::Throw => w.write_all(&[OP_THROW])?,
        Op::TryBegin(target) => {
            w.write_all(&[OP_TRY_BEGIN])?;
            write_u32(w, *target as u32)?;
        }
        Op::TryEnd => w.write_all(&[OP_TRY_END])?,
        Op::Print => w.write_all(&[OP_PRINT])?,
        Op::GcHint(size) => {
            w.write_all(&[OP_GC_HINT])?;
            write_u32(w, *size as u32)?;
        }
        Op::ThreadSpawn(func_idx) => {
            w.write_all(&[OP_THREAD_SPAWN])?;
            write_u32(w, *func_idx as u32)?;
        }
        Op::ChannelCreate => w.write_all(&[OP_CHANNEL_CREATE])?,
        Op::ChannelSend => w.write_all(&[OP_CHANNEL_SEND])?,
        Op::ChannelRecv => w.write_all(&[OP_CHANNEL_RECV])?,
        Op::ThreadJoin => w.write_all(&[OP_THREAD_JOIN])?,
    }
    Ok(())
}

fn read_op<R: Read>(r: &mut R) -> Result<Op, BytecodeError> {
    let tag = read_u8(r)?;
    let op = match tag {
        OP_PUSH_INT => Op::PushInt(read_i64(r)?),
        OP_PUSH_FLOAT => Op::PushFloat(read_f64(r)?),
        OP_PUSH_TRUE => Op::PushTrue,
        OP_PUSH_FALSE => Op::PushFalse,
        OP_PUSH_NULL => Op::PushNull,
        OP_PUSH_STRING => Op::PushString(read_u32(r)? as usize),
        OP_POP => Op::Pop,
        OP_DUP => Op::Dup,
        OP_GET_L => Op::GetL(read_u32(r)? as usize),
        OP_SET_L => Op::SetL(read_u32(r)? as usize),
        OP_ADD => Op::Add,
        OP_SUB => Op::Sub,
        OP_MUL => Op::Mul,
        OP_DIV => Op::Div,
        OP_MOD => Op::Mod,
        OP_NEG => Op::Neg,
        OP_ADD_I64 => Op::AddI64,
        OP_SUB_I64 => Op::SubI64,
        OP_MUL_I64 => Op::MulI64,
        OP_DIV_I64 => Op::DivI64,
        OP_ADD_F64 => Op::AddF64,
        OP_SUB_F64 => Op::SubF64,
        OP_MUL_F64 => Op::MulF64,
        OP_DIV_F64 => Op::DivF64,
        OP_EQ => Op::Eq,
        OP_NE => Op::Ne,
        OP_LT => Op::Lt,
        OP_LE => Op::Le,
        OP_GT => Op::Gt,
        OP_GE => Op::Ge,
        OP_LT_I64 => Op::LtI64,
        OP_LE_I64 => Op::LeI64,
        OP_GT_I64 => Op::GtI64,
        OP_GE_I64 => Op::GeI64,
        OP_LT_F64 => Op::LtF64,
        OP_NOT => Op::Not,
        OP_JMP => Op::Jmp(read_u32(r)? as usize),
        OP_JMP_IF_FALSE => Op::JmpIfFalse(read_u32(r)? as usize),
        OP_JMP_IF_TRUE => Op::JmpIfTrue(read_u32(r)? as usize),
        OP_CALL => {
            let func_idx = read_u32(r)? as usize;
            let argc = read_u32(r)? as usize;
            Op::Call(func_idx, argc)
        }
        OP_RET => Op::Ret,
        OP_NEW => Op::New(read_u32(r)? as usize),
        OP_GET_F => Op::GetF(read_u32(r)? as usize),
        OP_SET_F => Op::SetF(read_u32(r)? as usize),
        OP_GET_F_CACHED => {
            let idx = read_u32(r)? as usize;
            let offset = read_u16(r)?;
            Op::GetFCached(idx, offset)
        }
        OP_SET_F_CACHED => {
            let idx = read_u32(r)? as usize;
            let offset = read_u16(r)?;
            Op::SetFCached(idx, offset)
        }
        OP_ALLOC_ARRAY => Op::AllocArray(read_u32(r)? as usize),
        OP_ARRAY_LEN => Op::ArrayLen,
        OP_ARRAY_GET => Op::ArrayGet,
        OP_ARRAY_SET => Op::ArraySet,
        OP_ARRAY_PUSH => Op::ArrayPush,
        OP_ARRAY_POP => Op::ArrayPop,
        OP_ARRAY_GET_INT => Op::ArrayGetInt,
        OP_STRING_LEN => Op::StringLen,
        OP_STRING_CONCAT => Op::StringConcat,
        OP_TYPE_OF => Op::TypeOf,
        OP_TO_STRING => Op::ToString,
        OP_PARSE_INT => Op::ParseInt,
        OP_THROW => Op::Throw,
        OP_TRY_BEGIN => Op::TryBegin(read_u32(r)? as usize),
        OP_TRY_END => Op::TryEnd,
        OP_PRINT => Op::Print,
        OP_GC_HINT => Op::GcHint(read_u32(r)? as usize),
        OP_THREAD_SPAWN => Op::ThreadSpawn(read_u32(r)? as usize),
        OP_CHANNEL_CREATE => Op::ChannelCreate,
        OP_CHANNEL_SEND => Op::ChannelSend,
        OP_CHANNEL_RECV => Op::ChannelRecv,
        OP_THREAD_JOIN => Op::ThreadJoin,
        _ => return Err(BytecodeError::InvalidOpcode(tag)),
    };
    Ok(op)
}

// Helper functions for reading/writing primitives

fn write_u8<W: Write>(w: &mut W, v: u8) -> io::Result<()> {
    w.write_all(&[v])
}

fn read_u8<R: Read>(r: &mut R) -> Result<u8, BytecodeError> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)
        .map_err(|_| BytecodeError::UnexpectedEof)?;
    Ok(buf[0])
}

fn write_u16<W: Write>(w: &mut W, v: u16) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn read_u16<R: Read>(r: &mut R) -> Result<u16, BytecodeError> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)
        .map_err(|_| BytecodeError::UnexpectedEof)?;
    Ok(u16::from_le_bytes(buf))
}

fn write_u32<W: Write>(w: &mut W, v: u32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn read_u32<R: Read>(r: &mut R) -> Result<u32, BytecodeError> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)
        .map_err(|_| BytecodeError::UnexpectedEof)?;
    Ok(u32::from_le_bytes(buf))
}

fn write_u64<W: Write>(w: &mut W, v: u64) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn read_u64<R: Read>(r: &mut R) -> Result<u64, BytecodeError> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)
        .map_err(|_| BytecodeError::UnexpectedEof)?;
    Ok(u64::from_le_bytes(buf))
}

fn write_i64<W: Write>(w: &mut W, v: i64) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn read_i64<R: Read>(r: &mut R) -> Result<i64, BytecodeError> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)
        .map_err(|_| BytecodeError::UnexpectedEof)?;
    Ok(i64::from_le_bytes(buf))
}

fn write_f64<W: Write>(w: &mut W, v: f64) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn read_f64<R: Read>(r: &mut R) -> Result<f64, BytecodeError> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)
        .map_err(|_| BytecodeError::UnexpectedEof)?;
    Ok(f64::from_le_bytes(buf))
}

fn write_string<W: Write>(w: &mut W, s: &str) -> io::Result<()> {
    write_u32(w, s.len() as u32)?;
    w.write_all(s.as_bytes())
}

fn read_string<R: Read>(r: &mut R) -> Result<String, BytecodeError> {
    let len = read_u32(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)
        .map_err(|_| BytecodeError::UnexpectedEof)?;
    String::from_utf8(buf).map_err(|_| BytecodeError::InvalidUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_simple() {
        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "main".to_string(),
                arity: 0,
                locals_count: 2,
                code: vec![
                    Op::PushInt(42),
                    Op::PushFloat(3.14),
                    Op::PushTrue,
                    Op::PushFalse,
                    Op::PushNull,
                    Op::Add,
                    Op::Ret,
                ],
                stackmap: None,
            },
            strings: vec!["hello".to_string(), "world".to_string()],
            debug: None,
        };

        let bytes = serialize(&chunk);
        let restored = deserialize(&bytes).unwrap();

        assert_eq!(restored.main.name, chunk.main.name);
        assert_eq!(restored.main.arity, chunk.main.arity);
        assert_eq!(restored.main.locals_count, chunk.main.locals_count);
        assert_eq!(restored.main.code.len(), chunk.main.code.len());
        assert_eq!(restored.strings, chunk.strings);
    }

    #[test]
    fn test_roundtrip_with_functions() {
        let chunk = Chunk {
            functions: vec![Function {
                name: "add".to_string(),
                arity: 2,
                locals_count: 2,
                code: vec![Op::GetL(0), Op::GetL(1), Op::AddI64, Op::Ret],
                stackmap: None,
            }],
            main: Function {
                name: "main".to_string(),
                arity: 0,
                locals_count: 0,
                code: vec![
                    Op::PushInt(10),
                    Op::PushInt(20),
                    Op::Call(0, 2),
                    Op::Print,
                    Op::Ret,
                ],
                stackmap: None,
            },
            strings: vec![],
            debug: None,
        };

        let bytes = serialize(&chunk);
        let restored = deserialize(&bytes).unwrap();

        assert_eq!(restored.functions.len(), 1);
        assert_eq!(restored.functions[0].name, "add");
        assert_eq!(restored.functions[0].arity, 2);
    }

    #[test]
    fn test_roundtrip_with_stackmap() {
        // Create a stackmap with one entry
        let mut stackmap = FunctionStackMap::new();
        let mut entry = StackMapEntry::new(0, 1);
        entry.mark_stack_ref(0); // Mark first stack slot as reference
        entry.mark_local_ref(2); // Mark local 2 as reference
        stackmap.add_entry(entry);

        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "main".to_string(),
                arity: 0,
                locals_count: 4,
                code: vec![Op::New(2), Op::Ret],
                stackmap: Some(stackmap),
            },
            strings: vec![],
            debug: None,
        };

        let bytes = serialize(&chunk);
        let restored = deserialize(&bytes).unwrap();

        assert!(restored.main.stackmap.is_some());
        let sm = restored.main.stackmap.unwrap();
        assert_eq!(sm.len(), 1);
        let restored_entry = sm.get(0).unwrap();
        assert_eq!(restored_entry.pc, 0);
        assert_eq!(restored_entry.stack_height, 1);
        assert!(restored_entry.is_stack_ref(0));
        assert!(!restored_entry.is_stack_ref(1));
        assert!(restored_entry.is_local_ref(2));
    }

    #[test]
    fn test_invalid_magic() {
        let data = b"BADM\x01\x00\x00\x00";
        let result = deserialize(data);
        assert!(matches!(result, Err(BytecodeError::InvalidMagic)));
    }

    #[test]
    fn test_unsupported_version() {
        let data = b"MOCA\xFF\x00\x00\x00";
        let result = deserialize(data);
        assert!(matches!(
            result,
            Err(BytecodeError::UnsupportedVersion(255))
        ));
    }

    #[test]
    fn test_all_opcodes() {
        // Test that all opcodes roundtrip correctly
        let ops = vec![
            Op::PushInt(i64::MAX),
            Op::PushInt(i64::MIN),
            Op::PushFloat(std::f64::consts::PI),
            Op::PushTrue,
            Op::PushFalse,
            Op::PushNull,
            Op::PushString(42),
            Op::Pop,
            Op::Dup,
            Op::GetL(100),
            Op::SetL(200),
            Op::Add,
            Op::Sub,
            Op::Mul,
            Op::Div,
            Op::Mod,
            Op::Neg,
            Op::AddI64,
            Op::SubI64,
            Op::MulI64,
            Op::DivI64,
            Op::AddF64,
            Op::SubF64,
            Op::MulF64,
            Op::DivF64,
            Op::Eq,
            Op::Ne,
            Op::Lt,
            Op::Le,
            Op::Gt,
            Op::Ge,
            Op::LtI64,
            Op::LeI64,
            Op::GtI64,
            Op::GeI64,
            Op::LtF64,
            Op::Not,
            Op::Jmp(1000),
            Op::JmpIfFalse(2000),
            Op::JmpIfTrue(3000),
            Op::Call(5, 3),
            Op::Ret,
            Op::New(10),
            Op::GetF(1),
            Op::SetF(2),
            Op::GetFCached(3, 4),
            Op::SetFCached(5, 6),
            Op::AllocArray(7),
            Op::ArrayLen,
            Op::ArrayGet,
            Op::ArraySet,
            Op::ArrayPush,
            Op::ArrayPop,
            Op::ArrayGetInt,
            Op::StringLen,
            Op::StringConcat,
            Op::TypeOf,
            Op::ToString,
            Op::ParseInt,
            Op::Throw,
            Op::TryBegin(100),
            Op::TryEnd,
            Op::Print,
            Op::GcHint(1024),
            Op::ThreadSpawn(1),
            Op::ChannelCreate,
            Op::ChannelSend,
            Op::ChannelRecv,
            Op::ThreadJoin,
        ];

        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "test".to_string(),
                arity: 0,
                locals_count: 0,
                code: ops.clone(),
                stackmap: None,
            },
            strings: vec![],
            debug: None,
        };

        let bytes = serialize(&chunk);
        let restored = deserialize(&bytes).unwrap();

        assert_eq!(restored.main.code.len(), ops.len());
        for (orig, rest) in ops.iter().zip(restored.main.code.iter()) {
            assert_eq!(orig, rest);
        }
    }
}
