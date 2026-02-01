# Spec.md - vec<T> / map<K, V> 組み込み型の導入

## 1. Goal
- 組み込み型として `vec<T>` と `map<K, V>` を導入し、型安全なコレクション操作を可能にする
- 既存の `VectorAny` / `HashMapAny` を廃止し、ジェネリクス導入の布石を作る

## 2. Non-Goals
- ユーザー定義ジェネリクス（`struct Foo<T>` など）の導入
- `vec` / `map` のリテラル構文（`[1, 2, 3]` を `vec<int>` として解釈するなど）
- `array<T>` の廃止や統合
- パフォーマンス最適化（既存の実装を流用）

## 3. Target Users
- Moca 言語のユーザー
- 型安全なコレクション操作を求める開発者

## 4. Core User Flow
1. ユーザーが `vec<int>` や `map<string, int>` などの型注釈を書く
2. パーサーが `vec<T>` / `map<K, V>` 構文を認識し、TypeAnnotation に変換
3. 型チェッカーが型安全性を検証（`vec<int>` に `string` を入れるとエラー）
4. コード生成が既存の Vector/HashMap 実装を利用して実行可能コードを出力

## 5. Inputs & Outputs

### Inputs
- Moca ソースコード（`.mc` ファイル）
  ```moca
  let v: vec<int> = vec_new();
  v.push(42);
  v.push(100);
  print(v.get(0));

  let m: map<string, int> = map_new();
  m.put("key", 100);
  print(m.get("key"));
  ```

### Outputs
- 型チェック済みの AST
- 型エラー時のコンパイルエラーメッセージ
- 実行可能なバイトコード

## 6. Tech Stack
- 言語: Rust
- 対象言語: Moca (.mc)
- テスト: `cargo test` + スナップショットテスト
- 主要ファイル:
  - `src/compiler/parser.rs` - 型構文のパース
  - `src/compiler/types.rs` - Type enum の拡張
  - `src/compiler/typechecker.rs` - 型チェックロジック
  - `src/compiler/codegen.rs` - コード生成
  - `std/prelude.mc` - 標準ライブラリ

## 7. Rules & Constraints

### 型システムルール
- `vec<T>` は可変サイズの動的配列、`array<T>` は固定サイズ配列（両方残す）
- `map<K, V>` は K をキー、V を値とするハッシュマップ
- 型パラメータは任意の型を受け付ける（`vec<vec<int>>` なども可能）
- 型の不一致はコンパイルエラー（警告ではない）

### 構文ルール
- 小文字始まり: `vec<T>`, `map<K, V>`（`array<T>` と同じスタイル）
- 山括弧でパラメータを指定: `vec<int>`, `map<string, int>`

### メソッド
- VectorAny/HashMapAny のメソッドを廃止
- 新しいジェネリック対応メソッドを導入:
  - `vec<T>`: `push(value: T)`, `pop() -> T`, `get(index: int) -> T`, `set(index: int, value: T)`, `len() -> int`
  - `map<K, V>`: `put(key: K, value: V)`, `get(key: K) -> V`, `contains(key: K) -> bool`, `remove(key: K) -> bool`, `keys() -> vec<K>`, `values() -> vec<V>`

### 互換性
- `VectorAny` / `HashMapAny` は完全廃止
- 既存テストは `vec<any>` / `map<any, any>` に移行

## 8. Open Questions
- なし（仕様は確定済み）

## 9. Acceptance Criteria

1. パーサーが `vec<int>`, `vec<string>`, `vec<any>` などの構文を正しくパースできる
2. パーサーが `map<string, int>`, `map<int, any>` などの構文を正しくパースできる
3. `Type::Map(Box<Type>, Box<Type>)` が types.rs に追加されている
4. `vec<int>` に `string` を `push` するとコンパイルエラーになる
5. `map<string, int>` に `int` キーで `put` するとコンパイルエラーになる
6. `vec<T>` のメソッド（push, pop, get, set, len）が動作する
7. `map<K, V>` のメソッド（put, get, contains, remove, keys, values）が動作する
8. 既存の `VectorAny` / `HashMapAny` がソースから削除されている
9. 既存テストが `vec<any>` / `map<any, any>` で動作する
10. `cargo fmt && cargo check && cargo test && cargo clippy` が全てパスする

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo check` でコンパイルエラーがないことを確認
- パーサー変更後は単体テストでパース結果を確認
- 型チェッカー変更後は型エラーテストで動作確認

### 達成検証
- Acceptance Criteria のチェックリストを順に確認
- `cargo test` で全テストがパス
- 手動で `vec<int>` / `map<string, int>` を使うコードを書いて動作確認

### 漏れ検出
- `VectorAny` / `HashMapAny` を grep して残存箇所がないことを確認
- 型エラーになるべきケースが実際にエラーになることをテスト

## 11. Test Plan

### E2E シナリオ 1: vec<T> の基本操作
```
Given: vec<int> を生成するコード
When: push(42), push(100), get(0), pop() を順に実行
Then: 正しい値が返り、型エラーなくコンパイル・実行される
```

### E2E シナリオ 2: map<K, V> の基本操作
```
Given: map<string, int> を生成するコード
When: put("a", 1), put("b", 2), get("a"), contains("c") を順に実行
Then: 正しい値が返り、型エラーなくコンパイル・実行される
```

### E2E シナリオ 3: 型エラーの検出
```
Given: vec<int> を生成するコード
When: push("string") を実行しようとする
Then: コンパイル時に型エラーが発生し、適切なエラーメッセージが表示される
```
