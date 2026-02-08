# Spec.md

## 1. Goal
- `let v: T = new T {...}` / `var v: T = new T {...}` のように、`new` リテラルで型が明白な場合に冗長な型注釈を警告するリンタールール `redundant-type-annotation` を追加する

## 2. Non-Goals
- `new` リテラル以外の冗長な型注釈（例: `let v: int = 42`）は対象外
- 自動修正（autofix）は提供しない
- 型名が不一致の場合（例: `let v: Base = new Derived {...}`）は検出しない

## 3. Target Users
- mocaプログラマ。冗長な型注釈を除去して可読性を向上させたい場面

## 4. Core User Flow
1. ユーザーが `let v: T = new T {...}` のようなコードを書く
2. `moca lint <file>` を実行する
3. 冗長な型注釈について警告が表示される
4. ユーザーが `let v = new T {...}` に修正する

## 5. Inputs & Outputs
- **入力**: `.mc` ソースファイル（型チェック済みAST）
- **出力**: 診断メッセージ（`warning: redundant-type-annotation: ...`）

## 6. Tech Stack
- Rust（既存の linter フレームワーク `src/compiler/linter.rs`）
- 既存の `LintRule` トレイトを実装
- スナップショットテスト（`tests/snapshots/lint/`）

## 7. Rules & Constraints
- `Statement::Let` の `type_annotation` と `Expr::NewLiteral` の `type_name` + `type_args` を比較する
- 比較ロジック:
  - `TypeAnnotation::Named(name)` → `type_name == name && type_args.is_empty()`
  - `TypeAnnotation::Vec(inner)` → `type_name == "Vec" && type_args == [inner]`
  - `TypeAnnotation::Map(key, val)` → `type_name == "Map" && type_args == [key, val]`
  - `TypeAnnotation::Generic { name, type_args }` → `type_name == name && type_args` が一致
- `mutable: true`（var）、`mutable: false`（let）の両方に適用
- spanは `Statement::Let` の span を使用（型注釈がある文全体を指す）
- 診断メッセージ: `remove redundant type annotation; type is already specified by 'new T'`（Tは実際の型名）

## 8. Open Questions
- なし

## 9. Acceptance Criteria（最大10個）
1. `let v: Point = new Point { x: 1 }` で警告が出る
2. `var v: Vec<int> = new Vec<int> { 1, 2 }` で警告が出る
3. `let m: Map<string, int> = new Map<string, int> { "a": 1 }` で警告が出る
4. `let v: Container<int> = new Container<int> { value: 42 }` でジェネリック構造体も警告が出る
5. `let v = new Point { x: 1 }` は型注釈がないので警告なし
6. `let v: int = 42` は `new` リテラルではないので警告なし
7. `cargo test` が全て通る
8. `cargo clippy` が警告なしで通る
9. `moca lint` でスナップショットテストが正しく動作する

## 10. Verification Strategy
- **進捗検証**: 各タスク完了後に `cargo check` → `cargo test` → `cargo clippy` を実行
- **達成検証**: 全 Acceptance Criteria のスナップショットテストが通ること
- **漏れ検出**: 警告あり/なしの両方のケースをテストで網羅

## 11. Test Plan

### Test 1: 基本的な冗長型注釈の検出
- **Given**: `let v: Point = new Point { x: 1, y: 2 };` を含む `.mc` ファイル
- **When**: `moca lint` を実行
- **Then**: `redundant-type-annotation` 警告が出力される

### Test 2: var（mutable）でも検出される
- **Given**: `var v: Vec<int> = new Vec<int> { 1, 2 };` を含む `.mc` ファイル
- **When**: `moca lint` を実行
- **Then**: `redundant-type-annotation` 警告が出力される

### Test 3: 型注釈なしでは警告なし
- **Given**: `let v = new Vec<int> { 1, 2 };` を含む `.mc` ファイル
- **When**: `moca lint` を実行
- **Then**: `redundant-type-annotation` 警告は出力されない
