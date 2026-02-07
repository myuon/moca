# Spec.md — Array<T> 構造体化

## 1. Goal
- 固定長配列（Array）をVecと同じ構造体ベースのメモリレイアウト `[ptr, len]` に統一し、ヒープアクセスパターンを共通化する

## 2. Non-Goals
- Arrayに動的リサイズ（push/pop）機能を追加すること
- Vec<T>の実装を変更すること
- 文字列（String）のレイアウトを変更すること
- パフォーマンス最適化（間接参照の除去など）は本タスクでは行わない

## 3. Target Users
- mocaプログラマ（配列リテラル `[1, 2, 3]` やfor-inループの利用者）

## 4. Core User Flow
- 変更前と同じmocaコードがそのまま動作する（破壊的変更なし）
- `let arr = [1, 2, 3];` → Array<int>構造体が生成される
- `arr[0]` → ptr経由で要素にアクセス（内部的に2段間接参照）
- `arr[i] = x` → ptr経由で要素を書き換え
- `len(arr)` → lenフィールドを読み出し
- `for x in arr { ... }` → 従来通り動作

## 5. Inputs & Outputs
- **入力**: 既存のmocaソースコード（配列を使うもの）
- **出力**: 同じ実行結果（メモリレイアウトのみ変更、観測可能な動作は同一）

## 6. Tech Stack
- Rust（コンパイラ・VM実装）
- moca言語（`std/prelude.mc` でのArray<T>定義）

## 7. Rules & Constraints

### 7.1 Array<T> 構造体定義（`std/prelude.mc`）

```mc
// Array<T> - Fixed-length array implementation.
// Layout: [ptr, len]
struct Array<T> {
    ptr: int,
    len: int
}

impl<T> Array<T> {
    fun get(self, index: int) -> T {
        return __heap_load(self.ptr, index);
    }

    fun set(self, index: int, value: T) {
        __heap_store(self.ptr, index, value);
    }

    fun len(self) -> int {
        return self.len;
    }
}
```

### 7.2 配列リテラルのコンパイル変更（`codegen.rs`）

**変更前**: `[1, 2, 3]` →
```
I64Const(1), I64Const(2), I64Const(3), HeapAlloc(3)
```
（フラットなヒープオブジェクト1つ）

**変更後**: `[1, 2, 3]` →
```
// 1. データ配列を確保
I64Const(3)
HeapAllocDynSimple    // → data_ptr (3スロットのヒープオブジェクト)

// 2. 要素を書き込み
// data_ptr, index=0, value=1 → HeapStoreDyn
// data_ptr, index=1, value=2 → HeapStoreDyn
// data_ptr, index=2, value=3 → HeapStoreDyn

// 3. Array<T>構造体を生成
// stack: [data_ptr, len(3)]
HeapAlloc(2)          // → Array<T> { ptr: data_ptr, len: 3 }
```

### 7.3 配列アクセスのコンパイル変更（`codegen.rs`）

`arr[i]`（Index式）で、型が `Array<T>` の場合:
- Vecと同じパス: `HeapLoad(0)` でptrを取得 → `HeapLoadDyn` で要素アクセス
- **方法**: codegen の `is_vector` 判定に `Type::Array(_)` を追加する

`arr[i] = x`（IndexAssign文）でも同様に `HeapLoad(0)` + `HeapStoreDyn` パスを使う。

### 7.4 len() ビルトインの変更（`codegen.rs`）

`len()` の引数型に応じたコード生成:
- **Array<T>の場合**: `HeapLoad(1)` を生成（lenフィールド = slot 1）
- **文字列の場合**: `Op::StrLen` を生成（既存のStrLen opを使用）
- **その他（レガシー）**: 既存の `ArrayLen` は削除

`len()` のコード生成を型認識にするため、typecheckerから `len()` 呼び出しのspan → 引数型のマッピングをcodegenに渡す。

### 7.5 for-inループの変更（`codegen.rs`）

現在:
```
LocalGet(arr_slot)
ArrayLen            // ← 削除対象
I64LtS
```

変更後（Array<T>の場合）:
```
LocalGet(arr_slot)
HeapLoad(1)         // lenフィールド読み出し
I64LtS
```

要素アクセスも変更:
```
// 変更前
LocalGet(arr_slot)
LocalGet(idx_slot)
HeapLoadDyn

// 変更後
LocalGet(arr_slot)
HeapLoad(0)         // ptrフィールド読み出し
LocalGet(idx_slot)
HeapLoadDyn
```

### 7.6 ArrayLen opの削除

以下のファイルから `ArrayLen` を削除:
- `src/vm/ops.rs` - enum variant
- `src/vm/vm.rs` - 実行ハンドラ、JITヘルパー関数、JitCallContext登録
- `src/vm/verifier.rs` - スタック効果定義
- `src/vm/bytecode.rs` - シリアライズ/デシリアライズ（バイトコード番号85は欠番にする）
- `src/compiler/dump.rs` - ダンプ出力
- `src/compiler/codegen.rs` - from_str パース
- `src/vm/stackmap.rs` - セーフポイント判定
- `src/vm/microop_converter.rs` - MicroOp変換
- `src/jit/compiler_x86_64.rs` - x86_64 JITコンパイラ
- `src/jit/compiler.rs` (AArch64) - AArch64 JITコンパイラ
- `src/vm/marshal.rs` - JitCallContext の array_len_helper フィールド

### 7.7 ObjectKind::Array の扱い

- `ObjectKind::Array` は当面残す（将来活用の余地あり）
- 今回のスコープでは積極的に使用しない（データ配列は `ObjectKind::Slots` のまま）

### 7.8 型アノテーション

- `array<T>` 型アノテーションは引き続き `Array<T>` のエイリアスとして動作する
- 内部的に `Type::Array(T)` を残し、codegen側で `Array<T>` 構造体として認識する

### 7.9 desugar.rsの変更

`should_desugar_index` に `Type::Array(_)` を追加し:
- `arr[i]` → `Array::get(arr, i)` にデシュガー
- `arr[i] = x` → `Array::set(arr, i, x)` にデシュガー

**注意**: desugarでメソッド呼び出しに変換する場合、codegenの `is_vector` 判定にArray追加は不要になる。desugar側に統一すること。

## 8. Open Questions
- なし（全て確定済み）

## 9. Acceptance Criteria

1. `struct Array<T> { ptr: int, len: int }` が `std/prelude.mc` に定義されている
2. `[1, 2, 3]` で `Array<int>` 構造体が生成される（ptr→データ配列、len=3）
3. `arr[i]` がptr経由の間接アクセスで正しく値を返す
4. `arr[i] = x` がptr経由で要素を書き換えられる
5. `len(arr)` がlenフィールドの値を返す（ヒープヘッダのslot_countではない）
6. `for x in arr { ... }` が正しく動作する
7. `Op::ArrayLen` がコードベースから完全に削除されている
8. `len("hello")` が引き続き正しく5を返す（文字列は影響なし）
9. `cargo test` が全て通る（既存テストの互換性維持）
10. `cargo clippy` が警告なしで通る

## 10. Verification Strategy

- **進捗検証**: 各タスク完了後に `cargo check && cargo test` を実行し、コンパイル・テストが通ることを確認
- **達成検証**: 全Acceptance Criteriaをチェックリストで確認。特に既存の配列関連スナップショットテストが全て通ること
- **漏れ検出**: `grep -r "ArrayLen"` でコードベースに残存がないことを確認

## 11. Test Plan

### Test 1: 配列リテラルの生成とアクセス
- **Given**: `let arr = [10, 20, 30];`
- **When**: `print(arr[0]); print(arr[1]); print(arr[2]); print(len(arr));`
- **Then**: 出力が `10`, `20`, `30`, `3`

### Test 2: 配列要素の書き換え
- **Given**: `let arr = [1, 2, 3];`
- **When**: `arr[1] = 99; print(arr[1]);`
- **Then**: 出力が `99`

### Test 3: for-inループ
- **Given**: `let arr = [1, 2, 3];`
- **When**: `var sum = 0; for x in arr { sum = sum + x; } print(sum);`
- **Then**: 出力が `6`
