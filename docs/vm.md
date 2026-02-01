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

Heap objects are allocated on the managed heap and tracked by the garbage collector.

### HeapObject Types

```rust
enum HeapObject {
    Slots(MocaSlots),    // Indexed slots (arrays, strings, structs, vectors)
}
```

**Note:** Key-value maps use the stdlib HashMap implementation (see `map_new_any()` and related functions).

### MocaSlots Layout

All slot-based types (arrays, strings, structs, vectors) use the unified `MocaSlots` format:

```
+----------------+
| header (64bit) |  - obj_type + gc_mark + flags
+----------------+
| slots: Vec<Value> |  - Variable-length array of values
+----------------+
```

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

### Array Operations

Arrays are stored as `MocaSlots` with elements directly in slots (no length prefix).

```
ALLOC_HEAP <n>      // Allocate array with n elements
ARRAY_LEN           // Get array length via slots.len()
HEAP_LOAD_DYN       // arr[index] - direct slot access
HEAP_STORE_DYN      // arr[index] = value - direct slot store
```

**Note**: `typeof(array)` returns `"slots"` (arrays and strings share the same type).

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

### Syscall Operations

```
SYSCALL <num> <argc>  // Execute system call
```

#### Syscall Numbers

| Number | Name    | Arguments                 | Return Value                 |
|--------|---------|---------------------------|------------------------------|
| 1      | write   | fd, buf (string), count   | bytes written or error       |
| 2      | open    | path (string), flags      | fd (>=3) or error            |
| 3      | close   | fd                        | 0 on success or error        |
| 4      | read    | fd, count                 | string (heap ref) or error   |
| 5      | socket  | domain, type              | socket fd (>=3) or error     |
| 6      | connect | fd, host (string), port   | 0 on success or error        |

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

#### open Syscall

```
syscall_open(path: string, flags: int) -> int
```

- **path**: File path (relative or absolute)
- **flags**: Open flags (O_WRONLY, O_CREAT, O_TRUNC)
- **Returns**: File descriptor (>=3) on success, or negative error code

**Example:**
```moca
let fd = syscall_open("output.txt", 1 | 64 | 512);  // O_WRONLY | O_CREAT | O_TRUNC
if fd < 0 {
    print("Failed to open file");
}
```

#### write Syscall

```
syscall_write(fd: int, buf: string, count: int) -> int
```

- **fd**: File descriptor (1 = stdout, 2 = stderr, >=3 = file)
- **buf**: String buffer to write
- **count**: Number of bytes to write (truncated to string length if larger)
- **Returns**: Number of bytes written, or negative error code

**Example:**
```moca
// Write to stdout
syscall_write(1, "hello", 5);

// Write to file
let fd = syscall_open("test.txt", 577);
syscall_write(fd, "content", 7);
syscall_close(fd);
```

#### close Syscall

```
syscall_close(fd: int) -> int
```

- **fd**: File descriptor to close (must be >=3)
- **Returns**: 0 on success, or negative error code

**Note:** fd=0, 1, 2 (stdin, stdout, stderr) cannot be closed and will return EBADF.

#### read Syscall

```
syscall_read(fd: int, count: int) -> string | int
```

- **fd**: File descriptor to read from (must be >=3, opened with O_RDONLY)
- **count**: Maximum number of bytes to read
- **Returns**: String (heap reference) on success, or negative error code

**Note:** Returns empty string at EOF. fd=0, 1, 2 cannot be read from.

**Example:**
```moca
// Read from file
let fd = syscall_open("input.txt", 0);  // O_RDONLY
let content = syscall_read(fd, 1024);   // Read up to 1024 bytes
syscall_close(fd);
```

#### socket Syscall

```
syscall_socket(domain: int, type: int) -> int
```

- **domain**: Address family (AF_INET = 2 for IPv4)
- **type**: Socket type (SOCK_STREAM = 1 for TCP)
- **Returns**: Socket file descriptor (>=3) on success, or negative error code

**Note:** Only AF_INET (IPv4) and SOCK_STREAM (TCP) are currently supported.

**Example:**
```moca
let fd = syscall_socket(2, 1);  // AF_INET, SOCK_STREAM
if fd < 0 {
    print("Failed to create socket");
}
```

#### connect Syscall

```
syscall_connect(fd: int, host: string, port: int) -> int
```

- **fd**: Socket file descriptor (from syscall_socket)
- **host**: Hostname or IP address
- **port**: Port number
- **Returns**: 0 on success, or negative error code

**Example:**
```moca
let fd = syscall_socket(2, 1);  // AF_INET, SOCK_STREAM
let result = syscall_connect(fd, "example.com", 80);
if result < 0 {
    print("Connection failed");
}
```

#### Socket I/O

Once connected, sockets can use the same `read`, `write`, and `close` syscalls as files:

```moca
// Create and connect socket
let fd = syscall_socket(2, 1);
syscall_connect(fd, "example.com", 80);

// Send HTTP request
let request = "GET / HTTP/1.0\r\nHost: example.com\r\n\r\n";
syscall_write(fd, request, 38);

// Read response
let response = syscall_read(fd, 4096);

// Close socket
syscall_close(fd);
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
