# Spec.md — 文字列の構造体化（String [ptr, len]）

## 1. Goal
- 文字列の内部表現を `[ptr, len]` の2スロット構造体に変更し、`StrLen` オペコードを削除する。Array<T>・Vec<T>と統一的なメモリレイアウトにする。

## 2. Non-Goals
- 文字列のエンコーディング変更（UTF-8化など）
- 文字列の不変性保証
- JIT側のStringConst inline最適化（VM側に戻してよい）
- for-inの文字列対応追加（現状維持）

## 3. Target Users
- moca言語の開発者（内部最適化）

## 4. Core User Flow
- ユーザーから見た挙動変更なし。内部表現のみ変更。

## 5. Inputs & Outputs
- 入力: moca ソースコード（文字列を使用するプログラム）
- 出力: 実行結果は変更前と完全一致

## 6. Tech Stack
- Rust（既存プロジェクト）
- テスト: `cargo test`（既存スナップショットテスト）

## 7. Rules & Constraints

### 7.1 文字列メモリレイアウト変更

**変更前（現状）:**
```
StringConst → ObjectKind::String, slots = [char0, char1, ..., charN]
len(s)  → StrLen（ヘッダのslot_countを読む）
s[i]    → HeapLoadDyn（直接アクセス）
```

**変更後:**
```
StringConst → String構造体 [ptr, len] (ObjectKind::String)
  ptr → データ配列 [char0, char1, ..., charN] (ObjectKind::Slots)
len(s)  → HeapLoad(1)
s[i]    → HeapLoad(0) + HeapLoadDyn
```

### 7.2 各オペコード・関数への影響

| 対象 | 変更内容 |
|------|---------|
| `alloc_string` (heap.rs) | データ配列(ObjectKind::Slots)を割当後、`[ptr, len]`構造体(ObjectKind::String)を割当 |
| `StringConst` (vm.rs) | `alloc_string`の変更により自動対応 |
| `StrLen` | **削除**。codegen側で `HeapLoad(1)` を emit |
| `len()` builtin (codegen.rs) | 文字列も `HeapLoad(1)` を emit（StrLen emit → HeapLoad(1)に変更） |
| 文字列結合 `+` (vm.rs add) | `(Ref, Ref)` パスでString構造体のptr経由でデータを読み、新String構造体を生成 |
| `value_to_string` (vm.rs) | ObjectKind::String の場合、`ptr` 経由でデータ配列を読んで文字列化 |
| `print` | `value_to_string` の変更により自動対応 |
| `type_of` | 変更なし（`ObjectKind::String` のまま → `"string"` を返す） |
| `for c in str` (codegen.rs) | 現在の実装はArray<T>前提の `HeapLoad(1)` / `HeapLoad(0)+HeapLoadDyn` 。String構造体も同じレイアウトなのでそのまま動く |
| `s[i]` / `s[i]=x` (codegen.rs) | `has_ptr_layout` チェックに文字列型を追加（必要な場合のみ） |
| `RefEq` (文字列比較, vm.rs) | ptr経由でデータ配列を比較するよう変更 |
| JIT StringConst | push_string_helper経由でVM側のalloc_stringを呼ぶので自動対応 |

### 7.3 `alloc_string` 変更

```rust
pub fn alloc_string(&mut self, value: String) -> Result<GcRef, String> {
    let slots: Vec<Value> = value.chars().map(|c| Value::I64(c as i64)).collect();
    let len = slots.len();
    let data_ref = self.alloc_slots(slots)?;  // ObjectKind::Slots
    let struct_slots = vec![Value::Ref(data_ref), Value::I64(len as i64)];
    self.alloc_slots_with_kind(struct_slots, ObjectKind::String)
}
```

### 7.4 Op::StrLen 削除対象ファイル

ArrayLen削除と同じパターン:
- `src/vm/ops.rs` - enum variant + describe
- `src/vm/vm.rs` - execution arm
- `src/vm/bytecode.rs` - serialization/deserialization + constant
- `src/vm/verifier.rs` - stack effect
- `src/compiler/codegen.rs` - from_str parser + len() emit変更
- `src/compiler/dump.rs` - format
- JIT (compiler.rs, compiler_x86_64.rs) - match arm（存在する場合）

### 7.5 codegen len() の変更

```
// 変更前
len(s)  where s: string → StrLen
len(a)  where a: Array<T> → HeapLoad(1)

// 変更後
len(s)  where s: string → HeapLoad(1)
len(a)  where a: Array<T> → HeapLoad(1)
// → 型判定不要になる。常にHeapLoad(1)をemit
```

## 8. Open Questions
- なし

## 9. Acceptance Criteria

1. `len("hello")` が `5` を返す（HeapLoad(1)経由）
2. `"hello"[0]` が `104` (= 'h') を返す（HeapLoad(0) + HeapLoadDyn経由）
3. `"hello" + " world"` が `"hello world"` を返す
4. `print("hello")` が `hello` を表示する
5. `type_of("hello")` が `"string"` を返す
6. `Op::StrLen` がコードベースから完全に削除されている
7. `cargo test` が全パスする
8. `cargo clippy` が警告なしでパスする
9. `--dump-microops` でhot pathに `StrLen` が出現しない（`HeapLoad(1)` に置換されている）

## 10. Verification Strategy
- **進捗検証**: 各タスク完了後に `cargo check && cargo test` を実行
- **達成検証**: 全Acceptance Criteriaを `cargo test` + 手動確認でチェック
- **漏れ検出**: `grep -r "StrLen" src/` で残存がないことを確認

## 11. Test Plan

### Scenario 1: 文字列基本操作
- **Given**: 文字列リテラルを使用するプログラム
- **When**: `len("hello")`, `"hello"[2]`, `print("hello")` を実行
- **Then**: それぞれ `5`, `108`, `hello` が得られる

### Scenario 2: 文字列結合
- **Given**: 2つの文字列を `+` で結合するプログラム
- **When**: `"hello" + " " + "world"` を実行
- **Then**: `"hello world"` が得られる

### Scenario 3: 既存スナップショットテスト全パス
- **Given**: 既存のテストスイート
- **When**: `cargo test` を実行
- **Then**: 全テスト（322 unit + 4 mandelbrot + 16 snapshot）がパスする

## TODO

- [ ] 1. `alloc_string` を `[ptr, len]` 構造体に変更 + `value_to_string` / `RefEq` をString構造体対応に変更
- [ ] 2. `I64Add` の文字列結合パスをString構造体対応に変更
- [ ] 3. codegen: `len()` を常に `HeapLoad(1)` にemit + 文字列index対応（`has_ptr_layout`）
- [ ] 4. `Op::StrLen` をコードベース全体から削除
- [ ] 5. `cargo fmt && cargo check && cargo test && cargo clippy` を実行して全パスを確認
