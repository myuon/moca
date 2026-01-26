# Spec.md — BCVM v0 Redesign

## 1. Goal

BCVM v0仕様に基づき、既存VMを段階的にリファクタリングする。
コア機能（VM実行、Verifier、GC統合、StackMap生成）を実装し、Luaライクな軽量・組み込み可能なランタイムの基盤を確立する。

---

## 2. Non-Goals

- **C ABI / Embed Mode**: Phase 2で実装（本Specの範囲外）
- **Moving GC**: v0では非対応
- **Multithreading**: 仕様外として既存コード維持（v0仕様には含めない）
- **Exceptions**: 仕様外として既存コード維持（v0仕様には含めない）
- **Closures / Multiple return values**: v0では非対応
- **バイトコードバイナリ形式**: 当面Rust構造体のまま（後で設計）
- **Runtime source loading**: コンパイラ責務

---

## 3. Target Users

- mica言語のランタイム利用者
- 将来的にVMを組み込みたいホストアプリケーション開発者（Phase 2以降）

---

## 4. Core User Flow

1. ソースコード → コンパイラ → バイトコード（Chunk）+ StackMap生成
2. バイトコード → Verifier → 検証OK/NG
3. 検証済みバイトコード → VM実行（Tier0: Interpreter / Tier1: JIT）
4. 実行中、Safepoint到達時にGCが介入可能
5. 実行完了 → 結果Value返却

---

## 5. Inputs & Outputs

### Inputs
- **バイトコード**: コンパイラが生成したChunk（関数群、定数プール、StackMap）
- **エントリポイント**: main関数または指定関数

### Outputs
- **実行結果**: Value（I64 / Float / Bool / Ref / Null）
- **エラー**: 検証エラー / 実行時エラー

---

## 6. Tech Stack

- **言語**: Rust
- **ビルド**: Cargo
- **テスト**: cargo test（既存テストスイート + 新規仕様テスト）
- **JIT**: 既存インフラ（AArch64 / x86-64）

---

## 7. Rules & Constraints

### 7.1 Value Representation

```
Value = I64(i64) | Float(f64) | Bool(bool) | Ref(GcRef) | Null
```

- 物理実装: Rust enum（Tagged Union）
- Float追加（v0仕様拡張）

### 7.2 命令セット（v0 Core + Float拡張）

**Constants & Locals**
| Instruction | Stack Effect | Description |
|-------------|--------------|-------------|
| `CONST k` | +1 | Push constant |
| `GETL i` | +1 | Push local |
| `SETL i` | -1 | Store local (write barrier) |

**Stack Operations**
| Instruction | Stack Effect |
|-------------|--------------|
| `POP` | -1 |
| `DUP` | +1 |

**Arithmetic (I64)**
| Instruction | Stack Effect |
|-------------|--------------|
| `ADD_I64` | -1 |
| `SUB_I64` | -1 |
| `MUL_I64` | -1 |
| `DIV_I64` | -1 |

**Arithmetic (Float) — 拡張**
| Instruction | Stack Effect |
|-------------|--------------|
| `ADD_F64` | -1 |
| `SUB_F64` | -1 |
| `MUL_F64` | -1 |
| `DIV_F64` | -1 |

**Comparison**
| Instruction | Stack Effect |
|-------------|--------------|
| `EQ` | -1 |
| `LT_I64` | -1 |
| `LT_F64` | -1 |

**Control Flow**
| Instruction | Stack Effect |
|-------------|--------------|
| `JMP label` | 0 |
| `JMP_IF_TRUE label` | -1 |
| `JMP_IF_FALSE label` | -1 |

**Calls & Returns**
| Instruction | Stack Effect |
|-------------|--------------|
| `CALL f, argc` | -argc + 1 |
| `RET` | -1 |

**Heap & Objects**
| Instruction | Stack Effect |
|-------------|--------------|
| `NEW type_id` | +1 |
| `GETF field` | 0 |
| `SETF field` | -2 |

### 7.3 Safepoints

以下の位置で静的に定義：
- `CALL`
- `NEW`
- Backward jumps (`JMP*` where target < pc)

### 7.4 StackMap

```
StackMapEntry {
    pc: u32,
    stack_height: u16,
    stack_ref_bits: bitset,
    locals_ref_bits: bitset,
}
```

- コンパイラが生成（codegen.rs拡張）
- VMが検証

### 7.5 Verifier Rules（Phase 1実装範囲）

1. **Control Flow**: ジャンプ先が命令境界であること
2. **Stack Height Consistency**: 各基本ブロックのentry stack heightが一意
3. **Stack Effect Validation**: underflow/overflow なし、max_stack以内

（型検証は後続フェーズ）

### 7.6 GC

- Precise, non-moving, stop-the-world
- Write barriers: `SETL`, `SETF`

### 7.7 破壊的変更

- 既存API（`VM::run`等）は必要に応じて変更可
- テストは修正対応

---

## 8. Open Questions

1. **既存Quickened命令の扱い**: 削除 or 別モジュールに分離？
2. **String/Array操作命令**: コア命令セットに含めるか、拡張として分離するか

---

## 9. Acceptance Criteria（最大10個）

1. [ ] Value型が `I64 | Float | Bool | Ref | Null` の5種類に整理されている
2. [ ] 命令セットがv0仕様（+ Float拡張）に整理されている
3. [ ] Verifierが実装され、Stack Height不整合を検出・拒否できる
4. [ ] Verifierが Stack underflow/overflow を検出・拒否できる
5. [ ] Verifierがジャンプ先の命令境界チェックを行う
6. [ ] コンパイラがStackMapを生成する
7. [ ] VMがStackMapを検証する（Safepoint位置で存在確認）
8. [ ] `SETL`, `SETF` でwrite barrierが呼ばれる
9. [ ] 既存テストスイートが（修正後）パスする
10. [ ] 新規仕様テストが追加され、パスする

---

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo test` 実行
- 新規追加のVerifierテストがパスすることを確認

### 達成検証
- 全Acceptance Criteriaをチェックリストで確認
- `cargo test --all` が全パス

### 漏れ検出
- 仕様書の各セクションに対応するテストが存在することを確認
- Verifierの各ルールに対応するテストケース（正常系・異常系）

---

## 11. Test Plan

### E2E Scenario 1: 基本的な算術演算

**Given**: I64の加算・乗算を行う関数のバイトコード
**When**: Verifier通過後、VM実行
**Then**: 正しい計算結果（I64）が返る

```
// 例: (3 + 4) * 2 = 14
CONST 3
CONST 4
ADD_I64
CONST 2
MUL_I64
RET
```

### E2E Scenario 2: Verifier拒否（Stack Height不整合）

**Given**: 分岐でstack heightが異なるバイトコード
**When**: Verifierに渡す
**Then**: 検証エラーで拒否される

```
// if分岐の片方でPOPが多い不正なコード
CONST 1
JMP_IF_TRUE label_a
CONST 2
JMP label_end
label_a:
CONST 3
CONST 4  // ← stack height不整合
label_end:
RET
```

### E2E Scenario 3: GC Safepoint + StackMap

**Given**: オブジェクト生成とCALLを含むバイトコード（StackMap付き）
**When**: VM実行中にGCトリガー
**Then**: StackMapに基づきRef値が正しくトレースされ、GC後も正常実行継続

---

## Appendix: 命令セット整理方針

### 残す命令（v0 Core + Float）
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
- String/Array: 現状維持（整理は後続判断）

### 削除候補（要検討）
- Quickened命令（`AddInt`, `SubInt`等）: 必要なら残す
- Print: デバッグ用として残す可能性

---

## Appendix: 元仕様書との対応

| 元仕様セクション | 本Spec対応 | 状態 |
|-----------------|-----------|------|
| 1. Execution Modes | 7.x | Phase 1: Tier0/1, Phase 2: embed |
| 2. Runtime Architecture | 7.x | 対応済 |
| 3. Execution Model | 7.x | 対応済 |
| 4. Value Representation | 7.1 | Float追加で拡張 |
| 5. Bytecode Instruction Set | 7.2 | Float追加で拡張 |
| 6. Safepoints | 7.3 | 対応済 |
| 7. StackMap Specification | 7.4 | 対応済 |
| 8. Verifier Rules | 7.5 | Phase 1: 基本ルール |
| 9. Garbage Collection | 7.6 | 対応済 |
| 10. Modules | — | 既存維持 |
| 11. JIT Compatibility | — | 既存JIT維持 |
| 12. Embed Mode (C ABI) | Non-Goals | Phase 2 |
| 13. Stability Policy | — | バイナリ形式は後で設計 |
| 14. Non-Goals | 2 | 対応済 |
