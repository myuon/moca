# Spec.md — VM ValueType & 命令セット再設計

## 1. Goal

moca VM を動的型付き（`Value` enum + ランタイム型チェック）から **静的型付き（u64 スロット + 型別 opcode）** に移行する。WASM に近いアーキテクチャで、将来の GC・JIT・Verifier 拡張の基盤を作る。

## 2. Non-Goals

- GC アルゴリズムの変更（mark & sweep のまま）
- ヒープオブジェクトレイアウトの変更（2-word encoding 維持）
- JIT コンパイラの対応（feature flag で無効化して通す）
- Verifier の型スタック検証（スタック高さ検証のみ維持）
- FFI / threading の変更（命令はそのまま維持）
- 新しい言語機能の追加

## 3. Target Users

- moca 言語の開発者（自分自身）
- Coding Agent（この Spec を元に実装する）

## 4. Core User Flow

1. 既存の moca ソースコードをコンパイル
2. 新しい型別 opcode のバイトコードが生成される
3. VM が u64 スロットベースで実行する
4. 既存のテスト・ベンチマークがすべて通る

## 5. Inputs & Outputs

- **入力**: 既存の moca ソースコード（変更なし）
- **出力**: 新しいバイトコード形式で実行され、同じ結果を返す

## 6. Tech Stack

| カテゴリ | 選定 |
|---------|------|
| 言語 | Rust |
| テスト | `cargo test`（既存のスナップショットテスト + ユニットテスト） |
| ベンチ | 既存ベンチマーク（sum_loop, fibonacci, mutual_recursion 等） |

## 7. Rules & Constraints

### 7.1 ValueType 定義

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    I32,   // 32-bit 整数（u64 スロットの下位 32bit、上位は 0 埋め）
    I64,   // 64-bit 整数
    F32,   // 32-bit 浮動小数（u64 スロットの下位 32bit に f32 bits）
    F64,   // 64-bit 浮動小数
    Ref,   // GC 管理ヒープ参照
}
```

- operand stack / locals は `Vec<u64>` で表現
- i32 値は u64 の下位 32bit に格納（上位 32bit は 0）
- f32 値は u64 の下位 32bit に f32 のビット表現で格納
- f64 値は u64 に f64 のビット表現で格納
- ref 値は u64 にヒープインデックスとして格納（0 = null）

### 7.2 言語型とのマッピング

| moca の型 | VM ValueType | 備考 |
|-----------|-------------|------|
| `int` | I64 | |
| `float` | F64 | |
| `bool` | I32 | true=1, false=0 |
| `string` | Ref | ヒープ上の文字列オブジェクト |
| struct/array/vector | Ref | ヒープオブジェクト |
| `null` | Ref | ref の 0 値（`RefNull`） |

### 7.3 新しい命令セット（Op enum）

```rust
pub enum Op {
    // ====== Constants ======
    I32Const(i32),         // → [i32]
    I64Const(i64),         // → [i64]
    F32Const(f32),         // → [f32]
    F64Const(f64),         // → [f64]
    RefNull,               // → [ref] (null ref = 0)
    StringConst(usize),    // string pool index → [ref]

    // ====== Local Variables ======
    LocalGet(usize),       // [locals[n]] → stack
    LocalSet(usize),       // stack → [locals[n]]

    // ====== Stack Manipulation ======
    Drop,                  // [a] → []
    Dup,                   // [a] → [a, a]
    Pick(usize),           // n-th element をコピーして top へ
    PickDyn,               // [depth] → [value] (動的深度)

    // ====== i32 Arithmetic ======
    I32Add,                // [i32, i32] → [i32]
    I32Sub,
    I32Mul,
    I32DivS,
    I32RemS,
    I32Eqz,               // [i32] → [i32]  (x == 0 ? 1 : 0)

    // ====== i64 Arithmetic ======
    I64Add,                // [i64, i64] → [i64]
    I64Sub,
    I64Mul,
    I64DivS,
    I64RemS,
    I64Neg,                // [i64] → [i64]  (0 - x)

    // ====== f32 Arithmetic ======
    F32Add,                // [f32, f32] → [f32]
    F32Sub,
    F32Mul,
    F32Div,
    F32Neg,                // [f32] → [f32]

    // ====== f64 Arithmetic ======
    F64Add,                // [f64, f64] → [f64]
    F64Sub,
    F64Mul,
    F64Div,
    F64Neg,                // [f64] → [f64]

    // ====== i32 Comparison → i32 ======
    I32Eq,                 // [i32, i32] → [i32]
    I32Ne,
    I32LtS,
    I32LeS,
    I32GtS,
    I32GeS,

    // ====== i64 Comparison → i32 ======
    I64Eq,                 // [i64, i64] → [i32]
    I64Ne,
    I64LtS,
    I64LeS,
    I64GtS,
    I64GeS,

    // ====== f32 Comparison → i32 ======
    F32Eq,                 // [f32, f32] → [i32]
    F32Ne,
    F32Lt,
    F32Le,
    F32Gt,
    F32Ge,

    // ====== f64 Comparison → i32 ======
    F64Eq,                 // [f64, f64] → [i32]
    F64Ne,
    F64Lt,
    F64Le,
    F64Gt,
    F64Ge,

    // ====== Ref Comparison → i32 ======
    RefEq,                 // [ref, ref] → [i32]
    RefIsNull,             // [ref] → [i32]

    // ====== Type Conversion ======
    I32WrapI64,            // [i64] → [i32]  (truncate)
    I64ExtendI32S,         // [i32] → [i64]  (sign-extend)
    I64ExtendI32U,         // [i32] → [i64]  (zero-extend)
    F64ConvertI64S,        // [i64] → [f64]
    I64TruncF64S,          // [f64] → [i64]
    F64ConvertI32S,        // [i32] → [f64]
    F32ConvertI32S,        // [i32] → [f32]
    F32ConvertI64S,        // [i64] → [f32]
    I32TruncF32S,          // [f32] → [i32]
    I32TruncF64S,          // [f64] → [i32]
    I64TruncF32S,          // [f32] → [i64]
    F32DemoteF64,          // [f64] → [f32]
    F64PromoteF32,         // [f32] → [f64]

    // ====== Control Flow ======
    Jmp(usize),            // 無条件ジャンプ
    BrIf(usize),           // [i32] → []  (!=0 で分岐)
    BrIfFalse(usize),      // [i32] → []  (==0 で分岐)
    Call(usize, usize),    // (func_index, argc)
    Ret,

    // ====== Heap Operations ======
    // ヒープは引き続き 2-word encoding (tag+payload)。
    // HeapStore 系は型情報が必要（GC が ref を識別するため）。
    HeapAlloc(usize),      // [v1..vN] → [ref]  (N スロット確保)
    HeapAllocDyn,          // [size, v1..vN] → [ref]
    HeapAllocDynSimple,    // [size] → [ref]  (null 初期化)
    HeapLoad(usize),       // [ref] → [value]  (静的オフセット)
    HeapStore(usize),      // [ref, value] → []  (静的オフセット)
    HeapLoadDyn,           // [ref, index] → [value]
    HeapStoreDyn,          // [ref, index, value] → []
    ArrayLen,              // [ref] → [i64]

    // ====== System / Builtins ======
    Syscall(usize, usize), // (syscall_num, argc)
    GcHint(usize),
    PrintDebug,            // 一時的に維持（後述）
    TypeOf,                // [any] → [ref(string)]
    ToString,              // [any] → [ref(string)]
    ParseInt,              // [ref(string)] → [i64]
    StrLen,                // [ref(string)] → [i64]

    // ====== Exception Handling ======
    Throw,
    TryBegin(usize),
    TryEnd,

    // ====== CLI ======
    Argc,                  // → [i64]
    Argv,                  // [i64] → [ref(string)]
    Args,                  // → [ref(array)]

    // ====== Threading ======
    ThreadSpawn(usize),
    ChannelCreate,
    ChannelSend,
    ChannelRecv,
    ThreadJoin,
}
```

### 7.4 旧命令 → 新命令マッピング

| 旧 Op | 新 Op | 備考 |
|--------|-------|------|
| `PushInt(v)` | `I64Const(v)` | |
| `PushFloat(v)` | `F64Const(v)` | |
| `PushTrue` | `I32Const(1)` | |
| `PushFalse` | `I32Const(0)` | |
| `PushNull` | `RefNull` | |
| `PushString(idx)` | `StringConst(idx)` | |
| `Pop` | `Drop` | |
| `Dup` | `Dup` | |
| `Swap` | 廃止 | 使用箇所がないなら削除。必要なら `Pick(1)` 等で代替 |
| `Pick(n)` | `Pick(n)` | |
| `PickDyn` | `PickDyn` | |
| `GetL(n)` | `LocalGet(n)` | |
| `SetL(n)` | `LocalSet(n)` | |
| `Add` | `I64Add` / `F64Add` | codegen が型に応じて選択 |
| `Sub` | `I64Sub` / `F64Sub` | |
| `Mul` | `I64Mul` / `F64Mul` | |
| `Div` | `I64DivS` / `F64Div` | |
| `Mod` | `I64RemS` | float の mod は未使用 |
| `Neg` | `I64Neg` / `F64Neg` | |
| `Eq` | `I64Eq` / `F64Eq` / `RefEq` | codegen が型に応じて選択 |
| `Ne` | `I64Ne` / `F64Ne` | |
| `Lt` | `I64LtS` / `F64Lt` | |
| `Le` | `I64LeS` / `F64Le` | |
| `Gt` | `I64GtS` / `F64Gt` | |
| `Ge` | `I64GeS` / `F64Ge` | |
| `Not` | `I32Eqz` | bool(i32) の論理否定 |
| `Jmp(t)` | `Jmp(t)` | |
| `JmpIfFalse(t)` | `BrIfFalse(t)` | i32 条件 |
| `JmpIfTrue(t)` | `BrIf(t)` | i32 条件 |
| その他 | 同名維持 | Syscall, Thread, Exception 等 |

### 7.5 Codegen の変更方針

codegen は typechecker から渡される `Type` 情報を使って、型に応じた opcode を選択する。

**必要な情報の伝搬:**
- `ResolvedExpr` の各ノードに対応する `Type` を codegen が参照できる必要がある
- 現在 typechecker → resolver → codegen のパイプラインで `Type` 情報が部分的に渡されている
- 二項演算の codegen には「オペランドの型」が必要。これは以下のいずれかで実現:
  - resolver が各式の型を保持する（推奨）
  - codegen が式の構造から型を推論する

**具体的な変更:**
1. `compile_expr` で定数リテラルの emit を変更（`PushInt` → `I64Const` 等）
2. `compile_expr` の `Binary` で型に応じた opcode を選択
3. `compile_expr` の `Unary` で型に応じた opcode を選択
4. `compile_statement` で bool 条件を i32 として扱う

### 7.6 VM 実行エンジンの変更

**スタック:**
```rust
// 旧
stack: Vec<Value>,

// 新
stack: Vec<u64>,
```

**ヘルパー関数（u64 ⇔ 型変換）:**
```rust
fn u64_from_i64(v: i64) -> u64 { v as u64 }
fn u64_from_f64(v: f64) -> u64 { v.to_bits() }
fn u64_from_i32(v: i32) -> u64 { (v as u32) as u64 }  // zero-extend
fn u64_from_f32(v: f32) -> u64 { (v.to_bits()) as u64 }
fn u64_from_ref(r: GcRef) -> u64 { r.index as u64 }

fn i64_from_u64(v: u64) -> i64 { v as i64 }
fn f64_from_u64(v: u64) -> f64 { f64::from_bits(v) }
fn i32_from_u64(v: u64) -> i32 { v as i32 }
fn f32_from_u64(v: u64) -> f32 { f32::from_bits(v as u32) }
fn ref_from_u64(v: u64) -> GcRef { GcRef { index: v as usize } }
```

**dispatch ループ:** 各 opcode は型固定で、ランタイム型チェック不要。

### 7.7 ヒープとの接続（過渡的設計）

ヒープは引き続き 2-word encoding (`tag + payload`) を使用する。VM ↔ ヒープの変換:

- **HeapLoad**: ヒープから `(tag, payload)` を読み、`payload` を u64 としてスタックに push
- **HeapStore**: スタックから u64 を pop し、適切な tag をつけてヒープに書き込む
  - tag の決定: GC がヒープ上の ref を識別するために必要
  - 方針: codegen が ref を store する場合とそれ以外を区別できるよう、ヒープ store 時に write barrier で ref フラグを設定する（現行の `SetL` write barrier と同様のアプローチ）

### 7.8 PrintDebug の扱い

`PrintDebug` は過渡的にスタックの u64 を i64 として解釈して出力する。文字列の print は `print_str` 関数（stdlib）経由なので影響なし。正確な型別 print は将来の Syscall 化で対応。

### 7.9 Bytecode Serialization

- バージョンを `VERSION = 2` に上げる
- 新しい opcode タグ番号を割り当てる（旧タグとの互換性は不要）
- Function に `param_types: Vec<ValueType>` と `return_type: Option<ValueType>` を追加（将来の verifier 用）

### 7.10 JIT の扱い

- `feature = "jit"` でコンパイルされる JIT コードは、新しい Op enum に合わせてコンパイルが通るよう最低限の修正を行う
- JIT の実際の動作は保証しない（テストは JIT 無効で実行）

### 7.11 Verifier の扱い

- 新しい Op enum に合わせてパターンマッチを更新
- スタック高さ検証は維持
- 型スタック検証は今回は実装しない

### 7.12 既存の Swap の扱い

現在の codegen で `Swap` の使用箇所を確認し、不要なら削除。使用箇所がある場合は維持する。

## 8. Open Questions

- ヒープの 1-word 化（tag 廃止 + ref bitmap）は v1 で対応予定
- `TypeOf` / `ToString` は将来的に Syscall に統合予定

## 9. Acceptance Criteria

1. `cargo check` がエラーなく通る
2. `cargo test` の全テストが通る
3. `cargo clippy` が warning なく通る（既存の allow は除く）
4. `Value` enum が VM の実行パスから除去されている（ヒープ encode/decode の内部利用は許容）
5. operand stack と locals が `Vec<u64>` で実装されている
6. 算術・比較命令が型別 opcode になっている（`I64Add`, `F64Lt` 等）
7. codegen が typechecker の型情報を使って正しい型別 opcode を選択している
8. bool が i32 (0/1) として表現されている
9. 分岐命令（`BrIf`, `BrIfFalse`）が i32 条件値を消費する
10. bytecode serialization が新 opcode で正しく roundtrip する

## 10. Verification Strategy

- **進捗検証**: 各タスク完了後に `cargo check && cargo test` を実行。コンパイルが通りテストが通ることを確認
- **達成検証**: 全 Acceptance Criteria をチェックリストで確認。特に `cargo test` の全パスが必須
- **漏れ検出**: 既存のスナップショットテスト（`tests/snapshot_tests.rs`）が出力の一致を検証。ベンチマークの動作確認

## 11. Test Plan

### E2E シナリオ 1: 整数演算の型別 opcode 化

- **Given**: `let x: int = 1 + 2 * 3; print_debug(x);` をコンパイル
- **When**: 新 VM で実行
- **Then**: `7` が出力される。生成されたバイトコードに `I64Const`, `I64Add`, `I64Mul` が含まれる

### E2E シナリオ 2: Bool と条件分岐

- **Given**: `let b: bool = true; if b { print_debug(1); } else { print_debug(0); }` をコンパイル
- **When**: 新 VM で実行
- **Then**: `1` が出力される。`I32Const(1)` が bool 表現として使われ、`BrIfFalse` が分岐に使われる

### E2E シナリオ 3: ヒープオブジェクト（文字列・構造体）

- **Given**: `struct Point { x: int, y: int } let p = Point { x: 10, y: 20 }; print_debug(p.x + p.y);` をコンパイル
- **When**: 新 VM で実行
- **Then**: `30` が出力される。`HeapAlloc`, `HeapLoad`, `I64Add` が使われる
