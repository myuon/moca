# Spec.md

## 1. Goal
- 関数パラメータとして `Vec<int>`, `Vec<float>`, `Map<string, int>` 等の具体型パラメータ付きジェネリック構造体を受け取ったとき、メソッド呼び出し（`.get()`, `.set()`, `.len()` 等）が正しく解決されるようにする

## 2. Non-Goals
- 型推論アルゴリズム自体の変更
- 新しいジェネリクス機能の追加
- `let` 文の型解決の変更（既に正しく動作している）

## 3. Target Users
- mocaプログラムの開発者（関数パラメータに具体型のジェネリック構造体を使いたい場合）

## 4. Core User Flow
1. `fun foo(v: Vec<int>) { ... }` のようにジェネリック構造体を関数パラメータ型として使う
2. パラメータに対して `.get()`, `.set()`, `.len()`, `[]` 等のメソッド呼び出しが正常に動作する

## 5. Inputs & Outputs
- **入力**: 関数パラメータの型注釈（`Vec<int>`, `Vec<float>`, `Map<string, int>`, `Array<int>`, `Rand` 等）
- **出力**: メソッド呼び出しが正しく解決され、コンパイル・実行が成功する

## 6. Tech Stack
- **言語**: Rust
- **修正対象**: `src/compiler/resolver.rs`
- **テスト**: `cargo test`（既存スナップショットテスト + 新規テスト）

## 7. Rules & Constraints
- 修正は `resolver.rs` の関数パラメータ宣言部分のみ
- `let` 文の既存ロジック（620-651行）と同じパターンで型注釈から `struct_name` を抽出する
- 既存の全テストがパスし続けること
- 修正後、`sort_int`/`sort_float` のシグネチャを `Vec<any>` → 適切な型に戻す

## 8. Acceptance Criteria
1. `fun foo(v: Vec<int>) { v.len(); }` がコンパイル・実行できる
2. `fun foo(v: Vec<float>) { v[0]; }` がコンパイル・実行できる
3. `fun foo(m: Map<string, int>) { m.len(); }` がコンパイル・実行できる
4. `fun foo(a: Array<int>) { a.get(0); }` がコンパイル・実行できる
5. `fun foo(r: Rand) { r.next(); }` がコンパイル・実行できる
6. `sort_int` / `sort_float` が `Vec<int>` / `Vec<float>` パラメータで動作する
7. `cargo fmt && cargo check && cargo test && cargo clippy` が全パスする

## 9. Verification Strategy
- **進捗検証**: 各変更後に `cargo test` で既存テストが壊れていないことを確認
- **達成検証**: 新規スナップショットテスト + sort関数の型修正後にフルテスト通過
- **漏れ検出**: Vec, Map, Array, Named型すべてのパラメータパターンをテスト

## 10. Test Plan

### Test 1: ジェネリック構造体パラメータでのメソッド呼び出し
- **Given**: `Vec<int>`, `Map<string, int>` をパラメータに持つ関数
- **When**: パラメータに対してメソッド呼び出し
- **Then**: コンパイル成功、正しい結果を出力

### Test 2: sort関数の型改善
- **Given**: `sort_int(v: Vec<int>)` に書き換えた sort 関数
- **When**: `Vec<int>` を渡してソート実行
- **Then**: 正しくソートされ、全テストがパス

### Test 3: 既存テストの回帰なし
- **Given**: 修正後のコンパイラ
- **When**: `cargo test` 実行
- **Then**: 全既存テストがパス
