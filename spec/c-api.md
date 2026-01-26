---
title: micaVM C API Specification
version: 0.1.0
status: implemented
---

# micaVM C API Specification

## 1. Overview

micaVMのC言語APIを定義。ホストアプリケーションからVMを組み込み利用可能にする。
Luaライクな軽量組み込みランタイムとして、C/C++アプリケーションへの統合を実現する。

## 2. Scope

### In Scope
- VM lifecycle (create, free)
- Stack operations (push, pop, type checks)
- Function calls (mica functions, host functions)
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
#include <mica.h>

int main() {
    // 1. Create VM instance
    mica_vm *vm = mica_vm_new();

    // 2. Load bytecode
    mica_result res = mica_load_chunk(vm, bytecode, bytecode_len);
    if (res != MICA_RESULT_OK) {
        printf("Load error: %s\n", mica_get_error(vm));
        mica_vm_free(vm);
        return 1;
    }

    // 3. Call function
    mica_push_i64(vm, 42);  // argument
    res = mica_call(vm, "my_function", 1);  // 1 argument
    if (res != MICA_RESULT_OK) {
        printf("Call error: %s\n", mica_get_error(vm));
    }

    // 4. Get result
    int64_t result = mica_to_i64(vm, -1);
    mica_pop(vm, 1);

    // 5. Cleanup
    mica_vm_free(vm);
    return 0;
}
```

### 3.2 Host Function Registration

```c
// Host function: int add(int a, int b)
mica_result host_add(mica_vm *vm) {
    int64_t a = mica_to_i64(vm, 0);
    int64_t b = mica_to_i64(vm, 1);
    mica_pop(vm, 2);
    mica_push_i64(vm, a + b);
    return MICA_RESULT_OK;
}

// Registration
mica_register_function(vm, "add", host_add, 2);
```

## 4. API Reference

### 4.1 Result Codes

```c
typedef enum {
    MICA_RESULT_OK = 0,
    MICA_RESULT_ERROR_RUNTIME = 1,      // Runtime error
    MICA_RESULT_ERROR_TYPE = 2,         // Type mismatch
    MICA_RESULT_ERROR_VERIFY = 3,       // Bytecode verification failed
    MICA_RESULT_ERROR_MEMORY = 4,       // Out of memory
    MICA_RESULT_ERROR_INVALID_ARG = 5,  // Invalid argument
    MICA_RESULT_ERROR_NOT_FOUND = 6,    // Function/global not found
} MicaResult;
```

### 4.2 Opaque Types

```c
typedef struct MicaVm MicaVm;  // VM instance (opaque)
```

### 4.3 VM Lifecycle

```c
// Create new VM instance
MicaVm *mica_vm_new(void);

// Free VM instance
void mica_vm_free(MicaVm *vm);

// Configuration
void mica_set_memory_limit(MicaVm *vm, size_t bytes);
void mica_set_error_callback(MicaVm *vm, MicaErrorFn callback, void *userdata);

// Check if bytecode is loaded
bool mica_has_chunk(MicaVm *vm);
```

### 4.4 Bytecode Loading

```c
// Load from memory
MicaResult mica_load_chunk(MicaVm *vm, const uint8_t *data, size_t len);

// Load from file
MicaResult mica_load_file(MicaVm *vm, const char *path);

// Save to file
MicaResult mica_save_file(MicaVm *vm, const char *path);
```

### 4.5 Stack Operations

```c
// Push values
void mica_push_null(MicaVm *vm);
void mica_push_bool(MicaVm *vm, bool value);
void mica_push_i64(MicaVm *vm, int64_t value);
void mica_push_f64(MicaVm *vm, double value);
void mica_push_string(MicaVm *vm, const char *str, size_t len);

// Type checking (index: positive = from bottom, negative = from top)
bool mica_is_null(MicaVm *vm, int32_t index);
bool mica_is_bool(MicaVm *vm, int32_t index);
bool mica_is_i64(MicaVm *vm, int32_t index);
bool mica_is_f64(MicaVm *vm, int32_t index);
bool mica_is_string(MicaVm *vm, int32_t index);
bool mica_is_ref(MicaVm *vm, int32_t index);

// Get values
bool mica_to_bool(MicaVm *vm, int32_t index);
int64_t mica_to_i64(MicaVm *vm, int32_t index);
double mica_to_f64(MicaVm *vm, int32_t index);
const char *mica_to_string(MicaVm *vm, int32_t index, size_t *len);

// Stack manipulation
void mica_pop(MicaVm *vm, int32_t count);
int32_t mica_get_top(MicaVm *vm);
void mica_set_top(MicaVm *vm, int32_t index);
```

### 4.6 Function Calls

```c
// Call mica function by name
MicaResult mica_call(MicaVm *vm, const char *func_name, int32_t nargs);

// Protected call (catches errors)
MicaResult mica_pcall(MicaVm *vm, const char *func_name, int32_t nargs);
```

### 4.7 Host Function Registration

```c
// Host function signature
typedef MicaResult (*MicaCFunc)(MicaVm *vm);

// Register host function
MicaResult mica_register_function(MicaVm *vm, const char *name,
                                   MicaCFunc func, int32_t arity);
```

### 4.8 Globals

```c
// Set global (pops value from stack)
MicaResult mica_set_global(MicaVm *vm, const char *name);

// Get global (pushes value to stack)
MicaResult mica_get_global(MicaVm *vm, const char *name);
```

### 4.9 Error Handling

```c
// Get last error message
const char *mica_get_error(const MicaVm *vm);

// Check if error is pending
bool mica_has_error(const MicaVm *vm);

// Clear error
void mica_clear_error(MicaVm *vm);

// Error callback type
typedef void (*MicaErrorFn)(const char *message, void *userdata);
```

### 4.10 Version Info

```c
const char *mica_version(void);      // e.g., "0.1.0"
uint32_t mica_version_major(void);
uint32_t mica_version_minor(void);
uint32_t mica_version_patch(void);
```

## 5. Memory Model

### 5.1 Ownership Rules

1. **VM owns all heap objects**: Strings, arrays, objects allocated by VM
2. **Host gets handles**: Stack indices (read-only access)
3. **String lifetime**: `mica_to_string()` returns pointer valid until next GC or stack modification
4. **No host allocation**: Host cannot directly create VM objects (must use push APIs)

### 5.2 GC Integration

- GC may run at any safepoint during `mica_call`/`mica_pcall`
- Stack values are GC roots
- Host must not hold raw pointers across calls

## 6. Bytecode Serialization Format

### 6.1 Header

```
Magic: "MICA" (4 bytes)
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
name = "mica"
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
# target/release/libmica.so   (shared library)
# target/release/libmica.a    (static library)
# include/mica.h              (C header)
```

### 7.3 Linking C Programs

```bash
# Compile and link
gcc -o myapp myapp.c -L./target/release -lmica -Wl,-rpath,./target/release
```

## 8. Implementation Files

| File | Description |
|------|-------------|
| `src/ffi/mod.rs` | FFI module entry, version info |
| `src/ffi/types.rs` | FFI types (MicaResult, MicaVm, VmWrapper) |
| `src/ffi/vm_ffi.rs` | VM lifecycle functions |
| `src/ffi/stack.rs` | Stack operations |
| `src/ffi/call.rs` | Function calls, host functions, globals |
| `src/ffi/error.rs` | Error handling |
| `src/ffi/load.rs` | Bytecode loading |
| `src/vm/bytecode.rs` | Bytecode serialization |
| `include/mica.h` | Generated C header |
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
