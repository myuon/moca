# Phase 1: BCVM v0 Core Specification

> Status: **Implemented**

## 1. Goal

BCVM v0仕様に基づき、既存VMを段階的にリファクタリングする。
コア機能（VM実行、Verifier、GC統合、StackMap生成）を実装し、Luaライクな軽量・組み込み可能なランタイムの基盤を確立する。

## 2. Non-Goals

- **C ABI / Embed Mode**: Phase 2で実装
- **Moving GC**: v0では非対応
- **Multithreading**: 仕様外として既存コード維持
- **Exceptions**: 仕様外として既存コード維持
- **Closures / Multiple return values**: v0では非対応
- **バイトコードバイナリ形式**: Phase 2で実装
- **Runtime source loading**: コンパイラ責務

## 3. Target Users

- mica言語のランタイム利用者
- 将来的にVMを組み込みたいホストアプリケーション開発者（Phase 2以降）

## 4. Core User Flow

```
1. ソースコード → コンパイラ → バイトコード（Chunk）+ StackMap生成
2. バイトコード → Verifier → 検証OK/NG
3. 検証済みバイトコード → VM実行（Tier0: Interpreter / Tier1: JIT）
4. 実行中、Safepoint到達時にGCが介入可能
5. 実行完了 → 結果Value返却
```

## 5. Inputs & Outputs

### Inputs
- **バイトコード**: コンパイラが生成したChunk（関数群、定数プール、StackMap）
- **エントリポイント**: main関数または指定関数

### Outputs
- **実行結果**: Value（I64 / F64 / Bool / Ref / Null）
- **エラー**: 検証エラー / 実行時エラー

## 6. Tech Stack

- **言語**: Rust
- **ビルド**: Cargo
- **テスト**: cargo test（既存テストスイート + 新規仕様テスト）
- **JIT**: 既存インフラ（AArch64 / x86-64）

## 7. Rules & Constraints

### 7.1 Value Representation

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
- F64追加（v0仕様拡張）

### 7.2 命令セット（v0 Core + F64拡張）

#### Constants & Locals

| Instruction | Stack Effect | Description |
|-------------|--------------|-------------|
| `CONST k` | +1 | Push constant |
| `GETL i` | +1 | Push local |
| `SETL i` | -1 | Store local (write barrier) |

#### Stack Operations

| Instruction | Stack Effect |
|-------------|--------------|
| `POP` | -1 |
| `DUP` | +1 |

#### Arithmetic (I64)

| Instruction | Stack Effect |
|-------------|--------------|
| `ADD_I64` | -1 |
| `SUB_I64` | -1 |
| `MUL_I64` | -1 |
| `DIV_I64` | -1 |

#### Arithmetic (F64) — 拡張

| Instruction | Stack Effect |
|-------------|--------------|
| `ADD_F64` | -1 |
| `SUB_F64` | -1 |
| `MUL_F64` | -1 |
| `DIV_F64` | -1 |

#### Comparison

| Instruction | Stack Effect |
|-------------|--------------|
| `EQ` | -1 |
| `LT_I64` | -1 |
| `LT_F64` | -1 |

#### Control Flow

| Instruction | Stack Effect |
|-------------|--------------|
| `JMP label` | 0 |
| `JMP_IF_TRUE label` | -1 |
| `JMP_IF_FALSE label` | -1 |

#### Calls & Returns

| Instruction | Stack Effect |
|-------------|--------------|
| `CALL f, argc` | -argc + 1 |
| `RET` | -1 |

#### Heap & Objects

| Instruction | Stack Effect |
|-------------|--------------|
| `NEW type_id` | +1 |
| `GETF field` | 0 |
| `SETF field` | -2 |

### 7.3 Safepoints

以下の位置で静的に定義：
- `CALL`
- `NEW`
- `AllocArray`
- Backward jumps (`JMP*` where target < pc)
- `ThreadSpawn`, `ChannelCreate`

### 7.4 StackMap

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

- コンパイラが生成（codegen.rs拡張）
- VMが検証

### 7.5 Verifier Rules

1. **Control Flow**: ジャンプ先が命令境界であること
2. **Stack Height Consistency**: 各基本ブロックのentry stack heightが一意
3. **Stack Effect Validation**: underflow/overflow なし、max_stack以内

### 7.6 GC

- Precise, non-moving, stop-the-world
- Write barriers: `SETL`, `SETF`

### 7.7 破壊的変更

- 既存API（`VM::run`等）は必要に応じて変更可
- テストは修正対応

## 8. Acceptance Criteria

| # | Criteria | Status |
|---|----------|--------|
| 1 | Value型が `I64 \| F64 \| Bool \| Ref \| Null` の5種類に整理されている | ✅ |
| 2 | 命令セットがv0仕様（+ F64拡張）に整理されている | ✅ |
| 3 | Verifierが実装され、Stack Height不整合を検出・拒否できる | ✅ |
| 4 | Verifierが Stack underflow/overflow を検出・拒否できる | ✅ |
| 5 | Verifierがジャンプ先の命令境界チェックを行う | ✅ |
| 6 | コンパイラがStackMapを生成する | ✅ |
| 7 | VMがStackMapを検証する（Safepoint位置で存在確認） | ✅ |
| 8 | `SETL`, `SETF` でwrite barrierが呼ばれる | ✅ |
| 9 | 既存テストスイートが（修正後）パスする | ✅ |
| 10 | 新規仕様テストが追加され、パスする | ✅ |

## 9. Implementation Files

| File | Description |
|------|-------------|
| `src/vm/value.rs` | Value enum definition |
| `src/vm/ops.rs` | Bytecode operations |
| `src/vm/verifier.rs` | Bytecode verifier |
| `src/vm/stackmap.rs` | StackMap data structures |
| `src/vm/vm.rs` | VM interpreter with write barriers |
| `src/vm/heap.rs` | Heap and GC |

## Appendix: 命令セット整理方針

### 残す命令（v0 Core + F64）
- Constants: `CONST`, `GETL`, `SETL`
- Stack: `POP`, `DUP`
- Arithmetic: `ADD_I64`, `SUB_I64`, `MUL_I64`, `DIV_I64`, `ADD_F64`, `SUB_F64`, `MUL_F64`, `DIV_F64`
- Comparison: `EQ`, `LT_I64`, `LT_F64`
- Control: `JMP`, `JMP_IF_TRUE`, `JMP_IF_FALSE`
- Call: `CALL`, `RET`
- Heap: `NEW`, `GETF`, `SETF`

### 仕様外として維持（削除しない）
- Exception: `Throw`, `TryBegin`, `TryEnd`
- Threading: `ThreadSpawn`, `ChannelCreate`, `ChannelSend`, `ChannelRecv`, `ThreadJoin`
- String/Array: 現状維持
- Print: デバッグ用
