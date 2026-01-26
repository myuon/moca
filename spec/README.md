# BCVM v0 Specification

This directory contains the technical specifications for the mica bytecode virtual machine (BCVM v0).

## Overview

BCVM v0 is designed as a lightweight, embeddable runtime similar to Lua, supporting:
- Stack-based bytecode execution
- Precise garbage collection
- C FFI for embedding in host applications

## Specification Documents

| Document | Description | Status |
|----------|-------------|--------|
| [Phase 1: Core VM](phase1-core.md) | Value types, instruction set, Verifier, StackMap, GC | Implemented |
| [Phase 2: C ABI](phase2-c-abi.md) | C API for embedding, bytecode serialization | Implemented |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Host Application (C/C++)                 │
├─────────────────────────────────────────────────────────────┤
│                        C FFI Layer                           │
│   mica_vm_new(), mica_call(), mica_push_*(), mica_to_*()   │
├─────────────────────────────────────────────────────────────┤
│                        Rust Core                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │  Verifier   │  │  Interpreter│  │  GC (Precise, STW)  │  │
│  │             │  │   (Tier 0)  │  │                     │  │
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
#include <mica.h>

int main() {
    mica_vm *vm = mica_vm_new();

    // Load bytecode
    mica_load_file(vm, "program.micac");

    // Call function with argument
    mica_push_i64(vm, 42);
    mica_call(vm, "process", 1);

    // Get result
    int64_t result = mica_to_i64(vm, -1);

    mica_vm_free(vm);
    return 0;
}
```

## Implementation Status

### Phase 1 (Core VM) - Complete

- [x] Value type refactoring (I64, F64, Bool, Ref, Null)
- [x] Instruction set alignment with v0 spec
- [x] Bytecode Verifier (CFG, stack height validation)
- [x] StackMap data structures
- [x] Write barriers for GC

### Phase 2 (C ABI) - Complete

- [x] FFI module structure
- [x] VM lifecycle (new/free)
- [x] Stack operations (push/pop/is_*/to_*)
- [x] Function calls (call/pcall/register)
- [x] Globals API
- [x] Error handling
- [x] Bytecode serialization
- [x] Header generation (cbindgen)
- [x] Library builds (staticlib/cdylib)
- [x] C test suite

## Files

- `include/mica.h` - Generated C header
- `src/ffi/` - FFI implementation
- `src/vm/` - VM core implementation
- `src/vm/bytecode.rs` - Bytecode serialization
- `src/vm/verifier.rs` - Bytecode verifier
- `src/vm/stackmap.rs` - StackMap data structures
- `tests/c/` - C test suite
