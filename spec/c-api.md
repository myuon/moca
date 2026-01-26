---
title: mocaVM C API Specification
version: 0.1.0
status: implemented
---

# mocaVM C API Specification

## 1. Overview

mocaVMのC言語APIを定義。ホストアプリケーションからVMを組み込み利用可能にする。
Luaライクな軽量組み込みランタイムとして、C/C++アプリケーションへの統合を実現する。

## 2. Scope

### In Scope
- VM lifecycle (create, free)
- Stack operations (push, pop, type checks)
- Function calls (moca functions, host functions)
- Bytecode loading/saving
- Error handling
- Globals API

### Out of Scope
- Moving GC (v1以降)
- Direct host memory access (VMが全オブジェクトを所有)
- Async/await support (同期APIのみ)
- Hot reloading (バイトコード差し替えは再初期化が必要)
- Thread-safe API (単一スレッドからの呼び出しを前提)

## 3. Basic Usage

### 3.1 Embedding Flow

```c
#include <moca.h>

int main() {
    // 1. Create VM instance
    moca_vm *vm = moca_vm_new();

    // 2. Load bytecode
    moca_result res = moca_load_chunk(vm, bytecode, bytecode_len);
    if (res != MOCA_RESULT_OK) {
        printf("Load error: %s\n", moca_get_error(vm));
        moca_vm_free(vm);
        return 1;
    }

    // 3. Call function
    moca_push_i64(vm, 42);  // argument
    res = moca_call(vm, "my_function", 1);  // 1 argument
    if (res != MOCA_RESULT_OK) {
        printf("Call error: %s\n", moca_get_error(vm));
    }

    // 4. Get result
    int64_t result = moca_to_i64(vm, -1);
    moca_pop(vm, 1);

    // 5. Cleanup
    moca_vm_free(vm);
    return 0;
}
```

### 3.2 Host Function Registration

```c
// Host function: int add(int a, int b)
moca_result host_add(moca_vm *vm) {
    int64_t a = moca_to_i64(vm, 0);
    int64_t b = moca_to_i64(vm, 1);
    moca_pop(vm, 2);
    moca_push_i64(vm, a + b);
    return MOCA_RESULT_OK;
}

// Registration
moca_register_function(vm, "add", host_add, 2);
```

## 4. API Reference

### 4.1 Result Codes

```c
typedef enum {
    MOCA_RESULT_OK = 0,
    MOCA_RESULT_ERROR_RUNTIME = 1,      // Runtime error
    MOCA_RESULT_ERROR_TYPE = 2,         // Type mismatch
    MOCA_RESULT_ERROR_VERIFY = 3,       // Bytecode verification failed
    MOCA_RESULT_ERROR_MEMORY = 4,       // Out of memory
    MOCA_RESULT_ERROR_INVALID_ARG = 5,  // Invalid argument
    MOCA_RESULT_ERROR_NOT_FOUND = 6,    // Function/global not found
} MocaResult;
```

### 4.2 Opaque Types

```c
typedef struct MocaVm MocaVm;  // VM instance (opaque)
```

### 4.3 VM Lifecycle

```c
// Create new VM instance
MocaVm *moca_vm_new(void);

// Free VM instance
void moca_vm_free(MocaVm *vm);

// Configuration
void moca_set_memory_limit(MocaVm *vm, size_t bytes);
void moca_set_error_callback(MocaVm *vm, MocaErrorFn callback, void *userdata);

// Check if bytecode is loaded
bool moca_has_chunk(MocaVm *vm);
```

### 4.4 Bytecode Loading

```c
// Load from memory
MocaResult moca_load_chunk(MocaVm *vm, const uint8_t *data, size_t len);

// Load from file
MocaResult moca_load_file(MocaVm *vm, const char *path);

// Save to file
MocaResult moca_save_file(MocaVm *vm, const char *path);
```

### 4.5 Stack Operations

```c
// Push values
void moca_push_null(MocaVm *vm);
void moca_push_bool(MocaVm *vm, bool value);
void moca_push_i64(MocaVm *vm, int64_t value);
void moca_push_f64(MocaVm *vm, double value);
void moca_push_string(MocaVm *vm, const char *str, size_t len);

// Type checking (index: positive = from bottom, negative = from top)
bool moca_is_null(MocaVm *vm, int32_t index);
bool moca_is_bool(MocaVm *vm, int32_t index);
bool moca_is_i64(MocaVm *vm, int32_t index);
bool moca_is_f64(MocaVm *vm, int32_t index);
bool moca_is_string(MocaVm *vm, int32_t index);
bool moca_is_ref(MocaVm *vm, int32_t index);

// Get values
bool moca_to_bool(MocaVm *vm, int32_t index);
int64_t moca_to_i64(MocaVm *vm, int32_t index);
double moca_to_f64(MocaVm *vm, int32_t index);
const char *moca_to_string(MocaVm *vm, int32_t index, size_t *len);

// Stack manipulation
void moca_pop(MocaVm *vm, int32_t count);
int32_t moca_get_top(MocaVm *vm);
void moca_set_top(MocaVm *vm, int32_t index);
```

### 4.6 Function Calls

```c
// Call moca function by name
MocaResult moca_call(MocaVm *vm, const char *func_name, int32_t nargs);

// Protected call (catches errors)
MocaResult moca_pcall(MocaVm *vm, const char *func_name, int32_t nargs);
```

### 4.7 Host Function Registration

```c
// Host function signature
typedef MocaResult (*MocaCFunc)(MocaVm *vm);

// Register host function
MocaResult moca_register_function(MocaVm *vm, const char *name,
                                   MocaCFunc func, int32_t arity);
```

### 4.8 Globals

```c
// Set global (pops value from stack)
MocaResult moca_set_global(MocaVm *vm, const char *name);

// Get global (pushes value to stack)
MocaResult moca_get_global(MocaVm *vm, const char *name);
```

### 4.9 Error Handling

```c
// Get last error message
const char *moca_get_error(const MocaVm *vm);

// Check if error is pending
bool moca_has_error(const MocaVm *vm);

// Clear error
void moca_clear_error(MocaVm *vm);

// Error callback type
typedef void (*MocaErrorFn)(const char *message, void *userdata);
```

### 4.10 Version Info

```c
const char *moca_version(void);      // e.g., "0.1.0"
uint32_t moca_version_major(void);
uint32_t moca_version_minor(void);
uint32_t moca_version_patch(void);
```

## 5. Memory Model

### 5.1 Ownership Rules

1. **VM owns all heap objects**: Strings, arrays, objects allocated by VM
2. **Host gets handles**: Stack indices (read-only access)
3. **String lifetime**: `moca_to_string()` returns pointer valid until next GC or stack modification
4. **No host allocation**: Host cannot directly create VM objects (must use push APIs)

### 5.2 GC Integration

- GC may run at any safepoint during `moca_call`/`moca_pcall`
- Stack values are GC roots
- Host must not hold raw pointers across calls

## 6. Bytecode Serialization Format

### 6.1 Header

```
Magic: "MOCA" (4 bytes)
Version: u32 (format version = 1)
```

### 6.2 Layout

```
[Header]
[String Pool]
  count: u32
  for each string:
    len: u32
    data: [u8; len]
[Functions]
  count: u32
  for each function:
    name_len: u32
    name: [u8; name_len]
    arity: u32
    locals_count: u32
    code_len: u32
    code: [Op serialized]
    has_stackmap: u8
    if has_stackmap:
      entry_count: u32
      for each entry:
        pc: u32
        stack_height: u16
        stack_ref_bits: u64
        locals_ref_bits: u64
[Main Function]
  (same format as function)
[Has Debug Info]: u8 (0 = no)
```

## 7. Build Configuration

### 7.1 Cargo.toml

```toml
[lib]
name = "moca"
path = "src/lib.rs"
crate-type = ["cdylib", "staticlib", "rlib"]

[build-dependencies]
cbindgen = "0.28"
```

### 7.2 Building

```bash
# Build release library
cargo build --release

# Outputs:
# target/release/libmoca.so   (shared library)
# target/release/libmoca.a    (static library)
# include/moca.h              (C header)
```

### 7.3 Linking C Programs

```bash
# Compile and link
gcc -o myapp myapp.c -L./target/release -lmoca -Wl,-rpath,./target/release
```

## 8. Implementation Files

| File | Description |
|------|-------------|
| `src/ffi/mod.rs` | FFI module entry, version info |
| `src/ffi/types.rs` | FFI types (MocaResult, MocaVm, VmWrapper) |
| `src/ffi/vm_ffi.rs` | VM lifecycle functions |
| `src/ffi/stack.rs` | Stack operations |
| `src/ffi/call.rs` | Function calls, host functions, globals |
| `src/ffi/error.rs` | Error handling |
| `src/ffi/load.rs` | Bytecode loading |
| `src/vm/bytecode.rs` | Bytecode serialization |
| `include/moca.h` | Generated C header |
| `tests/c/test_ffi.c` | C test suite |
| `tests/c/Makefile` | C test build |

## 9. Test Suite

### Running C Tests

```bash
cd tests/c
make test
```

### Test Coverage

| Test | Description |
|------|-------------|
| test_version | Version API |
| test_vm_create_free | VM lifecycle |
| test_stack_* | Stack operations |
| test_error_* | Error handling |
| test_globals_* | Globals API |
| test_host_function_* | Host function registration |
| test_load_* | Bytecode loading |
