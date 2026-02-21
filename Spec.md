# Spec.md — #166: Map/Vecメソッドの型チェックをimplブロック定義から自動導出する

## 1. Goal
- `typechecker.rs` の `check_vec_method` / `check_map_method` のハードコードを廃止し、`impl` ブロックの定義から型情報を自動導出する
- prelude.mc の impl ブロックが唯一の型情報ソース（Single Source of Truth）になる

## 2. Non-Goals
- `check_ptr_method` の統合（ptr は1メソッドのみなので今回は対象外）
- `check_builtin` のグローバル関数（len, push, pop 等）の整理
- prelude.mc のメソッド実装の変更（型チェックの仕組みのみ変更）
- 新しい言語機能の追加

## 3. Target Users
- moca 言語の開発者（コンパイラ保守者）
- prelude.mc にメソッドを追加する人

## 4. Core User Flow
1. 開発者が `std/prelude.mc` の `impl<T> Vec<T>` や `impl<K, V> Map<K, V>` にメソッドを追加する
2. `typechecker.rs` の修正なしで、そのメソッドの型チェックが正しく動作する
3. moca プログラムからそのメソッドを呼び出せる

## 5. Inputs & Outputs
- **入力**: `impl<T> Vec<T>` / `impl<K, V> Map<K, V>` ブロック内のメソッドシグネチャ
- **出力**: メソッド呼び出し時の自動的な型チェック（引数の型・個数、戻り値の型）

## 6. Tech Stack
- 言語: Rust（既存コードベース）
- テスト: `cargo test`（既存の snapshot テスト + 新規テスト）

## 7. Rules & Constraints
- 既存の全 snapshot テストが変更なしでパスすること
- primitive 型（int, float, bool, string）の `check_primitive_method` は変更しない（既に lookup テーブル方式）
- Vec/Map の型パラメータ（`T`, `K`, `V`）が呼び出し側の具体型に正しく置換されること
- 内部ヘルパー（`_find_entry_int`, `_rehash_int` 等）も自動導出の対象にする
- `self` パラメータは型チェック時にスキップし、残りの引数で型チェックする（現行動作を維持）

## 8. Open Questions
- `register_impl_methods` で Vec/Map メソッドが `self.structs[name].methods` に登録される際、ジェネリック型パラメータがどう保持されるか → 実装時に調査

## 9. Acceptance Criteria
1. `check_vec_method` 関数が `typechecker.rs` から削除されている
2. `check_map_method` 関数が `typechecker.rs` から削除されている
3. Vec のメソッド呼び出し（push, pop, get, set, len）の型チェックが `structs["Vec"].methods` のルックアップで行われている
4. Map のメソッド呼び出し（put_int, get_int, contains_int, remove_int, keys, values, len, _find_entry_int, _rehash_int 等）の型チェックが `structs["Map"].methods` のルックアップで行われている
5. 既存の全 snapshot テスト（basic, generics, errors, lint 等）がパスする
6. `cargo clippy` が警告なしでパスする
7. prelude.mc の Vec impl に新メソッド（例: `first`）を追加し、コンパイラ変更なしで型チェックが通ることを示す新テストが存在する

## 10. Verification Strategy

- **進捗検証**: 各フェーズ完了後に `cargo test` を実行し、既存テストの回帰がないことを確認
- **達成検証**: AC 1-7 のチェックリストを全て満たすこと。特に AC7（新メソッド追加テスト）が本リファクタの真の価値を証明する
- **漏れ検出**: `check_vec_method` / `check_map_method` への参照が typechecker.rs に残っていないことを grep で確認

## 11. Test Plan

### Test 1: Vec メソッドの型チェックが自動導出で動作する
- **Given**: 既存の `tests/snapshots/basic/simple_vec.mc`, `vec_push_realloc.mc`, `vector_index_syntax.mc` 等
- **When**: `cargo test snapshot_basic` を実行
- **Then**: 全テストがパスする

### Test 2: Map メソッドの型チェックが自動導出で動作する
- **Given**: 既存の `tests/snapshots/basic/map_basic.mc`, `map_collision.mc`, `map_int_keys.mc`, `map_iteration.mc` 等
- **When**: `cargo test snapshot_basic` を実行
- **Then**: 全テストがパスする

### Test 3: prelude にメソッド追加でコンパイラ変更不要
- **Given**: `impl<T> Vec<T>` に `fun first(self) -> T` メソッドを追加
- **When**: テストコードから `v.first()` を呼び出す
- **Then**: `typechecker.rs` を変更せずに型チェックがパスし、正しい結果が得られる

---

## TODO

- [ ] 1. 調査: `register_impl_methods` での Vec/Map メソッドの登録状況とジェネリック型パラメータの保持方法を確認
- [ ] 2. Core: Vec メソッド呼び出しを `structs["Vec"].methods` ルックアップに切り替え、`check_vec_method` を削除
- [ ] 3. Core: Map メソッド呼び出しを `structs["Map"].methods` ルックアップに切り替え、`check_map_method` を削除
- [ ] 4. Test: prelude に Vec::first メソッドを追加し、コンパイラ変更なしで型チェックが通る新テストを追加
- [ ] 5. Polish: 全テスト通過確認 (`cargo fmt && cargo check && cargo test && cargo clippy`)
