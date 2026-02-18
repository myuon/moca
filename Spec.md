# Spec.md — Phase 1: Map の any 型依存削減

## 1. Goal
- Map<K, V> の `put/get/contains/remove/set` からランタイム `type_of()` ディスパッチを除去し、コンパイル時の型情報（desugar パスの既存機能）に基づく型別メソッド直接呼び出しのみで動作するようにする

## 2. Non-Goals
- `to_string` の型別特殊化（Phase 2）
- 型別命令の追加（`Op::StringEq` 等、Phase 3）
- ObjectKind の廃止（Phase 4）
- `any` 型そのものの言語仕様からの削除
- `keys()` / `values()` の返り値型の改善（`vec<any>` → `vec<K>` / `vec<V>` は別途対応）
- `HashMapEntry` の型パラメータ化（`__heap_load`/`__heap_store` の型付けとの整合性を優先し据え置き）

## 3. Target Users
- moca 言語のユーザー（Map を使用するプログラムの作者）
- moca コンパイラ / VM の開発者（Phase 2〜4 への土台）

## 4. Core User Flow
- ユーザーは `map<string, int>` や `map<int, string>` のように型付きMapを使う
- `m.put(key, val)`, `m.get(key)`, `m[key]`, `m[key] = val` 等の操作は、既存の desugar パスが K 型を見て `put_string` / `put_int` 等に自動書き換え済み
- **変更点**: prelude から generic ディスパッチメソッド（`put/set/get/contains/remove`）を削除
- **変更点**: ヘルパー関数 `_map_find_entry_int/string` / `_map_rehash_int/string` の引数型を `Map<any, any>` から適切な型に修正
- **変更点**: basic テスト群（map_basic, map_collision, map_int_keys, map_iteration, map_resize）を `map<any, any>` から型付き Map に移行
- **変更点**: `impl map` の `map::new()` を対処
- **変更点**: desugar の `set` メソッドに対する型別書き換え対応を確認・追加

## 5. Inputs & Outputs
- **入力**: 既存の Map 実装（std/prelude.mc）、既存テスト群（tests/snapshots/basic/map_*.mc）
- **出力**: `type_of()` を使わない Map 実装、型付き Map を使うテスト

## 6. Tech Stack
- Rust（コンパイラ）
- moca（prelude/テスト）
- テスト: `cargo test`（snapshot tests）

## 7. Rules & Constraints
- 既存 snapshot テストの**外部挙動（出力）は維持**する（.stdout の内容は変わらない）
- テストの .mc ファイルは型付きMap への書き換えが許容される
- `cargo fmt && cargo check && cargo test && cargo clippy` が全パスすること
- desugar パスの `specialize_map_method` は既に `put/get/contains/remove` → `put_int/put_string` 等の書き換えを行っている（変更不要の見込み）
- desugar で `set` → `put_int/put_string` への書き換えが正しく動くか確認が必要

## 8. Open Questions
- `map::new()` の `impl map` ブロックが `map<any, any>` を返している。呼び出し側で型付きMap を使うなら不要になる可能性あり → 実際の使用状況を調べて判断

## 9. Acceptance Criteria
1. prelude の `Map<K,V>` impl から `put(self, key: any, val: any)` が削除されている
2. prelude の `Map<K,V>` impl から `set(self, key: any, val: any)` が削除されている
3. prelude の `Map<K,V>` impl から `get(self, key: any) -> any` が削除されている
4. prelude の `Map<K,V>` impl から `contains(self, key: any) -> bool` が削除されている
5. prelude の `Map<K,V>` impl から `remove(self, key: any) -> bool` が削除されている
6. `_map_find_entry_int/string` の引数が `Map<any, any>` ではなくなっている
7. `_map_rehash_int/string` の引数が `Map<any, any>` ではなくなっている
8. basic テスト群が型付き Map (`map<string, ...>` / `map<int, ...>`) を使用している
9. 全テスト（`cargo test`）がパスする
10. `cargo clippy` に警告がない

## 10. Verification Strategy
- **進捗検証**: 各タスク完了後に `cargo check && cargo test` を実行
- **達成検証**: `grep "type_of" std/prelude.mc` で Map メソッド内の type_of 使用がゼロ、`grep "key: any" std/prelude.mc` で Map メソッドに any 残留なし
- **漏れ検出**: 全 Acceptance Criteria をチェックリストで確認

## 11. Test Plan

### Scenario 1: 型付き Map<string, V> の基本操作
- **Given**: `let m: map<string, int> = Map<string, int>::new();` で Map 作成
- **When**: `m.put("key", 42)`, `m.get("key")`, `m.contains("key")`, `m.remove("key")` を実行
- **Then**: desugar が put_string/get_string/contains_string/remove_string に書き換え、正しい結果を返す

### Scenario 2: 型付き Map<int, V> の基本操作
- **Given**: `let m: map<int, string> = Map<int, string>::new();` で Map 作成
- **When**: `m.put(1, "one")`, `m.get(1)`, `m.contains(1)`, `m.remove(1)` を実行
- **Then**: desugar が put_int/get_int/contains_int/remove_int に書き換え、正しい結果を返す

### Scenario 3: リサイズ後のデータ整合性
- **Given**: 型付き Map に 20 エントリを追加（リサイズ発生）
- **When**: 全エントリの存在確認と値の検証
- **Then**: 全エントリが正しく取得でき、リサイズ後もデータが保持されている

---

## TODO

- [ ] 1. desugar: `set` メソッドの型別書き換え対応を確認（`put_int`/`put_string` に書き換わるか）
- [ ] 2. prelude: Map<K,V> impl から generic ディスパッチメソッド (put/set/get/contains/remove) を削除
- [ ] 3. prelude: `_map_find_entry_int/string` と `_map_rehash_int/string` の引数型を `Map<any, any>` から修正
- [ ] 4. prelude: `impl map` の `map::new()` を対処
- [ ] 5. テスト: map_basic.mc を型付き Map に移行（.stdout は変更なし）
- [ ] 6. テスト: map_collision.mc を型付き Map に移行（.stdout は変更なし）
- [ ] 7. テスト: map_int_keys.mc を型付き Map に移行（.stdout は変更なし）
- [ ] 8. テスト: map_iteration.mc を型付き Map に移行（.stdout は変更なし）
- [ ] 9. テスト: map_resize.mc を型付き Map に移行（.stdout は変更なし）
- [ ] 10. 全テスト通過確認 (`cargo fmt && cargo check && cargo test && cargo clippy`)
