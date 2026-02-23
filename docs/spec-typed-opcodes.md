---
title: Typed Opcode Architecture
description: WASM-like typed opcode specification for the moca VM. Defines type-specific instructions for arithmetic, comparison, and control flow.
---

# Typed Opcode Architecture

This document defines the WASM-like typed opcode architecture for the moca VM.

## Overview

The moca VM uses statically-typed opcodes where each operation specifies its operand types. This follows the WebAssembly (WASM) design philosophy, enabling:

- Efficient execution without runtime type checks
- Easier verification and optimization
- Clear type semantics for JIT compilation

## ValueType

The VM uses five fundamental value types:

```rust
pub enum ValueType {
    I32,   // 32-bit integer (used for booleans)
    I64,   // 64-bit integer
    F32,   // 32-bit float
    F64,   // 64-bit float
    Ref,   // GC-managed heap reference
}
```

### Language Type Mapping

| moca type | VM ValueType | Notes |
|-----------|-------------|-------|
| `int` | I64 | 64-bit signed integer |
| `float` | F64 | 64-bit IEEE 754 double |
| `bool` | I32 | true=1, false=0 |
| `string` | Ref | Heap-allocated string object |
| struct/array/vector | Ref | Heap-allocated objects |
| `null` | Ref | Null reference (value 0) |

## Instruction Set

### Constants

```
I32Const(i32)      // Push 32-bit integer → [i32]
I64Const(i64)      // Push 64-bit integer → [i64]
F32Const(f32)      // Push 32-bit float → [f32]
F64Const(f64)      // Push 64-bit float → [f64]
RefNull            // Push null reference → [ref]
StringConst(idx)   // Push string from pool → [ref]
```

### Local Variables

```
LocalGet(n)        // Push locals[n] → [value]
LocalSet(n)        // Pop to locals[n] → []
```

### Stack Manipulation

```
Drop               // Discard top → []
Dup                // Duplicate top → [a, a]
Pick(n)            // Copy n-th element to top
PickDyn            // Dynamic pick: [depth] → [value]
```

### i32 Arithmetic

```
I32Add             // [i32, i32] → [i32]
I32Sub             // [i32, i32] → [i32]
I32Mul             // [i32, i32] → [i32]
I32DivS            // [i32, i32] → [i32] (signed division)
I32RemS            // [i32, i32] → [i32] (signed remainder)
I32Eqz             // [i32] → [i32] (x == 0 ? 1 : 0)
```

### i64 Arithmetic

```
I64Add             // [i64, i64] → [i64]
I64Sub             // [i64, i64] → [i64]
I64Mul             // [i64, i64] → [i64]
I64DivS            // [i64, i64] → [i64] (signed division)
I64RemS            // [i64, i64] → [i64] (signed remainder)
I64Neg             // [i64] → [i64] (negation)
```

### f32 Arithmetic

```
F32Add             // [f32, f32] → [f32]
F32Sub             // [f32, f32] → [f32]
F32Mul             // [f32, f32] → [f32]
F32Div             // [f32, f32] → [f32]
F32Neg             // [f32] → [f32]
```

### f64 Arithmetic

```
F64Add             // [f64, f64] → [f64]
F64Sub             // [f64, f64] → [f64]
F64Mul             // [f64, f64] → [f64]
F64Div             // [f64, f64] → [f64]
F64Neg             // [f64] → [f64]
```

### Comparison Operations

All comparison operations return i32 (0 or 1):

```
// i32 comparisons
I32Eq, I32Ne, I32LtS, I32LeS, I32GtS, I32GeS

// i64 comparisons
I64Eq, I64Ne, I64LtS, I64LeS, I64GtS, I64GeS

// f32 comparisons
F32Eq, F32Ne, F32Lt, F32Le, F32Gt, F32Ge

// f64 comparisons
F64Eq, F64Ne, F64Lt, F64Le, F64Gt, F64Ge

// Reference comparisons
RefEq              // [ref, ref] → [i32]
RefIsNull          // [ref] → [i32]
```

### Type Conversion

```
I32WrapI64         // [i64] → [i32] (truncate)
I64ExtendI32S      // [i32] → [i64] (sign-extend)
I64ExtendI32U      // [i32] → [i64] (zero-extend)
F64ConvertI64S     // [i64] → [f64]
I64TruncF64S       // [f64] → [i64]
F64ConvertI32S     // [i32] → [f64]
F32ConvertI32S     // [i32] → [f32]
F32ConvertI64S     // [i64] → [f32]
I32TruncF32S       // [f32] → [i32]
I32TruncF64S       // [f64] → [i32]
I64TruncF32S       // [f32] → [i64]
F32DemoteF64       // [f64] → [f32]
F64PromoteF32      // [f32] → [f64]
```

### Control Flow

```
Jmp(target)        // Unconditional jump
BrIf(target)       // [i32] → [] (branch if != 0)
BrIfFalse(target)  // [i32] → [] (branch if == 0)
Call(idx, argc)    // Call function at index with argc args
Ret                // Return from function
```

### Heap Operations

```
HeapAlloc(n)       // [v1..vN] → [ref] (allocate N slots)
HeapAllocDyn       // [size, v1..vN] → [ref]
HeapAllocDynSimple // [size] → [ref] (null-initialized)
HeapLoad(idx)      // [ref] → [value] (static offset)
HeapStore(idx)     // [ref, value] → [] (static offset)
HeapLoadDyn        // [ref, index] → [value]
HeapStoreDyn       // [ref, index, value] → []
ArrayLen           // [ref] → [i64]
```

### System Operations

```
Hostcall(num, argc) // Host call
GcHint(size)       // GC allocation hint
PrintDebug         // Debug print
TypeOf             // [any] → [ref(string)]
ToString           // [any] → [ref(string)]
ParseInt           // [ref(string)] → [i64]
StrLen             // [ref(string)] → [i64]
```

### Exception Handling

```
Throw              // Throw exception
TryBegin(handler)  // Begin try block
TryEnd             // End try block
```

### CLI Operations

```
Argc               // → [i64] (argument count)
Argv               // [i64] → [ref(string)]
Args               // → [ref(array)]
```

### Threading

```
ThreadSpawn(idx)   // Spawn thread
ChannelCreate      // Create channel
ChannelSend        // Send to channel
ChannelRecv        // Receive from channel
ThreadJoin         // Join thread
```

## Legacy Opcode Migration

The following table maps legacy opcodes to their typed equivalents:

| Legacy Op | New Op | Notes |
|-----------|--------|-------|
| `PushInt(v)` | `I64Const(v)` | |
| `PushFloat(v)` | `F64Const(v)` | |
| `PushTrue` | `I32Const(1)` | Bool as i32 |
| `PushFalse` | `I32Const(0)` | Bool as i32 |
| `PushNull` | `RefNull` | |
| `PushString(idx)` | `StringConst(idx)` | |
| `Pop` | `Drop` | |
| `GetL(n)` | `LocalGet(n)` | |
| `SetL(n)` | `LocalSet(n)` | |
| `Add` | `I64Add` / `F64Add` | Codegen selects by type |
| `Sub` | `I64Sub` / `F64Sub` | |
| `Mul` | `I64Mul` / `F64Mul` | |
| `Div` | `I64DivS` / `F64Div` | |
| `Mod` | `I64RemS` | |
| `Neg` | `I64Neg` / `F64Neg` | |
| `Eq` | `I64Eq` / `F64Eq` / `RefEq` | |
| `Ne` | `I64Ne` / `F64Ne` | |
| `Lt` | `I64LtS` / `F64Lt` | |
| `Le` | `I64LeS` / `F64Le` | |
| `Gt` | `I64GtS` / `F64Gt` | |
| `Ge` | `I64GeS` / `F64Ge` | |
| `Not` | `I32Eqz` | Logical negation |
| `JmpIfFalse(t)` | `BrIfFalse(t)` | |
| `JmpIfTrue(t)` | `BrIf(t)` | |
| `AllocHeap(n)` | `HeapAlloc(n)` | |

## Codegen Type Selection

The code generator uses type inference to select appropriate typed opcodes:

1. **Literal types**: Integer literals → `I64Const`, float literals → `F64Const`
2. **Binary operations**: Check operand types from resolver/typechecker
3. **Comparisons**: Result type is always `I32` (boolean)
4. **Conditions**: `if`/`while` consume `I32` values via `BrIfFalse`

## Boolean Semantics

Booleans are represented as `I32`:
- `true` → `I32Const(1)`
- `false` → `I32Const(0)`
- Comparison results push `Bool` values to preserve display semantics
- Branch instructions (`BrIf`, `BrIfFalse`) check `is_truthy()`

## Bytecode Format

- Version: 2
- Opcodes use fixed encoding per instruction type
- Function metadata includes `local_types: Vec<ValueType>` for verification
