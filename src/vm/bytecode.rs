//! Bytecode serialization/deserialization for moca.
//!
//! Binary format:
//! - Magic: "MOCA" (4 bytes)
//! - Version: u32 (little-endian)
//! - String pool: length + strings
//! - Functions: count + function data
//! - Main function
//! - Debug info (optional)

use super::heap::ElemKind;
use super::stackmap::{FunctionStackMap, RefBitset, StackMapEntry};
use super::{Chunk, Function, Op, ValueType};
use std::io::{self, Read, Write};

/// Magic bytes for moca bytecode files
pub const MAGIC: &[u8; 4] = b"MOCA";

/// Current bytecode format version
pub const VERSION: u32 = 2;

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
    /// Invalid value type tag
    InvalidValueType(u8),
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
            BytecodeError::InvalidValueType(t) => write!(f, "invalid value type tag: {}", t),
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

    // Type descriptors
    write_u32(w, chunk.type_descriptors.len() as u32)?;
    for td in &chunk.type_descriptors {
        write_string(w, &td.tag_name)?;
        write_u32(w, td.field_names.len() as u32)?;
        for field_name in &td.field_names {
            write_string(w, field_name)?;
        }
        write_u32(w, td.field_type_tags.len() as u32)?;
        for field_type_tag in &td.field_type_tags {
            write_string(w, field_type_tag)?;
        }
        write_u32(w, td.aux_type_tags.len() as u32)?;
        for aux_type_tag in &td.aux_type_tags {
            write_string(w, aux_type_tag)?;
        }
        // vtables: Vec<(iface_idx, Vec<func_idx>)>
        write_u32(w, td.vtables.len() as u32)?;
        for (iface_idx, func_indices) in &td.vtables {
            write_u32(w, *iface_idx as u32)?;
            write_u32(w, func_indices.len() as u32)?;
            for fi in func_indices {
                write_u32(w, *fi as u32)?;
            }
        }
    }

    // Interface descriptors
    write_u32(w, chunk.interface_descriptors.len() as u32)?;
    for id in &chunk.interface_descriptors {
        write_string(w, &id.name)?;
        write_u32(w, id.method_names.len() as u32)?;
        for mn in &id.method_names {
            write_string(w, mn)?;
        }
    }

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

    // Type descriptors
    let td_count = read_u32(r)? as usize;
    let mut type_descriptors = Vec::with_capacity(td_count);
    for _ in 0..td_count {
        let tag_name = read_string(r)?;
        let field_count = read_u32(r)? as usize;
        let mut field_names = Vec::with_capacity(field_count);
        for _ in 0..field_count {
            field_names.push(read_string(r)?);
        }
        let field_type_count = read_u32(r)? as usize;
        let mut field_type_tags = Vec::with_capacity(field_type_count);
        for _ in 0..field_type_count {
            field_type_tags.push(read_string(r)?);
        }
        let aux_type_count = read_u32(r)? as usize;
        let mut aux_type_tags = Vec::with_capacity(aux_type_count);
        for _ in 0..aux_type_count {
            aux_type_tags.push(read_string(r)?);
        }
        let vtable_count = read_u32(r)? as usize;
        let mut vtables = Vec::with_capacity(vtable_count);
        for _ in 0..vtable_count {
            let iface_idx = read_u32(r)? as usize;
            let func_count = read_u32(r)? as usize;
            let mut func_indices = Vec::with_capacity(func_count);
            for _ in 0..func_count {
                func_indices.push(read_u32(r)? as usize);
            }
            vtables.push((iface_idx, func_indices));
        }
        type_descriptors.push(super::TypeDescriptor {
            tag_name,
            field_names,
            field_type_tags,
            aux_type_tags,
            vtables,
        });
    }

    // Interface descriptors
    let iface_count = read_u32(r)? as usize;
    let mut interface_descriptors = Vec::with_capacity(iface_count);
    for _ in 0..iface_count {
        let name = read_string(r)?;
        let method_count = read_u32(r)? as usize;
        let mut method_names = Vec::with_capacity(method_count);
        for _ in 0..method_count {
            method_names.push(read_string(r)?);
        }
        interface_descriptors.push(super::InterfaceDescriptor { name, method_names });
    }

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
        type_descriptors,
        interface_descriptors,
        debug,
    })
}

fn write_function<W: Write>(w: &mut W, func: &Function) -> io::Result<()> {
    write_string(w, &func.name)?;
    write_u32(w, func.arity as u32)?;
    write_u32(w, func.locals_count as u32)?;

    // Local types
    write_u32(w, func.local_types.len() as u32)?;
    for vt in &func.local_types {
        write_value_type(w, *vt)?;
    }

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

    // Local types
    let local_types_len = read_u32(r)? as usize;
    let mut local_types = Vec::with_capacity(local_types_len);
    for _ in 0..local_types_len {
        local_types.push(read_value_type(r)?);
    }

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
        local_types,
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

// ============================================================
// Opcode tags — sequential u8, no gaps
// ============================================================

// Constants
const OP_I32_CONST: u8 = 0;
const OP_I64_CONST: u8 = 1;
const OP_F32_CONST: u8 = 2;
const OP_F64_CONST: u8 = 3;
const OP_REF_NULL: u8 = 4;
const OP_STRING_CONST: u8 = 5;

// Local Variables
const OP_LOCAL_GET: u8 = 6;
const OP_LOCAL_SET: u8 = 7;

// Stack Manipulation
const OP_DROP: u8 = 8;
const OP_DUP: u8 = 9;
const OP_PICK: u8 = 10;
const OP_PICK_DYN: u8 = 11;

// i32 Arithmetic
const OP_I32_ADD: u8 = 12;
const OP_I32_SUB: u8 = 13;
const OP_I32_MUL: u8 = 14;
const OP_I32_DIV_S: u8 = 15;
const OP_I32_REM_S: u8 = 16;
const OP_I32_EQZ: u8 = 17;

// i64 Arithmetic
const OP_I64_ADD: u8 = 18;
const OP_I64_SUB: u8 = 19;
const OP_I64_MUL: u8 = 20;
const OP_I64_DIV_S: u8 = 21;
const OP_I64_REM_S: u8 = 22;
const OP_I64_NEG: u8 = 23;

// f32 Arithmetic
const OP_F32_ADD: u8 = 24;
const OP_F32_SUB: u8 = 25;
const OP_F32_MUL: u8 = 26;
const OP_F32_DIV: u8 = 27;
const OP_F32_NEG: u8 = 28;

// f64 Arithmetic
const OP_F64_ADD: u8 = 29;
const OP_F64_SUB: u8 = 30;
const OP_F64_MUL: u8 = 31;
const OP_F64_DIV: u8 = 32;
const OP_F64_NEG: u8 = 33;

// i32 Comparison
const OP_I32_EQ: u8 = 34;
const OP_I32_NE: u8 = 35;
const OP_I32_LT_S: u8 = 36;
const OP_I32_LE_S: u8 = 37;
const OP_I32_GT_S: u8 = 38;
const OP_I32_GE_S: u8 = 39;

// i64 Comparison
const OP_I64_EQ: u8 = 40;
const OP_I64_NE: u8 = 41;
const OP_I64_LT_S: u8 = 42;
const OP_I64_LE_S: u8 = 43;
const OP_I64_GT_S: u8 = 44;
const OP_I64_GE_S: u8 = 45;

// f32 Comparison
const OP_F32_EQ: u8 = 46;
const OP_F32_NE: u8 = 47;
const OP_F32_LT: u8 = 48;
const OP_F32_LE: u8 = 49;
const OP_F32_GT: u8 = 50;
const OP_F32_GE: u8 = 51;

// f64 Comparison
const OP_F64_EQ: u8 = 52;
const OP_F64_NE: u8 = 53;
const OP_F64_LT: u8 = 54;
const OP_F64_LE: u8 = 55;
const OP_F64_GT: u8 = 56;
const OP_F64_GE: u8 = 57;

// Ref Comparison
const OP_REF_EQ: u8 = 58;
const OP_REF_IS_NULL: u8 = 59;

// Type Conversion
const OP_I32_WRAP_I64: u8 = 60;
const OP_I64_EXTEND_I32_S: u8 = 61;
const OP_I64_EXTEND_I32_U: u8 = 62;
const OP_F64_CONVERT_I64_S: u8 = 63;
const OP_I64_TRUNC_F64_S: u8 = 64;
const OP_F64_CONVERT_I32_S: u8 = 65;
const OP_F32_CONVERT_I32_S: u8 = 66;
const OP_F32_CONVERT_I64_S: u8 = 67;
const OP_I32_TRUNC_F32_S: u8 = 68;
const OP_I32_TRUNC_F64_S: u8 = 69;
const OP_I64_TRUNC_F32_S: u8 = 70;
const OP_F32_DEMOTE_F64: u8 = 71;
const OP_F64_PROMOTE_F32: u8 = 72;

// Control Flow
const OP_JMP: u8 = 73;
const OP_BR_IF: u8 = 74;
const OP_BR_IF_FALSE: u8 = 75;
const OP_CALL: u8 = 76;
const OP_RET: u8 = 77;

// Heap Operations
const OP_HEAP_ALLOC: u8 = 78;
const OP_HEAP_ALLOC_DYN: u8 = 79;
const OP_HEAP_ALLOC_DYN_SIMPLE: u8 = 80;
const OP_HEAP_LOAD: u8 = 81;
const OP_HEAP_STORE: u8 = 82;
const OP_HEAP_LOAD_DYN: u8 = 83;
const OP_HEAP_STORE_DYN: u8 = 84;

// System / Builtins
const OP_HOSTCALL: u8 = 86;
const OP_GC_HINT: u8 = 87;
const OP_TYPE_OF: u8 = 89;
const OP_HEAP_SIZE: u8 = 90;
// Exception Handling
const OP_THROW: u8 = 93;
const OP_TRY_BEGIN: u8 = 94;
const OP_TRY_END: u8 = 95;

// CLI Arguments
const OP_ARGC: u8 = 96;
const OP_ARGV: u8 = 97;
const OP_ARGS: u8 = 98;

// Threading
const OP_THREAD_SPAWN: u8 = 99;
const OP_CHANNEL_CREATE: u8 = 100;
const OP_CHANNEL_SEND: u8 = 101;
const OP_CHANNEL_RECV: u8 = 102;
const OP_THREAD_JOIN: u8 = 103;
const OP_HEAP_ALLOC_ARRAY: u8 = 104;

// Indirect heap access (ptr-based layout)
const OP_HEAP_LOAD2: u8 = 105;

// Closures
const OP_CALL_INDIRECT: u8 = 106;

const OP_HEAP_STORE2: u8 = 107;
// 108 was OP_HEAP_ALLOC_STRING, now unused (merged into HeapAllocArray with kind)

// Bitwise operations
const OP_I64_AND: u8 = 110;
const OP_I64_OR: u8 = 111;
const OP_I64_XOR: u8 = 112;
const OP_I64_SHL: u8 = 113;
const OP_I64_SHR_S: u8 = 114;
const OP_I64_SHR_U: u8 = 115;
const OP_F64_REINTERPRET_AS_I64: u8 = 116;
const OP_UMUL128_HI: u8 = 117;
const OP_HEAP_OFFSET_REF: u8 = 118;
const OP_GLOBAL_GET: u8 = 119;
// 120 is unused (was OP_IFACE_DESC_LOAD)
const OP_CALL_DYNAMIC: u8 = 121;
const OP_VTABLE_LOOKUP: u8 = 122;

fn write_op<W: Write>(w: &mut W, op: &Op) -> io::Result<()> {
    match op {
        // Constants
        Op::I32Const(v) => {
            w.write_all(&[OP_I32_CONST])?;
            write_i32(w, *v)?;
        }
        Op::I64Const(v) => {
            w.write_all(&[OP_I64_CONST])?;
            write_i64(w, *v)?;
        }
        Op::F32Const(v) => {
            w.write_all(&[OP_F32_CONST])?;
            write_f32(w, *v)?;
        }
        Op::F64Const(v) => {
            w.write_all(&[OP_F64_CONST])?;
            write_f64(w, *v)?;
        }
        Op::RefNull => w.write_all(&[OP_REF_NULL])?,
        Op::StringConst(idx) => {
            w.write_all(&[OP_STRING_CONST])?;
            write_u32(w, *idx as u32)?;
        }

        // Local Variables
        Op::LocalGet(idx) => {
            w.write_all(&[OP_LOCAL_GET])?;
            write_u32(w, *idx as u32)?;
        }
        Op::LocalSet(idx) => {
            w.write_all(&[OP_LOCAL_SET])?;
            write_u32(w, *idx as u32)?;
        }

        // Stack Manipulation
        Op::Drop => w.write_all(&[OP_DROP])?,
        Op::Dup => w.write_all(&[OP_DUP])?,
        Op::Pick(n) => {
            w.write_all(&[OP_PICK])?;
            write_u32(w, *n as u32)?;
        }
        Op::PickDyn => w.write_all(&[OP_PICK_DYN])?,

        // i32 Arithmetic
        Op::I32Add => w.write_all(&[OP_I32_ADD])?,
        Op::I32Sub => w.write_all(&[OP_I32_SUB])?,
        Op::I32Mul => w.write_all(&[OP_I32_MUL])?,
        Op::I32DivS => w.write_all(&[OP_I32_DIV_S])?,
        Op::I32RemS => w.write_all(&[OP_I32_REM_S])?,
        Op::I32Eqz => w.write_all(&[OP_I32_EQZ])?,

        // i64 Arithmetic
        Op::I64Add => w.write_all(&[OP_I64_ADD])?,
        Op::I64Sub => w.write_all(&[OP_I64_SUB])?,
        Op::I64Mul => w.write_all(&[OP_I64_MUL])?,
        Op::I64DivS => w.write_all(&[OP_I64_DIV_S])?,
        Op::I64RemS => w.write_all(&[OP_I64_REM_S])?,
        Op::I64Neg => w.write_all(&[OP_I64_NEG])?,
        Op::I64And => w.write_all(&[OP_I64_AND])?,
        Op::I64Or => w.write_all(&[OP_I64_OR])?,
        Op::I64Xor => w.write_all(&[OP_I64_XOR])?,
        Op::I64Shl => w.write_all(&[OP_I64_SHL])?,
        Op::I64ShrS => w.write_all(&[OP_I64_SHR_S])?,
        Op::I64ShrU => w.write_all(&[OP_I64_SHR_U])?,
        Op::F64ReinterpretAsI64 => w.write_all(&[OP_F64_REINTERPRET_AS_I64])?,
        Op::UMul128Hi => w.write_all(&[OP_UMUL128_HI])?,

        // f32 Arithmetic
        Op::F32Add => w.write_all(&[OP_F32_ADD])?,
        Op::F32Sub => w.write_all(&[OP_F32_SUB])?,
        Op::F32Mul => w.write_all(&[OP_F32_MUL])?,
        Op::F32Div => w.write_all(&[OP_F32_DIV])?,
        Op::F32Neg => w.write_all(&[OP_F32_NEG])?,

        // f64 Arithmetic
        Op::F64Add => w.write_all(&[OP_F64_ADD])?,
        Op::F64Sub => w.write_all(&[OP_F64_SUB])?,
        Op::F64Mul => w.write_all(&[OP_F64_MUL])?,
        Op::F64Div => w.write_all(&[OP_F64_DIV])?,
        Op::F64Neg => w.write_all(&[OP_F64_NEG])?,

        // i32 Comparison
        Op::I32Eq => w.write_all(&[OP_I32_EQ])?,
        Op::I32Ne => w.write_all(&[OP_I32_NE])?,
        Op::I32LtS => w.write_all(&[OP_I32_LT_S])?,
        Op::I32LeS => w.write_all(&[OP_I32_LE_S])?,
        Op::I32GtS => w.write_all(&[OP_I32_GT_S])?,
        Op::I32GeS => w.write_all(&[OP_I32_GE_S])?,

        // i64 Comparison
        Op::I64Eq => w.write_all(&[OP_I64_EQ])?,
        Op::I64Ne => w.write_all(&[OP_I64_NE])?,
        Op::I64LtS => w.write_all(&[OP_I64_LT_S])?,
        Op::I64LeS => w.write_all(&[OP_I64_LE_S])?,
        Op::I64GtS => w.write_all(&[OP_I64_GT_S])?,
        Op::I64GeS => w.write_all(&[OP_I64_GE_S])?,

        // f32 Comparison
        Op::F32Eq => w.write_all(&[OP_F32_EQ])?,
        Op::F32Ne => w.write_all(&[OP_F32_NE])?,
        Op::F32Lt => w.write_all(&[OP_F32_LT])?,
        Op::F32Le => w.write_all(&[OP_F32_LE])?,
        Op::F32Gt => w.write_all(&[OP_F32_GT])?,
        Op::F32Ge => w.write_all(&[OP_F32_GE])?,

        // f64 Comparison
        Op::F64Eq => w.write_all(&[OP_F64_EQ])?,
        Op::F64Ne => w.write_all(&[OP_F64_NE])?,
        Op::F64Lt => w.write_all(&[OP_F64_LT])?,
        Op::F64Le => w.write_all(&[OP_F64_LE])?,
        Op::F64Gt => w.write_all(&[OP_F64_GT])?,
        Op::F64Ge => w.write_all(&[OP_F64_GE])?,

        // Ref Comparison
        Op::RefEq => w.write_all(&[OP_REF_EQ])?,
        Op::RefIsNull => w.write_all(&[OP_REF_IS_NULL])?,

        // Type Conversion
        Op::I32WrapI64 => w.write_all(&[OP_I32_WRAP_I64])?,
        Op::I64ExtendI32S => w.write_all(&[OP_I64_EXTEND_I32_S])?,
        Op::I64ExtendI32U => w.write_all(&[OP_I64_EXTEND_I32_U])?,
        Op::F64ConvertI64S => w.write_all(&[OP_F64_CONVERT_I64_S])?,
        Op::I64TruncF64S => w.write_all(&[OP_I64_TRUNC_F64_S])?,
        Op::F64ConvertI32S => w.write_all(&[OP_F64_CONVERT_I32_S])?,
        Op::F32ConvertI32S => w.write_all(&[OP_F32_CONVERT_I32_S])?,
        Op::F32ConvertI64S => w.write_all(&[OP_F32_CONVERT_I64_S])?,
        Op::I32TruncF32S => w.write_all(&[OP_I32_TRUNC_F32_S])?,
        Op::I32TruncF64S => w.write_all(&[OP_I32_TRUNC_F64_S])?,
        Op::I64TruncF32S => w.write_all(&[OP_I64_TRUNC_F32_S])?,
        Op::F32DemoteF64 => w.write_all(&[OP_F32_DEMOTE_F64])?,
        Op::F64PromoteF32 => w.write_all(&[OP_F64_PROMOTE_F32])?,

        // Control Flow
        Op::Jmp(target) => {
            w.write_all(&[OP_JMP])?;
            write_u32(w, *target as u32)?;
        }
        Op::BrIf(target) => {
            w.write_all(&[OP_BR_IF])?;
            write_u32(w, *target as u32)?;
        }
        Op::BrIfFalse(target) => {
            w.write_all(&[OP_BR_IF_FALSE])?;
            write_u32(w, *target as u32)?;
        }
        Op::Call(func_idx, argc) => {
            w.write_all(&[OP_CALL])?;
            write_u32(w, *func_idx as u32)?;
            write_u32(w, *argc as u32)?;
        }
        Op::Ret => w.write_all(&[OP_RET])?,

        // Heap Operations
        Op::HeapAlloc(size) => {
            w.write_all(&[OP_HEAP_ALLOC])?;
            write_u32(w, *size as u32)?;
        }
        // HeapAllocArray removed — use HeapAlloc instead
        Op::HeapAllocDyn => w.write_all(&[OP_HEAP_ALLOC_DYN])?,
        Op::HeapAllocDynSimple(_) => w.write_all(&[OP_HEAP_ALLOC_DYN_SIMPLE])?,
        Op::HeapLoad(offset) => {
            w.write_all(&[OP_HEAP_LOAD])?;
            write_u32(w, *offset as u32)?;
        }
        Op::HeapStore(offset) => {
            w.write_all(&[OP_HEAP_STORE])?;
            write_u32(w, *offset as u32)?;
        }
        Op::HeapLoadDyn => w.write_all(&[OP_HEAP_LOAD_DYN])?,
        Op::HeapStoreDyn => w.write_all(&[OP_HEAP_STORE_DYN])?,
        Op::HeapLoad2(_) => w.write_all(&[OP_HEAP_LOAD2])?,
        Op::HeapStore2(_) => w.write_all(&[OP_HEAP_STORE2])?,
        Op::HeapOffsetRef => w.write_all(&[OP_HEAP_OFFSET_REF])?,
        // System / Builtins
        Op::Hostcall(num, argc) => {
            w.write_all(&[OP_HOSTCALL])?;
            write_u32(w, *num as u32)?;
            write_u32(w, *argc as u32)?;
        }
        Op::GcHint(size) => {
            w.write_all(&[OP_GC_HINT])?;
            write_u32(w, *size as u32)?;
        }
        Op::TypeOf => w.write_all(&[OP_TYPE_OF])?,
        Op::HeapSize => w.write_all(&[OP_HEAP_SIZE])?,
        // Exception Handling
        Op::Throw => w.write_all(&[OP_THROW])?,
        Op::TryBegin(target) => {
            w.write_all(&[OP_TRY_BEGIN])?;
            write_u32(w, *target as u32)?;
        }
        Op::TryEnd => w.write_all(&[OP_TRY_END])?,

        // CLI Arguments
        Op::Argc => w.write_all(&[OP_ARGC])?,
        Op::Argv => w.write_all(&[OP_ARGV])?,
        Op::Args => w.write_all(&[OP_ARGS])?,

        // Threading
        Op::ThreadSpawn(func_idx) => {
            w.write_all(&[OP_THREAD_SPAWN])?;
            write_u32(w, *func_idx as u32)?;
        }
        Op::ChannelCreate => w.write_all(&[OP_CHANNEL_CREATE])?,
        Op::ChannelSend => w.write_all(&[OP_CHANNEL_SEND])?,
        Op::ChannelRecv => w.write_all(&[OP_CHANNEL_RECV])?,
        Op::ThreadJoin => w.write_all(&[OP_THREAD_JOIN])?,

        // Closures
        Op::CallIndirect(argc) => {
            w.write_all(&[OP_CALL_INDIRECT])?;
            write_u32(w, *argc as u32)?;
        }

        // Globals
        Op::GlobalGet(idx) => {
            w.write_all(&[OP_GLOBAL_GET])?;
            write_u32(w, *idx as u32)?;
        }

        // Dynamic call by func_index on stack
        Op::CallDynamic(argc) => {
            w.write_all(&[OP_CALL_DYNAMIC])?;
            write_u32(w, *argc as u32)?;
        }

        // Vtable lookup
        Op::VtableLookup => {
            w.write_all(&[OP_VTABLE_LOOKUP])?;
        }
    }
    Ok(())
}

fn read_op<R: Read>(r: &mut R) -> Result<Op, BytecodeError> {
    let tag = read_u8(r)?;
    let op = match tag {
        // Constants
        OP_I32_CONST => Op::I32Const(read_i32(r)?),
        OP_I64_CONST => Op::I64Const(read_i64(r)?),
        OP_F32_CONST => Op::F32Const(read_f32(r)?),
        OP_F64_CONST => Op::F64Const(read_f64(r)?),
        OP_REF_NULL => Op::RefNull,
        OP_STRING_CONST => Op::StringConst(read_u32(r)? as usize),

        // Local Variables
        OP_LOCAL_GET => Op::LocalGet(read_u32(r)? as usize),
        OP_LOCAL_SET => Op::LocalSet(read_u32(r)? as usize),

        // Stack Manipulation
        OP_DROP => Op::Drop,
        OP_DUP => Op::Dup,
        OP_PICK => Op::Pick(read_u32(r)? as usize),
        OP_PICK_DYN => Op::PickDyn,

        // i32 Arithmetic
        OP_I32_ADD => Op::I32Add,
        OP_I32_SUB => Op::I32Sub,
        OP_I32_MUL => Op::I32Mul,
        OP_I32_DIV_S => Op::I32DivS,
        OP_I32_REM_S => Op::I32RemS,
        OP_I32_EQZ => Op::I32Eqz,

        // i64 Arithmetic
        OP_I64_ADD => Op::I64Add,
        OP_I64_SUB => Op::I64Sub,
        OP_I64_MUL => Op::I64Mul,
        OP_I64_DIV_S => Op::I64DivS,
        OP_I64_REM_S => Op::I64RemS,
        OP_I64_NEG => Op::I64Neg,
        OP_I64_AND => Op::I64And,
        OP_I64_OR => Op::I64Or,
        OP_I64_XOR => Op::I64Xor,
        OP_I64_SHL => Op::I64Shl,
        OP_I64_SHR_S => Op::I64ShrS,
        OP_I64_SHR_U => Op::I64ShrU,
        OP_F64_REINTERPRET_AS_I64 => Op::F64ReinterpretAsI64,
        OP_UMUL128_HI => Op::UMul128Hi,

        // f32 Arithmetic
        OP_F32_ADD => Op::F32Add,
        OP_F32_SUB => Op::F32Sub,
        OP_F32_MUL => Op::F32Mul,
        OP_F32_DIV => Op::F32Div,
        OP_F32_NEG => Op::F32Neg,

        // f64 Arithmetic
        OP_F64_ADD => Op::F64Add,
        OP_F64_SUB => Op::F64Sub,
        OP_F64_MUL => Op::F64Mul,
        OP_F64_DIV => Op::F64Div,
        OP_F64_NEG => Op::F64Neg,

        // i32 Comparison
        OP_I32_EQ => Op::I32Eq,
        OP_I32_NE => Op::I32Ne,
        OP_I32_LT_S => Op::I32LtS,
        OP_I32_LE_S => Op::I32LeS,
        OP_I32_GT_S => Op::I32GtS,
        OP_I32_GE_S => Op::I32GeS,

        // i64 Comparison
        OP_I64_EQ => Op::I64Eq,
        OP_I64_NE => Op::I64Ne,
        OP_I64_LT_S => Op::I64LtS,
        OP_I64_LE_S => Op::I64LeS,
        OP_I64_GT_S => Op::I64GtS,
        OP_I64_GE_S => Op::I64GeS,

        // f32 Comparison
        OP_F32_EQ => Op::F32Eq,
        OP_F32_NE => Op::F32Ne,
        OP_F32_LT => Op::F32Lt,
        OP_F32_LE => Op::F32Le,
        OP_F32_GT => Op::F32Gt,
        OP_F32_GE => Op::F32Ge,

        // f64 Comparison
        OP_F64_EQ => Op::F64Eq,
        OP_F64_NE => Op::F64Ne,
        OP_F64_LT => Op::F64Lt,
        OP_F64_LE => Op::F64Le,
        OP_F64_GT => Op::F64Gt,
        OP_F64_GE => Op::F64Ge,

        // Ref Comparison
        OP_REF_EQ => Op::RefEq,
        OP_REF_IS_NULL => Op::RefIsNull,

        // Type Conversion
        OP_I32_WRAP_I64 => Op::I32WrapI64,
        OP_I64_EXTEND_I32_S => Op::I64ExtendI32S,
        OP_I64_EXTEND_I32_U => Op::I64ExtendI32U,
        OP_F64_CONVERT_I64_S => Op::F64ConvertI64S,
        OP_I64_TRUNC_F64_S => Op::I64TruncF64S,
        OP_F64_CONVERT_I32_S => Op::F64ConvertI32S,
        OP_F32_CONVERT_I32_S => Op::F32ConvertI32S,
        OP_F32_CONVERT_I64_S => Op::F32ConvertI64S,
        OP_I32_TRUNC_F32_S => Op::I32TruncF32S,
        OP_I32_TRUNC_F64_S => Op::I32TruncF64S,
        OP_I64_TRUNC_F32_S => Op::I64TruncF32S,
        OP_F32_DEMOTE_F64 => Op::F32DemoteF64,
        OP_F64_PROMOTE_F32 => Op::F64PromoteF32,

        // Control Flow
        OP_JMP => Op::Jmp(read_u32(r)? as usize),
        OP_BR_IF => Op::BrIf(read_u32(r)? as usize),
        OP_BR_IF_FALSE => Op::BrIfFalse(read_u32(r)? as usize),
        OP_CALL => {
            let func_idx = read_u32(r)? as usize;
            let argc = read_u32(r)? as usize;
            Op::Call(func_idx, argc)
        }
        OP_RET => Op::Ret,

        // Heap Operations
        OP_HEAP_ALLOC => Op::HeapAlloc(read_u32(r)? as usize),
        // OP_HEAP_ALLOC_ARRAY removed — use HeapAlloc instead
        OP_HEAP_ALLOC_ARRAY => Op::HeapAlloc(read_u32(r)? as usize),
        OP_HEAP_ALLOC_DYN => Op::HeapAllocDyn,
        OP_HEAP_ALLOC_DYN_SIMPLE => Op::HeapAllocDynSimple(ElemKind::Tagged),
        OP_HEAP_LOAD => Op::HeapLoad(read_u32(r)? as usize),
        OP_HEAP_STORE => Op::HeapStore(read_u32(r)? as usize),
        OP_HEAP_LOAD_DYN => Op::HeapLoadDyn,
        OP_HEAP_STORE_DYN => Op::HeapStoreDyn,
        OP_HEAP_LOAD2 => Op::HeapLoad2(ElemKind::Tagged),
        OP_HEAP_STORE2 => Op::HeapStore2(ElemKind::Tagged),
        OP_HEAP_OFFSET_REF => Op::HeapOffsetRef,
        // System / Builtins
        OP_HOSTCALL => Op::Hostcall(read_u32(r)? as usize, read_u32(r)? as usize),
        OP_GC_HINT => Op::GcHint(read_u32(r)? as usize),
        OP_TYPE_OF => Op::TypeOf,
        OP_HEAP_SIZE => Op::HeapSize,
        // Exception Handling
        OP_THROW => Op::Throw,
        OP_TRY_BEGIN => Op::TryBegin(read_u32(r)? as usize),
        OP_TRY_END => Op::TryEnd,

        // CLI Arguments
        OP_ARGC => Op::Argc,
        OP_ARGV => Op::Argv,
        OP_ARGS => Op::Args,

        // Threading
        OP_THREAD_SPAWN => Op::ThreadSpawn(read_u32(r)? as usize),
        OP_CHANNEL_CREATE => Op::ChannelCreate,
        OP_CHANNEL_SEND => Op::ChannelSend,
        OP_CHANNEL_RECV => Op::ChannelRecv,
        OP_THREAD_JOIN => Op::ThreadJoin,

        // Closures
        OP_CALL_INDIRECT => Op::CallIndirect(read_u32(r)? as usize),

        // Type Descriptor
        // Globals
        OP_GLOBAL_GET => Op::GlobalGet(read_u32(r)? as usize),

        // Dynamic call by func_index on stack
        OP_CALL_DYNAMIC => Op::CallDynamic(read_u32(r)? as usize),

        // Vtable lookup
        OP_VTABLE_LOOKUP => Op::VtableLookup,

        _ => return Err(BytecodeError::InvalidOpcode(tag)),
    };
    Ok(op)
}

// ============================================================
// ValueType serialization
// ============================================================

const VT_I32: u8 = 0;
const VT_I64: u8 = 1;
const VT_F32: u8 = 2;
const VT_F64: u8 = 3;
const VT_REF: u8 = 4;

fn write_value_type<W: Write>(w: &mut W, vt: ValueType) -> io::Result<()> {
    let tag = match vt {
        ValueType::I32 => VT_I32,
        ValueType::I64 => VT_I64,
        ValueType::F32 => VT_F32,
        ValueType::F64 => VT_F64,
        ValueType::Ref => VT_REF,
    };
    write_u8(w, tag)
}

fn read_value_type<R: Read>(r: &mut R) -> Result<ValueType, BytecodeError> {
    let tag = read_u8(r)?;
    match tag {
        VT_I32 => Ok(ValueType::I32),
        VT_I64 => Ok(ValueType::I64),
        VT_F32 => Ok(ValueType::F32),
        VT_F64 => Ok(ValueType::F64),
        VT_REF => Ok(ValueType::Ref),
        _ => Err(BytecodeError::InvalidValueType(tag)),
    }
}

// ============================================================
// Helper functions for reading/writing primitives
// ============================================================

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

fn write_i32<W: Write>(w: &mut W, v: i32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn read_i32<R: Read>(r: &mut R) -> Result<i32, BytecodeError> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)
        .map_err(|_| BytecodeError::UnexpectedEof)?;
    Ok(i32::from_le_bytes(buf))
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

fn write_f32<W: Write>(w: &mut W, v: f32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn read_f32<R: Read>(r: &mut R) -> Result<f32, BytecodeError> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)
        .map_err(|_| BytecodeError::UnexpectedEof)?;
    Ok(f32::from_le_bytes(buf))
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
                    Op::I32Const(42),
                    Op::F64Const(3.14),
                    Op::RefNull,
                    Op::I64Add,
                    Op::Ret,
                ],
                stackmap: None,
                local_types: vec![],
            },
            strings: vec!["hello".to_string(), "world".to_string()],
            type_descriptors: vec![],
            interface_descriptors: vec![],
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
                code: vec![Op::LocalGet(0), Op::LocalGet(1), Op::I64Add, Op::Ret],
                stackmap: None,
                local_types: vec![ValueType::I64, ValueType::I64],
            }],
            main: Function {
                name: "main".to_string(),
                arity: 0,
                locals_count: 0,
                code: vec![
                    Op::I64Const(10),
                    Op::I64Const(20),
                    Op::Call(0, 2),
                    Op::TypeOf,
                    Op::Ret,
                ],
                stackmap: None,
                local_types: vec![],
            },
            strings: vec![],
            type_descriptors: vec![],
            interface_descriptors: vec![],
            debug: None,
        };

        let bytes = serialize(&chunk);
        let restored = deserialize(&bytes).unwrap();

        assert_eq!(restored.functions.len(), 1);
        assert_eq!(restored.functions[0].name, "add");
        assert_eq!(restored.functions[0].arity, 2);
        assert_eq!(
            restored.functions[0].local_types,
            vec![ValueType::I64, ValueType::I64]
        );
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
                code: vec![Op::HeapAlloc(2), Op::Ret],
                stackmap: Some(stackmap),
                local_types: vec![
                    ValueType::I64,
                    ValueType::Ref,
                    ValueType::Ref,
                    ValueType::I32,
                ],
            },
            strings: vec![],
            type_descriptors: vec![],
            interface_descriptors: vec![],
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
        assert_eq!(
            restored.main.local_types,
            vec![
                ValueType::I64,
                ValueType::Ref,
                ValueType::Ref,
                ValueType::I32
            ]
        );
    }

    #[test]
    fn test_invalid_magic() {
        let data = b"BADM\x02\x00\x00\x00";
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
            // Constants
            Op::I32Const(i32::MAX),
            Op::I32Const(i32::MIN),
            Op::I32Const(0),
            Op::I64Const(i64::MAX),
            Op::I64Const(i64::MIN),
            Op::F32Const(std::f32::consts::PI),
            Op::F32Const(-0.0f32),
            Op::F64Const(std::f64::consts::PI),
            Op::F64Const(-0.0f64),
            Op::RefNull,
            Op::StringConst(42),
            // Local Variables
            Op::LocalGet(100),
            Op::LocalSet(200),
            // Stack Manipulation
            Op::Drop,
            Op::Dup,
            Op::Pick(3),
            Op::PickDyn,
            // i32 Arithmetic
            Op::I32Add,
            Op::I32Sub,
            Op::I32Mul,
            Op::I32DivS,
            Op::I32RemS,
            Op::I32Eqz,
            // i64 Arithmetic
            Op::I64Add,
            Op::I64Sub,
            Op::I64Mul,
            Op::I64DivS,
            Op::I64RemS,
            Op::I64Neg,
            // f32 Arithmetic
            Op::F32Add,
            Op::F32Sub,
            Op::F32Mul,
            Op::F32Div,
            Op::F32Neg,
            // f64 Arithmetic
            Op::F64Add,
            Op::F64Sub,
            Op::F64Mul,
            Op::F64Div,
            Op::F64Neg,
            // i32 Comparison
            Op::I32Eq,
            Op::I32Ne,
            Op::I32LtS,
            Op::I32LeS,
            Op::I32GtS,
            Op::I32GeS,
            // i64 Comparison
            Op::I64Eq,
            Op::I64Ne,
            Op::I64LtS,
            Op::I64LeS,
            Op::I64GtS,
            Op::I64GeS,
            // f32 Comparison
            Op::F32Eq,
            Op::F32Ne,
            Op::F32Lt,
            Op::F32Le,
            Op::F32Gt,
            Op::F32Ge,
            // f64 Comparison
            Op::F64Eq,
            Op::F64Ne,
            Op::F64Lt,
            Op::F64Le,
            Op::F64Gt,
            Op::F64Ge,
            // Ref Comparison
            Op::RefEq,
            Op::RefIsNull,
            // Type Conversion
            Op::I32WrapI64,
            Op::I64ExtendI32S,
            Op::I64ExtendI32U,
            Op::F64ConvertI64S,
            Op::I64TruncF64S,
            Op::F64ConvertI32S,
            Op::F32ConvertI32S,
            Op::F32ConvertI64S,
            Op::I32TruncF32S,
            Op::I32TruncF64S,
            Op::I64TruncF32S,
            Op::F32DemoteF64,
            Op::F64PromoteF32,
            // Control Flow
            Op::Jmp(1000),
            Op::BrIf(2000),
            Op::BrIfFalse(3000),
            Op::Call(5, 3),
            Op::Ret,
            // Heap Operations
            Op::HeapAlloc(5),
            Op::HeapAllocDyn,
            Op::HeapAllocDynSimple(ElemKind::Tagged),
            // HeapAllocArray removed from test
            Op::HeapLoad(1),
            Op::HeapStore(2),
            Op::HeapLoadDyn,
            Op::HeapStoreDyn,
            Op::HeapLoad2(ElemKind::Tagged),
            Op::HeapStore2(ElemKind::Tagged),
            Op::HeapOffsetRef,
            // System / Builtins
            Op::Hostcall(7, 2),
            Op::GcHint(1024),
            Op::TypeOf,
            Op::HeapSize,
            // Exception Handling
            Op::Throw,
            Op::TryBegin(100),
            Op::TryEnd,
            // CLI Arguments
            Op::Argc,
            Op::Argv,
            Op::Args,
            // Threading
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
                local_types: vec![],
            },
            strings: vec![],
            type_descriptors: vec![],
            interface_descriptors: vec![],
            debug: None,
        };

        let bytes = serialize(&chunk);
        let restored = deserialize(&bytes).unwrap();

        assert_eq!(restored.main.code.len(), ops.len());
        for (orig, rest) in ops.iter().zip(restored.main.code.iter()) {
            assert_eq!(orig, rest);
        }
    }

    #[test]
    fn test_value_type_roundtrip() {
        let chunk = Chunk {
            functions: vec![Function {
                name: "typed_fn".to_string(),
                arity: 3,
                locals_count: 5,
                code: vec![Op::LocalGet(0), Op::Ret],
                stackmap: None,
                local_types: vec![
                    ValueType::I32,
                    ValueType::I64,
                    ValueType::F32,
                    ValueType::F64,
                    ValueType::Ref,
                ],
            }],
            main: Function {
                name: "main".to_string(),
                arity: 0,
                locals_count: 0,
                code: vec![Op::Ret],
                stackmap: None,
                local_types: vec![],
            },
            strings: vec![],
            type_descriptors: vec![],
            interface_descriptors: vec![],
            debug: None,
        };

        let bytes = serialize(&chunk);
        let restored = deserialize(&bytes).unwrap();

        assert_eq!(restored.functions[0].local_types.len(), 5);
        assert_eq!(restored.functions[0].local_types[0], ValueType::I32);
        assert_eq!(restored.functions[0].local_types[1], ValueType::I64);
        assert_eq!(restored.functions[0].local_types[2], ValueType::F32);
        assert_eq!(restored.functions[0].local_types[3], ValueType::F64);
        assert_eq!(restored.functions[0].local_types[4], ValueType::Ref);
    }

    #[test]
    fn test_f32_const_roundtrip() {
        let ops = vec![
            Op::F32Const(1.5f32),
            Op::F32Const(-0.0f32),
            Op::F32Const(f32::INFINITY),
            Op::F32Const(f32::NEG_INFINITY),
            Op::Ret,
        ];

        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "f32_test".to_string(),
                arity: 0,
                locals_count: 0,
                code: ops.clone(),
                stackmap: None,
                local_types: vec![],
            },
            strings: vec![],
            type_descriptors: vec![],
            interface_descriptors: vec![],
            debug: None,
        };

        let bytes = serialize(&chunk);
        let restored = deserialize(&bytes).unwrap();

        assert_eq!(restored.main.code.len(), ops.len());
        for (orig, rest) in ops.iter().zip(restored.main.code.iter()) {
            assert_eq!(orig, rest);
        }
    }

    #[test]
    fn test_i32_const_roundtrip() {
        let ops = vec![
            Op::I32Const(0),
            Op::I32Const(1),
            Op::I32Const(-1),
            Op::I32Const(i32::MAX),
            Op::I32Const(i32::MIN),
            Op::Ret,
        ];

        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "i32_test".to_string(),
                arity: 0,
                locals_count: 0,
                code: ops.clone(),
                stackmap: None,
                local_types: vec![],
            },
            strings: vec![],
            type_descriptors: vec![],
            interface_descriptors: vec![],
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
