---
title: VM Specification
description: 仮想マシンアーキテクチャの仕様。バイトコード命令セット、64ビットタグ付き値表現、Mark-Sweep GC を定義。
---

# Moca VM Specification

This document defines the Virtual Machine architecture including bytecode, value representation, and garbage collection.

## Architecture Overview

- Stack-based VM
- 64-bit tagged values
- Frame-based call stack
- Mark-Sweep garbage collection

## Value Representation (64-bit Tagged Pointer)

```
Lower 3 bits    Type
-----------    ----
000            PTR (heap object)
001            SMI (signed 61-bit integer)
010            BOOL (true=1, false=0 in upper bits)
011            NIL
100            UNDEF
101-111        reserved
```

## Heap Object Layout

```
+----------------+
| header (64bit) |  - type_id (16bit) + gc_mark (1bit) + flags
+----------------+
| field_count    |  - Number of fields for objects
+----------------+
| fields[]       |  - Array of Values
+----------------+
```

## Call Frame

```rust
struct Frame {
    pc: usize,           // Program counter
    locals: Vec<Value>,  // Local variables
    stack_base: usize,   // Stack base pointer
}
```

## Bytecode Instruction Set

### Stack Operations

```
PUSH_INT <i64>      // Push integer
PUSH_FLOAT <f64>    // Push float
PUSH_TRUE           // Push true
PUSH_FALSE          // Push false
PUSH_STRING <idx>   // Push string from constant pool
PUSH_NIL            // Push nil
POP                 // Discard stack top
```

### Local Variables

```
LOAD_LOCAL <idx>    // Push locals[idx]
STORE_LOCAL <idx>   // Pop to locals[idx]
```

### Global Variables and Functions

```
LOAD_GLOBAL <idx>   // Push globals[idx]
CALL <argc>         // Call function with argc arguments
```

### Arithmetic

```
ADD                 // a + b
SUB                 // a - b
MUL                 // a * b
DIV                 // a / b
MOD                 // a % b
NEG                 // Unary minus
```

### Comparison

```
EQ                  // a == b
NE                  // a != b
LT                  // a < b
LE                  // a <= b
GT                  // a > b
GE                  // a >= b
```

### Logic

```
NOT                 // Logical negation
// && and || use JMP for short-circuit evaluation
```

### Control Flow

```
JMP <offset>           // Unconditional jump
JMP_IF_FALSE <offset>  // Jump if false
JMP_IF_TRUE <offset>   // Jump if true (for short-circuit)
RET                    // Return from function
```

### Object Operations

```
ALLOC_OBJ <type_id> <n_fields>  // Allocate object
READ_FIELD <field_idx>          // Read object field
WRITE_FIELD <field_idx>         // Write object field
```

### Array Operations

```
ALLOC_ARR <len>     // Allocate array
ARR_LEN             // Get array length
ARR_GET             // arr[index]
ARR_SET             // arr[index] = value
```

### Heap Slot Operations

Low-level operations for heap-allocated objects with indexed slots.

```
ALLOC_HEAP <n>      // Allocate object with n slots, push ref
ALLOC_HEAP_DYN      // Pop size, allocate dynamically, push ref
HEAP_LOAD <idx>     // Pop ref, push slots[idx]
HEAP_STORE <idx>    // Pop ref and value, store to slots[idx]
HEAP_LOAD_DYN       // Pop ref and index, push slots[index]
HEAP_STORE_DYN      // Pop ref, index, and value, store to slots[index]
```

### Vector Operations

Vectors use a 3-slot header structure: `[ptr, len, cap]`

- Slot 0: Pointer to data storage (separate heap object)
- Slot 1: Current length
- Slot 2: Capacity

Vector operations (`push`, `pop`, `vec_len`) are expanded by the compiler to low-level heap operations:

```
// vec_len(vec) expands to:
//   SlotGet(1) - get length from slot 1

// push(vec, value) expands to:
//   1. Check if len >= cap
//   2. If capacity exceeded: allocate new storage (max(8, cap*2))
//   3. Copy old data to new storage
//   4. Store value at data[len]
//   5. Increment len

// pop(vec) expands to:
//   1. Check if len > 0 (error if empty)
//   2. Decrement len
//   3. Load and return data[len]
```

### Stack Operations (Extended)

```
SWAP                // Swap top two stack elements
PICK <n>            // Copy element at depth n to top
PICK_DYN            // Pop depth, copy element at that depth to top
```

### Type Checking

```
IS_PTR              // Check if pointer
IS_SMI              // Check if SMI
IS_NIL              // Check if nil
TYPE_ID             // Get type ID
```

### Exception Handling

```
THROW                       // Throw exception
TRY_BEGIN <handler_offset>  // Begin try block
TRY_END                     // End try block
```

### Built-in Operations

```
PRINT               // Output stack top to stdout
GC_HINT <bytes>     // Hint GC about allocation
```

## Garbage Collection

### Algorithm

- Mark-Sweep (non-moving)
- Stop-The-World (STW) mark phase
- Incremental sweep (future)

### Root Set

- VM Value stack
- VM globals
- Locals on call stack

### Trigger Conditions

- Heap usage exceeds threshold
- Explicit `gc_collect()` call

### Safepoints

- Before/after function calls
- Loop back-edges
- Before large allocations

### GC Phases (Concurrent Mark)

```
1. Initial Mark (STW, short)
   - Mark objects directly referenced from roots

2. Concurrent Mark (parallel)
   - Traverse heap and mark
   - Track mutator changes via Write Barrier

3. Remark (STW, short)
   - Process changes during Concurrent Mark

4. Concurrent Sweep (parallel)
   - Free unmarked objects
```

### Write Barrier

```rust
// On field write
fn write_barrier(obj: *mut Object, field: usize, new_value: Value) {
    if gc.is_marking() && new_value.is_ptr() {
        // Snapshot-at-the-beginning barrier
        let old_value = obj.fields[field];
        if old_value.is_ptr() && !old_value.is_marked() {
            gc.mark_gray(old_value);
        }
    }
    obj.fields[field] = new_value;
}
```

## IR Architecture

### High IR

- Represents language semantics
- Reference reads/writes use dedicated instructions (`read_ref`, `write_ref`)
- Fixed hook points for GC/JIT
- Verifier checks type and stack consistency

### Mid IR

- Normalized for GC and optimization
- Clear distinction between reference and non-reference types
- Side-effect classification (pure / effectful)
- Explicit safepoint candidates

### Low IR

- Close to VM / future JIT representation
- Raw pointer operations
- Finalized object layout
- Explicit write barriers
- Stack map generation info

## Thread Model

- Each thread has independent VM instance
- Heap is shared (GC stops all threads)
- Inter-thread communication via Channel
