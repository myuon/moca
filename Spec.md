# Spec.md - Object型からMap型への移行

## 1. Goal
- moca言語から組み込みオブジェクト型を削除し、動的なキー・バリュー構造はstdライブラリのHashMap（map）に統一する

## 2. Non-Goals
- struct型の変更（固定フィールドのstructはそのまま維持）
- HashMap実装のパフォーマンス最適化
- 新しい構文の追加

## 3. Target Users
- moca言語の利用者
- 動的オブジェクトを使っていたコードをmapベースに移行する開発者

## 4. Core User Flow
1. 既存の `{ key: value }` リテラルを `map_new_any()` + `map_put_string()` に書き換え
2. 既存の `obj.field` アクセスを `map_get_string(obj, "field")` に書き換え
3. 既存の `obj.field = value` 代入を `map_put_string(obj, "field", value)` に書き換え
4. コンパイル・実行して動作確認

## 5. Inputs & Outputs
### 入力
- 既存のobject関連コード（テストファイル）
- コンパイラソースコード（parser, resolver, codegen, typechecker）
- VMソースコード（ops, vm, heap）

### 出力
- object構文・opcodeを削除したコンパイラ
- MocaObject型を削除したVM
- mapベースに書き換えたテストファイル

## 6. Tech Stack
- 言語: Rust
- テスト: cargo test（スナップショットテスト）
- 対象モジュール:
  - `src/compiler/parser.rs`
  - `src/compiler/resolver.rs`
  - `src/compiler/codegen.rs`
  - `src/compiler/typechecker.rs`
  - `src/compiler/types.rs`
  - `src/vm/ops.rs`
  - `src/vm/vm.rs`
  - `src/vm/heap.rs`
  - `tests/snapshots/basic/object_*.mc`

## 7. Rules & Constraints
- struct型（`struct Foo { x: int }`）は変更しない
- `obj.field` 構文はstruct専用として残す（objectには使えなくなる）
- 既存のmapテスト（map_*.mc）は変更しない
- 全ての既存テストがパスすること

## 8. Open Questions
なし

## 9. Acceptance Criteria
1. [ ] オブジェクトリテラル `{ key: value }` 構文がコンパイルエラーになる
2. [ ] `New` opcodeが削除されている
3. [ ] `GetF` opcodeが削除されている（struct用の仕組みは別途維持）
4. [ ] `SetF` opcodeが削除されている
5. [ ] `MocaObject` 型がheap.rsから削除されている
6. [ ] 既存のobjectテストがmapベースに書き換えられている
7. [ ] 書き換えたテストが全てパスする
8. [ ] structのフィールドアクセス（`point.x`）は引き続き動作する
9. [ ] `cargo check` がエラーなく通る
10. [ ] `cargo test` が全てパスする

## 10. Verification Strategy
- **進捗検証**: 各タスク完了時に `cargo check` と `cargo test` を実行
- **達成検証**: 全Acceptance Criteriaをチェックリストで確認
- **漏れ検出**: grep で `Object` `GetF` `SetF` `New` の残存確認

## 11. Test Plan

### Scenario 1: オブジェクトリテラルがエラーになること
- **Given**: `let obj = { name: "test" };` というコード
- **When**: コンパイルを実行
- **Then**: コンパイルエラーが発生する

### Scenario 2: mapベースのコードが動作すること
- **Given**: 以下のコード
  ```moca
  let obj = map_new_any();
  map_put_string(obj, "name", "Alice");
  print(map_get_string(obj, "name"));
  ```
- **When**: 実行する
- **Then**: "Alice" が出力される

### Scenario 3: structは引き続き動作すること
- **Given**: 以下のコード
  ```moca
  struct Point { x: int, y: int }
  let p = Point { x: 10, y: 20 };
  print(p.x);
  ```
- **When**: 実行する
- **Then**: "10" が出力される
