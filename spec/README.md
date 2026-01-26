---
title: mocaVM Specification
version: 0.1.0
---

# mocaVM Specification

This directory contains the technical specifications for mocaVM, the moca bytecode virtual machine.

## Documents

| Document | Description |
|----------|-------------|
| [vm-core.md](vm-core.md) | Value types, instruction set, Verifier, StackMap, GC |
| [c-api.md](c-api.md) | C API for embedding, bytecode serialization |
| [testing.md](testing.md) | Snapshot testing infrastructure |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Host Application (C/C++)                 │
├─────────────────────────────────────────────────────────────┤
│                        C FFI Layer                           │
│   moca_vm_new(), moca_call(), moca_push_*(), moca_to_*()    │
├─────────────────────────────────────────────────────────────┤
│                        Rust Core                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │  Verifier   │  │  Interpreter│  │  GC (Precise, STW)  │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                   Bytecode / StackMap                    ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

## Quick Reference

### Value Types

```
Value = I64(i64) | F64(f64) | Bool(bool) | Ref(GcRef) | Null
```

### Core Instructions

| Category | Instructions |
|----------|-------------|
| Constants | `CONST`, `GETL`, `SETL` |
| Stack | `POP`, `DUP` |
| Arithmetic | `ADD_I64`, `SUB_I64`, `MUL_I64`, `DIV_I64`, `ADD_F64`, `SUB_F64`, `MUL_F64`, `DIV_F64` |
| Comparison | `EQ`, `LT_I64`, `LT_F64` |
| Control | `JMP`, `JMP_IF_TRUE`, `JMP_IF_FALSE` |
| Calls | `CALL`, `RET` |
| Heap | `NEW`, `GETF`, `SETF` |

### C API Example

```c
#include <moca.h>

int main() {
    moca_vm *vm = moca_vm_new();

    moca_load_file(vm, "program.mocac");

    moca_push_i64(vm, 42);
    moca_call(vm, "process", 1);

    int64_t result = moca_to_i64(vm, -1);

    moca_vm_free(vm);
    return 0;
}
```

## Files

| Path | Description |
|------|-------------|
| `include/moca.h` | Generated C header |
| `src/ffi/` | FFI implementation |
| `src/vm/` | VM core implementation |
| `src/vm/bytecode.rs` | Bytecode serialization |
| `src/vm/verifier.rs` | Bytecode verifier |
| `src/vm/stackmap.rs` | StackMap data structures |
| `tests/c/` | C test suite |
