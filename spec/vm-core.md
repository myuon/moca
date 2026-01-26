---
title: micaVM Core Specification
version: 0.1.0
status: implemented
---

# micaVM Core Specification

## 1. Overview

micaVM v0仕様に基づくコア機能の定義。
VM実行、Verifier、GC統合、StackMap生成を含み、Luaライクな軽量・組み込み可能なランタイムの基盤を構成する。

## 2. Scope

### In Scope
- Value representation
- Instruction set
- Verifier rules
- StackMap format
- GC integration (write barriers)

### Out of Scope
- C ABI / Embed Mode → [c-api.md](c-api.md)
- Moving GC (v1以降)
- Multithreading (既存コード維持)
- Exceptions (既存コード維持)
- Closures / Multiple return values (v1以降)

## 3. Execution Flow

```
1. ソースコード → コンパイラ → バイトコード（Chunk）+ StackMap生成
2. バイトコード → Verifier → 検証OK/NG
3. 検証済みバイトコード → VM実行（Tier0: Interpreter / Tier1: JIT）
4. 実行中、Safepoint到達時にGCが介入可能
5. 実行完了 → 結果Value返却
```

## 4. Value Representation

```rust
enum Value {
    I64(i64),
    F64(f64),
    Bool(bool),
    Ref(GcRef),
    Null,
}
```

- 物理実装: Rust enum（Tagged Union）

## 5. Instruction Set

### 5.1 Constants & Locals

| Instruction | Stack Effect | Description |
|-------------|--------------|-------------|
| `CONST k` | +1 | Push constant |
| `GETL i` | +1 | Push local |
| `SETL i` | -1 | Store local (write barrier) |

### 5.2 Stack Operations

| Instruction | Stack Effect |
|-------------|--------------|
| `POP` | -1 |
| `DUP` | +1 |

### 5.3 Arithmetic (I64)

| Instruction | Stack Effect |
|-------------|--------------|
| `ADD_I64` | -1 |
| `SUB_I64` | -1 |
| `MUL_I64` | -1 |
| `DIV_I64` | -1 |

### 5.4 Arithmetic (F64)

| Instruction | Stack Effect |
|-------------|--------------|
| `ADD_F64` | -1 |
| `SUB_F64` | -1 |
| `MUL_F64` | -1 |
| `DIV_F64` | -1 |

### 5.5 Comparison

| Instruction | Stack Effect |
|-------------|--------------|
| `EQ` | -1 |
| `LT_I64` | -1 |
| `LT_F64` | -1 |

### 5.6 Control Flow

| Instruction | Stack Effect |
|-------------|--------------|
| `JMP label` | 0 |
| `JMP_IF_TRUE label` | -1 |
| `JMP_IF_FALSE label` | -1 |

### 5.7 Calls & Returns

| Instruction | Stack Effect |
|-------------|--------------|
| `CALL f, argc` | -argc + 1 |
| `RET` | -1 |

### 5.8 Heap & Objects

| Instruction | Stack Effect |
|-------------|--------------|
| `NEW type_id` | +1 |
| `GETF field` | 0 |
| `SETF field` | -2 |

### 5.9 Extended Instructions (仕様外、既存維持)

以下の命令は仕様外として削除せず維持：
- Exception: `Throw`, `TryBegin`, `TryEnd`
- Threading: `ThreadSpawn`, `ChannelCreate`, `ChannelSend`, `ChannelRecv`, `ThreadJoin`
- String/Array operations
- Print (デバッグ用)

## 6. Safepoints

以下の位置で静的に定義：
- `CALL`
- `NEW`
- `AllocArray`
- Backward jumps (`JMP*` where target < pc)
- `ThreadSpawn`, `ChannelCreate`

## 7. StackMap

```rust
struct StackMapEntry {
    pc: u32,
    stack_height: u16,
    stack_ref_bits: RefBitset,   // bitset for stack slots
    locals_ref_bits: RefBitset,  // bitset for local slots
}

struct RefBitset(u64);  // supports up to 64 slots

struct FunctionStackMap {
    entries: HashMap<u32, StackMapEntry>,
}
```

- コンパイラが生成
- VMが検証

## 8. Verifier Rules

1. **Control Flow**: ジャンプ先が命令境界であること
2. **Stack Height Consistency**: 各基本ブロックのentry stack heightが一意
3. **Stack Effect Validation**: underflow/overflow なし、max_stack以内

## 9. GC

- Precise, non-moving, stop-the-world
- Write barriers: `SETL`, `SETF`

## 10. Implementation Files

| File | Description |
|------|-------------|
| `src/vm/value.rs` | Value enum definition |
| `src/vm/ops.rs` | Bytecode operations |
| `src/vm/verifier.rs` | Bytecode verifier |
| `src/vm/stackmap.rs` | StackMap data structures |
| `src/vm/vm.rs` | VM interpreter with write barriers |
| `src/vm/heap.rs` | Heap and GC |
