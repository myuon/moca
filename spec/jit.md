---
title: JIT Specification
description: JIT コンパイルと実行時最適化機能の仕様。Tier 0 インタプリタと Tier 1 ベースライン JIT の2段階実行モデル。
---

# Mica JIT Specification

This document defines the JIT compilation and runtime optimization features.

## Overview

Mica uses a tiered execution model:
1. **Tier 0**: Bytecode Interpreter with Quickening
2. **Tier 1**: Baseline JIT (AArch64, x86-64)

## Tiered Execution

```
┌─────────────────────────────────────────────────────────┐
│                    Function Entry                        │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│  Tier 0: Bytecode Interpreter                           │
│  - Immediate execution                                  │
│  - Increment call counter                               │
│  - Queue for JIT when hot threshold reached             │
└─────────────────────────────────────────────────────────┘
                          │
                          │ hot (count >= threshold)
                          ▼
┌─────────────────────────────────────────────────────────┐
│  Tier 1: Baseline JIT (AArch64)                         │
│  - Template-based native code generation                │
│  - Replace function entry with JIT code                 │
│  - Embed safepoints and stack maps                      │
└─────────────────────────────────────────────────────────┘
```

### Threshold and Triggers

- Default: 1000 invocations to trigger JIT
- Configurable via `--jit-threshold=<n>`
- Disable JIT with `--jit=off`

## Quickening

Quickening specializes bytecode instructions at first execution based on observed types.

### How It Works

1. First execution of an instruction observes operand types
2. Instruction is rewritten to a specialized version
3. Subsequent executions use the fast path

### Specialized Instructions

```
// Generic instruction
ADD                     // Any Value addition

// Specialized instructions (after quickening)
ADD_SMI_SMI             // SMI + SMI fast path
ADD_FLOAT_FLOAT         // f64 + f64 fast path
ADD_STRING_STRING       // String concatenation fast path
```

### Implementation Example

```rust
fn execute_add(&mut self) {
    let b = self.pop();
    let a = self.pop();

    if a.is_smi() && b.is_smi() {
        // Rewrite to ADD_SMI_SMI for next execution
        self.quicken_to(Op::ADD_SMI_SMI);
        self.push(a.as_smi() + b.as_smi());
    } else {
        // Generic path
        self.push(self.generic_add(a, b));
    }
}
```

## Inline Cache

Inline caches speed up property access and method calls by caching type information.

### Use Cases

- Property access (`obj.field`)
- Method calls (`obj.method()`)
- Array access (type specialization)

### Structure

```rust
struct InlineCache {
    // Monomorphic cache
    cached_type_id: u32,
    cached_offset: u16,

    // State
    state: CacheState,
}

enum CacheState {
    Uninitialized,        // No type seen yet
    Monomorphic,          // Single type
    Polymorphic(4),       // 2-4 types
    Megamorphic,          // 5+ types, cache disabled
}
```

### Property Access Optimization

```rust
fn execute_read_field(&mut self, field_name: &str, ic: &mut InlineCache) {
    let obj = self.peek();
    let type_id = obj.type_id();

    if ic.state == Monomorphic && ic.cached_type_id == type_id {
        // Cache hit: direct offset access
        let value = obj.read_field_at_offset(ic.cached_offset);
        self.replace_top(value);
    } else {
        // Cache miss: lookup and update cache
        let (value, offset) = obj.lookup_field(field_name);
        ic.update(type_id, offset);
        self.replace_top(value);
    }
}
```

## Baseline JIT (AArch64)

### Code Generation Strategy

- Template-based (fixed pattern per instruction)
- Simple register allocation (maintain stack-based model)
- Minimal optimization (future Tier 2 will handle complex optimization)

### AArch64 Register Convention

```
x0-x7   : Arguments / return values
x8      : Indirect result location
x9-x15  : Caller-saved temporaries
x16-x17 : Intra-procedure-call scratch (IP0, IP1)
x18     : Platform register (reserved)
x19-x28 : Callee-saved
x29     : Frame pointer (FP)
x30     : Link register (LR)
sp      : Stack pointer
```

### Mica JIT Register Usage

```
x19     : VM state pointer
x20     : Value stack pointer
x21     : Locals base pointer
x22     : Constants pool pointer
x23-x28 : Reserved for future use
```

### Code Generation Example (ADD_SMI_SMI)

```asm
// Pop two values, add, push result
ldr x0, [x20, #-8]!     // pop a
ldr x1, [x20, #-8]!     // pop b
// Tag check (SMI lower bits are 001)
and x2, x0, #0x7
and x3, x1, #0x7
orr x2, x2, x3
cmp x2, #0x2            // Both SMI?
b.ne slow_path
// SMI addition (maintain tag)
add x0, x0, x1
sub x0, x0, #1          // Tag adjustment
str x0, [x20], #8       // push result
```

### Safepoint Emission

```asm
// Safepoint: Point where GC can stop the thread
safepoint:
    ldr x0, [x19, #VM_GC_PENDING_OFFSET]
    cbz x0, continue
    bl gc_safepoint_handler
continue:
```

## Stack Map

Stack maps allow the GC to identify references in JIT-compiled frames.

### Purpose

- Identify reference slots on stack
- Enable precise GC during JIT execution

### Structure

```rust
struct StackMap {
    // PC → stack slot reference bitmap
    entries: Vec<StackMapEntry>,
}

struct StackMapEntry {
    native_pc: u32,          // JIT code offset
    bytecode_pc: u32,        // Corresponding bytecode PC
    stack_slots: BitVec,     // Reference slot bitmap
    locals_slots: BitVec,    // Reference locals bitmap
}
```

## Write Barrier in JIT

```asm
write_field:
    // Barrier check
    ldr x3, [x19, #VM_GC_MARKING_OFFSET]
    cbz x3, no_barrier

    // Old value check
    ldr x4, [x0, x1, lsl #3]  // old value
    and x5, x4, #0x7
    cbnz x5, no_barrier       // not a pointer

    // Mark gray
    bl gc_mark_gray

no_barrier:
    str x2, [x0, x1, lsl #3]  // write new value
```

## CLI Options

```bash
--jit=[on|off|auto]     # JIT mode (default: auto)
--jit-threshold=<n>     # Compilation threshold (default: 1000)
--trace-jit             # Output JIT compilation info
```

### Example Output with --trace-jit

```
$ mica run --trace-jit app.mica
[JIT] Compiling: sum (1000 calls)
[JIT] Generated 256 bytes of native code
[JIT] Compiling: compute (1000 calls)
[JIT] Generated 512 bytes of native code
Result: 4950
```

## Baseline JIT (x86-64)

### Code Generation Strategy

- Template-based (fixed pattern per instruction)
- Simple register allocation (maintain stack-based model)
- System V AMD64 ABI compliant (Linux/macOS)

### x86-64 Register Convention

```
rax     : Return value / scratch
rcx     : 4th argument / scratch
rdx     : 3rd argument / scratch
rbx     : Callee-saved
rsp     : Stack pointer
rbp     : Frame pointer (callee-saved)
rsi     : 2nd argument
rdi     : 1st argument
r8-r9   : 5th-6th arguments
r10-r11 : Caller-saved temporaries
r12-r15 : Callee-saved
```

### Mica JIT Register Usage (x86-64)

```
r12     : VM state pointer
r13     : Value stack pointer
r14     : Locals base pointer
r15     : Constants pool pointer
rax     : Temporary 0
rcx     : Temporary 1
```

### Code Generation Example (ADD_SMI_SMI)

```asm
; Pop two values, add, push result
mov rax, [r13 - 16]      ; load a (tag)
mov rcx, [r13 - 8]       ; load a (payload)
sub r13, 16              ; pop a
mov rdx, [r13 - 16]      ; load b (tag)
mov rsi, [r13 - 8]       ; load b (payload)
sub r13, 16              ; pop b
; Check both are integers
cmp rax, TAG_INT
jne slow_path
cmp rdx, TAG_INT
jne slow_path
; Integer addition
add rcx, rsi
; Push result
mov [r13], rax           ; tag (TAG_INT)
mov [r13 + 8], rcx       ; payload
add r13, 16
```

### Supported Operations (x86-64)

- **Stack**: PushInt, PushFloat, PushTrue, PushFalse, PushNil, Pop
- **Locals**: LoadLocal, StoreLocal
- **Arithmetic**: Add, Sub, Mul, Div (integer variants included)
- **Comparison**: Lt, Le, Gt, Ge, Eq, Ne (integer variants included)
- **Control Flow**: Jmp, JmpIfTrue, JmpIfFalse, Ret

### Conditional Compilation

```rust
#[cfg(all(target_arch = "x86_64", feature = "jit"))]
mod x86_64;

#[cfg(all(target_arch = "x86_64", feature = "jit"))]
mod compiler_x86_64;
```

## Performance Considerations

- Quickening provides 1.5-2x speedup for type-stable code
- Inline caches provide 3-5x speedup for property access
- JIT provides 3-10x overall speedup compared to interpreter
- Target: 3x+ improvement over v1 interpreter in microbenchmarks
