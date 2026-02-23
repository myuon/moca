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

Heap objects are allocated on the managed heap (linear memory) and tracked by the garbage collector.

### Linear Memory Architecture

The heap uses a `Vec<u64>` as linear memory. Objects are stored contiguously with headers.

```
Linear Memory (Vec<u64>):
+-------+--------------------+------------+--------------------+-----+
| Rsv 0 | Object 0           | Free Block | Object 2           | ... |
+-------+--------------------+------------+--------------------+-----+
```

- Offset 0 is reserved (invalid/null reference)
- Objects and free blocks are interspersed

### Object Layout (in u64 words)

```
+----------------+------+------+------+------+-----+
| Header (1 word)| Tag0 | Val0 | Tag1 | Val1 | ... |
+----------------+------+------+------+------+-----+
```

- Each slot is 2 words: tag + payload (see Value encoding)
- Total object size = 1 + 2 × slot_count words

### Header Layout (64 bits)

```
+--------+------+------------------+-------------------+
| marked | free | slot_count (32)  | reserved (30)     |
| 1 bit  | 1 bit| 32 bits          | 30 bits           |
+--------+------+------------------+-------------------+
```

- Bit 63: marked flag for GC
- Bit 62: free flag (1 = free block in free list, 0 = allocated)
- Bits 30-61: slot count (max 2^32 - 1 slots)
- Bits 0-29: reserved for future use

### Value Encoding (for slots)

Each Value is stored as 2 u64 words: [tag, payload]

| Tag | Type | Payload |
|-----|------|---------|
| 0 | I64 | 64-bit signed integer |
| 1 | F64 | IEEE 754 double bits |
| 2 | Bool | 0 or 1 |
| 3 | Null | 0 |
| 4 | Ref | Offset into linear memory |

### Free List Management

Free blocks are embedded within linear memory:

```
Free Block Layout:
+----------------+----------------+
| Header         | Next Free Ptr  |
| (free=1, size) | (offset or 0)  |
+----------------+----------------+
```

- First-fit allocation algorithm
- Block splitting when free block is larger than needed
- Minimum free block size: 2 words

### GcRef Structure

```rust
struct GcRef {
    index: usize,  // Offset in words (u64 units) into linear memory
}
```

- Offset 0 is reserved as invalid/null reference
- GcRef points to the header word of an object

### HeapObject (View Structure)

```rust
struct HeapObject {
    marked: bool,        // GC mark flag
    slots: Vec<Value>,   // Parsed values from memory
}
```

HeapObject is constructed on-demand by parsing linear memory. It is a read-only view.

All heap-allocated data (arrays, strings, structs, vectors) use this unified slot-based format.

**Note:** Key-value maps use the stdlib HashMap implementation (see `map_new_any()` and related functions).

**Key Design**: Length is derived from `slots.len()`, not stored redundantly.

#### Array Layout
```
Array [1, 2, 3] → slots: [1, 2, 3]
- len = slots.len() = 3
- arr[i] = slots[i]
```

#### String Layout
```
"hello" → slots: [104, 101, 108, 108, 111]  // Unicode code points
- len = slots.len() = 5
- str[i] = slots[i] (returns character code)
```

Strings are stored as arrays of Unicode code points (i64 values). This allows:
- O(1) length access via `slots.len()`
- O(1) character access via index
- Full Unicode support (not just ASCII)

#### Struct Layout
```
Point { x: 10, y: 20 } → slots: [10, 20]
- field_count = slots.len() = 2
- p.x = slots[0], p.y = slots[1]
```

#### Vector Layout
```
Vector<T> → slots: [ptr, len, cap]
- slots[0]: Pointer to data storage (separate heap object)
- slots[1]: Current length
- slots[2]: Capacity
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

The VM uses **typed opcodes** following WASM conventions. See [spec-typed-opcodes.md](spec-typed-opcodes.md) for the complete instruction reference.

### Overview

Instructions are type-specific (e.g., `I64Add` for integer addition, `F64Add` for float addition). This enables:
- Efficient execution without runtime type checks
- Clear semantics for verification and JIT compilation

### Constants

```
I64Const <i64>      // Push 64-bit integer
F64Const <f64>      // Push 64-bit float
I32Const <i32>      // Push 32-bit integer (used for booleans)
RefNull             // Push null reference
StringConst <idx>   // Push string from constant pool
```

### Local Variables

```
LocalGet <idx>      // Push locals[idx]
LocalSet <idx>      // Pop to locals[idx]
```

### Stack Operations

```
Drop                // Discard stack top
Dup                 // Duplicate stack top
Pick <n>            // Copy n-th element to top
```

### Arithmetic (Type-Specific)

```
I64Add, I64Sub, I64Mul, I64DivS, I64RemS, I64Neg  // 64-bit integer
F64Add, F64Sub, F64Mul, F64Div, F64Neg            // 64-bit float
I32Add, I32Sub, I32Mul, I32DivS, I32RemS, I32Eqz  // 32-bit integer
```

### Comparison (Returns i32 boolean)

```
I64Eq, I64Ne, I64LtS, I64LeS, I64GtS, I64GeS      // 64-bit integer
F64Eq, F64Ne, F64Lt, F64Le, F64Gt, F64Ge          // 64-bit float
RefEq, RefIsNull                                   // Reference
```

### Control Flow

```
Jmp <offset>        // Unconditional jump
BrIfFalse <offset>  // Jump if i32 == 0
BrIf <offset>       // Jump if i32 != 0
Call <idx> <argc>   // Call function
Ret                 // Return from function
```

### Heap Operations

```
HeapAlloc <n>       // Allocate object with n slots, push ref
HeapAllocDyn        // Pop size, allocate dynamically, push ref
HeapLoad <idx>      // Pop ref, push slots[idx]
HeapStore <idx>     // Pop ref and value, store to slots[idx]
HeapLoadDyn         // Pop ref and index, push slots[index]
HeapStoreDyn        // Pop ref, index, and value, store to slots[index]
ArrayLen            // Get array length
```

### Vector Operations

Vectors use a 3-slot structure: `[ptr, len, cap]`

- `slots[0]`: Pointer to data storage (separate heap object)
- `slots[1]`: Current length
- `slots[2]`: Capacity

Vector operations use a combination of builtins and stdlib functions:

```
// vec_len(vec) - builtin
//   HeapLoad(1) - get length from slot 1

// vec_capacity(vec) - builtin
//   HeapLoad(2) - get capacity from slot 2

// vec_push(vec, value) - calls vec_push_any from std/prelude.mc
//   1. Check if len >= cap
//   2. If capacity exceeded: allocate new storage (max(8, cap*2))
//   3. Copy old data to new storage
//   4. Store value at data[len]
//   5. Increment len

// vec_pop(vec) - calls vec_pop_any from std/prelude.mc
//   1. Check if len > 0 (error if empty)
//   2. Decrement len
//   3. Load and return data[len]

// vec_get(vec, index) - calls vec_get_any from std/prelude.mc
//   Load data[index] via __heap_load intrinsic

// vec_set(vec, index, value) - calls vec_set_any from std/prelude.mc
//   Store value at data[index] via __heap_store intrinsic
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
PRINT_DEBUG         // Debug output stack top to stdout
GC_HINT <bytes>     // Hint GC about allocation
```

### Hostcall Operations

```
HOSTCALL <num> <argc>  // Execute host call
```

#### Hostcall Numbers

| Number | Name    | Arguments                 | Return Value                 |
|--------|---------|---------------------------|------------------------------|
| 1      | write   | fd, buf (string), count   | bytes written or error       |
| 2      | open    | path (string), flags      | fd (>=3) or error            |
| 3      | close   | fd                        | 0 on success or error        |
| 4      | read    | fd, count                 | string (heap ref) or error   |
| 5      | socket  | domain, type              | socket fd (>=3) or error     |
| 6      | connect | fd, host (string), port   | 0 on success or error        |
| 7      | bind    | fd, host (string), port   | 0 on success or error        |
| 8      | listen  | fd, backlog               | 0 on success or error        |
| 9      | accept  | fd                        | client fd (>=3) or error     |
| 10     | time    | (none)                    | epoch seconds (int)          |
| 11     | time_nanos | (none)                 | epoch nanoseconds (int)      |

#### Error Codes

| Value | Name            | Description                    |
|-------|-----------------|--------------------------------|
| -1    | EBADF           | Bad file descriptor            |
| -2    | ENOENT          | No such file or directory      |
| -3    | EACCES          | Permission denied              |
| -4    | ECONNREFUSED    | Connection refused             |
| -5    | ETIMEDOUT       | Connection timed out           |
| -6    | EAFNOSUPPORT    | Address family not supported   |
| -7    | ESOCKTNOSUPPORT | Socket type not supported      |

#### Open Flags

| Value | Name     | Description                    |
|-------|----------|--------------------------------|
| 0     | O_RDONLY | Read only                      |
| 1     | O_WRONLY | Write only                     |
| 64    | O_CREAT  | Create file if not exists      |
| 512   | O_TRUNC  | Truncate existing file         |

Flags can be combined with bitwise OR (e.g., `1 | 64 | 512` = 577).

#### Socket Constants

| Value | Name        | Description                    |
|-------|-------------|--------------------------------|
| 2     | AF_INET     | IPv4 address family            |
| 1     | SOCK_STREAM | TCP socket type                |

#### open Hostcall

```
hostcall_open(path: string, flags: int) -> int
```

- **path**: File path (relative or absolute)
- **flags**: Open flags (O_WRONLY, O_CREAT, O_TRUNC)
- **Returns**: File descriptor (>=3) on success, or negative error code

**Example:**
```moca
let fd = hostcall_open("output.txt", 1 | 64 | 512);  // O_WRONLY | O_CREAT | O_TRUNC
if fd < 0 {
    print("Failed to open file");
}
```

#### write Hostcall

```
hostcall_write(fd: int, buf: string, count: int) -> int
```

- **fd**: File descriptor (1 = stdout, 2 = stderr, >=3 = file)
- **buf**: String buffer to write
- **count**: Number of bytes to write (truncated to string length if larger)
- **Returns**: Number of bytes written, or negative error code

**Example:**
```moca
// Write to stdout
hostcall_write(1, "hello", 5);

// Write to file
let fd = hostcall_open("test.txt", 577);
hostcall_write(fd, "content", 7);
hostcall_close(fd);
```

#### close Hostcall

```
hostcall_close(fd: int) -> int
```

- **fd**: File descriptor to close (must be >=3)
- **Returns**: 0 on success, or negative error code

**Note:** fd=0, 1, 2 (stdin, stdout, stderr) cannot be closed and will return EBADF.

#### read Hostcall

```
hostcall_read(fd: int, count: int) -> string | int
```

- **fd**: File descriptor to read from (must be >=3, opened with O_RDONLY)
- **count**: Maximum number of bytes to read
- **Returns**: String (heap reference) on success, or negative error code

**Note:** Returns empty string at EOF. fd=0, 1, 2 cannot be read from.

**Example:**
```moca
// Read from file
let fd = hostcall_open("input.txt", 0);  // O_RDONLY
let content = hostcall_read(fd, 1024);   // Read up to 1024 bytes
hostcall_close(fd);
```

#### socket Hostcall

```
hostcall_socket(domain: int, type: int) -> int
```

- **domain**: Address family (AF_INET = 2 for IPv4)
- **type**: Socket type (SOCK_STREAM = 1 for TCP)
- **Returns**: Socket file descriptor (>=3) on success, or negative error code

**Note:** Only AF_INET (IPv4) and SOCK_STREAM (TCP) are currently supported.

**Example:**
```moca
let fd = hostcall_socket(2, 1);  // AF_INET, SOCK_STREAM
if fd < 0 {
    print("Failed to create socket");
}
```

#### connect Hostcall

```
hostcall_connect(fd: int, host: string, port: int) -> int
```

- **fd**: Socket file descriptor (from hostcall_socket)
- **host**: Hostname or IP address
- **port**: Port number
- **Returns**: 0 on success, or negative error code

**Example:**
```moca
let fd = hostcall_socket(2, 1);  // AF_INET, SOCK_STREAM
let result = hostcall_connect(fd, "example.com", 80);
if result < 0 {
    print("Connection failed");
}
```

#### Socket I/O

Once connected, sockets can use the same `read`, `write`, and `close` hostcalls as files:

```moca
// Create and connect socket
let fd = hostcall_socket(2, 1);
hostcall_connect(fd, "example.com", 80);

// Send HTTP request
let request = "GET / HTTP/1.0\r\nHost: example.com\r\n\r\n";
hostcall_write(fd, request, 38);

// Read response
let response = hostcall_read(fd, 4096);

// Close socket
hostcall_close(fd);
```

#### time Hostcall

```
hostcall_time() -> int
```

- **Returns**: Current time as Unix epoch seconds (i64)

Uses `std::time::SystemTime::now()` internally.

#### time_nanos Hostcall

```
hostcall_time_nanos() -> int
```

- **Returns**: Current time as Unix epoch nanoseconds (i64, valid until ~2262)

Uses `std::time::SystemTime::now()` internally.

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
