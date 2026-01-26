# Spec.md — BCVM v0 Phase 2: C ABI / Embed Mode

## 1. Goal

BCVM v0のC言語APIを設計・実装し、ホストアプリケーションからVMを組み込み利用可能にする。
Luaライクな軽量組み込みランタイムとして、C/C++アプリケーションへの統合を実現する。

---

## 2. Non-Goals

- **Moving GC**: v0では非対応
- **Direct host memory access**: VMが全オブジェクトを所有（ホストは不透明ハンドル経由）
- **Async/await support**: 同期APIのみ
- **Hot reloading**: バイトコード差し替えは再初期化が必要
- **Thread-safe API**: 単一スレッドからの呼び出しを前提（マルチスレッド対応はv1以降）

---

## 3. Target Users

- C/C++アプリケーション開発者（ゲームエンジン、エディタ等）
- Python/Node.js等からFFI経由で利用するユーザー（将来的なバインディング生成対応）
- 組み込みシステム開発者（軽量スクリプト実行環境として）

---

## 4. Core User Flow

### 4.1 Basic Embedding Flow

```c
#include <mica.h>

int main() {
    // 1. Create VM instance
    mica_vm *vm = mica_vm_new();

    // 2. Load bytecode
    mica_result res = mica_load_chunk(vm, bytecode, bytecode_len);
    if (res != MICA_OK) {
        printf("Load error: %s\n", mica_get_error(vm));
        mica_vm_free(vm);
        return 1;
    }

    // 3. Call function
    mica_push_i64(vm, 42);  // argument
    res = mica_call(vm, "my_function", 1);  // 1 argument
    if (res != MICA_OK) {
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

### 4.2 Host Function Registration

```c
// Host function: int add(int a, int b)
mica_result host_add(mica_vm *vm) {
    int64_t a = mica_to_i64(vm, 0);
    int64_t b = mica_to_i64(vm, 1);
    mica_push_i64(vm, a + b);
    return MICA_OK;
}

// Registration
mica_register_function(vm, "add", host_add, 2);
```

---

## 5. Inputs & Outputs

### Inputs
- **Bytecode**: シリアライズされたChunk（バイナリ形式）
- **Host functions**: C関数ポインタ + メタデータ
- **Values**: push API経由のI64/F64/Bool/String/Null

### Outputs
- **Result codes**: `MICA_OK`, `MICA_ERROR_*`
- **Error messages**: `mica_get_error()` 経由
- **Values**: pop/peek API経由

---

## 6. Tech Stack

- **言語**: Rust (core) + C (public API)
- **FFI**: `#[no_mangle]` + `extern "C"`
- **Header生成**: cbindgen (自動生成) + 手動キュレーション
- **ビルド成果物**:
  - `libmica.a` (static library)
  - `libmica.so` / `libmica.dylib` (shared library)
  - `mica.h` (public header)

---

## 7. API Design

### 7.1 Result Codes

```c
typedef enum {
    MICA_OK = 0,
    MICA_ERROR_RUNTIME = 1,      // Runtime error
    MICA_ERROR_TYPE = 2,         // Type mismatch
    MICA_ERROR_VERIFY = 3,       // Bytecode verification failed
    MICA_ERROR_MEMORY = 4,       // Out of memory
    MICA_ERROR_INVALID_ARG = 5,  // Invalid argument
    MICA_ERROR_NOT_FOUND = 6,    // Function/global not found
} mica_result;
```

### 7.2 Opaque Types

```c
typedef struct mica_vm mica_vm;           // VM instance
typedef struct mica_value mica_value;     // Value handle (for advanced use)
```

### 7.3 VM Lifecycle

```c
// Create/destroy
mica_vm *mica_vm_new(void);
void mica_vm_free(mica_vm *vm);

// Configuration (before loading)
void mica_set_memory_limit(mica_vm *vm, size_t bytes);
void mica_set_error_callback(mica_vm *vm, mica_error_fn callback, void *userdata);
```

### 7.4 Bytecode Loading

```c
// Load from memory
mica_result mica_load_chunk(mica_vm *vm, const uint8_t *data, size_t len);

// Load from file (convenience)
mica_result mica_load_file(mica_vm *vm, const char *path);
```

### 7.5 Stack Operations

```c
// Push values
void mica_push_null(mica_vm *vm);
void mica_push_bool(mica_vm *vm, bool value);
void mica_push_i64(mica_vm *vm, int64_t value);
void mica_push_f64(mica_vm *vm, double value);
void mica_push_string(mica_vm *vm, const char *str, size_t len);

// Type checking
bool mica_is_null(mica_vm *vm, int index);
bool mica_is_bool(mica_vm *vm, int index);
bool mica_is_i64(mica_vm *vm, int index);
bool mica_is_f64(mica_vm *vm, int index);
bool mica_is_string(mica_vm *vm, int index);
bool mica_is_ref(mica_vm *vm, int index);

// Get values (index: positive = from bottom, negative = from top)
bool mica_to_bool(mica_vm *vm, int index);
int64_t mica_to_i64(mica_vm *vm, int index);
double mica_to_f64(mica_vm *vm, int index);
const char *mica_to_string(mica_vm *vm, int index, size_t *len);

// Stack manipulation
void mica_pop(mica_vm *vm, int count);
int mica_get_top(mica_vm *vm);  // Get stack height
void mica_set_top(mica_vm *vm, int index);
```

### 7.6 Function Calls

```c
// Call mica function by name
// Args must be pushed before call, result is on stack after
mica_result mica_call(mica_vm *vm, const char *func_name, int nargs);

// Protected call (catches errors instead of aborting)
mica_result mica_pcall(mica_vm *vm, const char *func_name, int nargs);
```

### 7.7 Host Function Registration

```c
// Host function signature
typedef mica_result (*mica_cfunc)(mica_vm *vm);

// Register host function
mica_result mica_register_function(mica_vm *vm, const char *name,
                                    mica_cfunc func, int arity);
```

### 7.8 Globals

```c
// Set global variable
mica_result mica_set_global(mica_vm *vm, const char *name);  // pops value from stack

// Get global variable
mica_result mica_get_global(mica_vm *vm, const char *name);  // pushes value to stack
```

### 7.9 Error Handling

```c
// Get last error message (valid until next API call)
const char *mica_get_error(mica_vm *vm);

// Error callback type
typedef void (*mica_error_fn)(const char *message, void *userdata);
```

### 7.10 Version & Info

```c
// Version info
const char *mica_version(void);      // e.g., "0.1.0"
int mica_version_major(void);
int mica_version_minor(void);
int mica_version_patch(void);
```

---

## 8. Memory Model

### 8.1 Ownership Rules

1. **VM owns all heap objects**: Strings, arrays, objects allocated by VM
2. **Host gets handles**: Stack indices or opaque pointers (read-only access)
3. **String lifetime**: `mica_to_string()` returns pointer valid until next GC or stack modification
4. **No host allocation**: Host cannot directly create VM objects (must use push APIs)

### 8.2 GC Integration

- GC may run at any safepoint during `mica_call`/`mica_pcall`
- Stack values are GC roots
- Host must not hold raw pointers across calls

---

## 9. Bytecode Serialization Format

### 9.1 Header

```
Magic: "MICA" (4 bytes)
Version: u32 (format version, starts at 1)
Flags: u32 (reserved)
```

### 9.2 Chunk Layout

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
    name_idx: u32 (index into string pool)
    arity: u32
    locals_count: u32
    code_len: u32
    code: [u8; code_len]  (opcodes)
    stackmap: Option<StackMap>
[Main Function Index]: u32
```

---

## 10. Acceptance Criteria

1. [ ] `mica.h` ヘッダーが生成され、全パブリックAPIが宣言されている
2. [ ] `mica_vm_new()` / `mica_vm_free()` でVMの生成・破棄ができる
3. [ ] `mica_load_chunk()` でバイトコードをロードできる
4. [ ] Stack操作API（push/pop/is_*/to_*）が動作する
5. [ ] `mica_call()` でmica関数を呼び出し、結果を取得できる
6. [ ] `mica_register_function()` でホスト関数を登録・呼び出しできる
7. [ ] エラー発生時に `mica_get_error()` でメッセージを取得できる
8. [ ] バイトコードシリアライズ/デシリアライズが動作する
9. [ ] C言語のテストプログラムが正常に動作する
10. [ ] 静的ライブラリと共有ライブラリがビルドできる

---

## 11. Test Plan

### Unit Test 1: VM Lifecycle

```c
void test_vm_lifecycle() {
    mica_vm *vm = mica_vm_new();
    assert(vm != NULL);
    mica_vm_free(vm);
}
```

### Unit Test 2: Stack Operations

```c
void test_stack_ops() {
    mica_vm *vm = mica_vm_new();

    mica_push_i64(vm, 42);
    assert(mica_is_i64(vm, -1));
    assert(mica_to_i64(vm, -1) == 42);

    mica_push_f64(vm, 3.14);
    assert(mica_is_f64(vm, -1));

    assert(mica_get_top(vm) == 2);
    mica_pop(vm, 2);
    assert(mica_get_top(vm) == 0);

    mica_vm_free(vm);
}
```

### Unit Test 3: Host Function

```c
mica_result test_add(mica_vm *vm) {
    int64_t a = mica_to_i64(vm, 0);
    int64_t b = mica_to_i64(vm, 1);
    mica_push_i64(vm, a + b);
    return MICA_OK;
}

void test_host_function() {
    mica_vm *vm = mica_vm_new();
    mica_register_function(vm, "test_add", test_add, 2);

    // Load bytecode that calls test_add
    // ...

    mica_vm_free(vm);
}
```

### Integration Test: End-to-End

```c
void test_e2e() {
    mica_vm *vm = mica_vm_new();

    // Load bytecode for: fn add(a, b) { return a + b; }
    mica_result res = mica_load_file(vm, "test_add.micac");
    assert(res == MICA_OK);

    // Call: add(10, 20)
    mica_push_i64(vm, 10);
    mica_push_i64(vm, 20);
    res = mica_call(vm, "add", 2);
    assert(res == MICA_OK);

    // Check result
    assert(mica_to_i64(vm, -1) == 30);

    mica_vm_free(vm);
}
```

---

## 12. Implementation Tasks

### Phase 2-1: Foundation
- [ ] Create `src/ffi/mod.rs` module
- [ ] Implement `mica_vm_new()` / `mica_vm_free()`
- [ ] Setup cbindgen for header generation
- [ ] Configure Cargo for cdylib/staticlib output

### Phase 2-2: Stack API
- [ ] Implement push functions (null, bool, i64, f64, string)
- [ ] Implement type checking functions (is_*)
- [ ] Implement conversion functions (to_*)
- [ ] Implement stack manipulation (pop, get_top, set_top)

### Phase 2-3: Execution
- [ ] Implement `mica_call()` / `mica_pcall()`
- [ ] Implement `mica_register_function()`
- [ ] Implement globals API

### Phase 2-4: Bytecode Format
- [ ] Design binary serialization format
- [ ] Implement `mica_load_chunk()` (deserialize)
- [ ] Implement `mica_load_file()` (convenience)
- [ ] Add serialization to compiler output

### Phase 2-5: Error Handling
- [ ] Implement error storage in VM
- [ ] Implement `mica_get_error()`
- [ ] Implement error callback mechanism

### Phase 2-6: Build System
- [ ] Configure static library build
- [ ] Configure shared library build
- [ ] Generate and curate `mica.h`
- [ ] Create C test suite

---

## 13. Verification Strategy

### Progress Verification
- 各FFI関数実装後にC言語テストを追加・実行
- `cargo test` でRust側のユニットテストを維持

### Completion Verification
- 全Acceptance Criteriaをチェック
- C言語テストスイートが全パス
- サンプルアプリケーション（examples/embed.c）が動作

### Memory Safety
- Miriでのテスト実行（可能な範囲で）
- Valgrindでのメモリリークチェック
