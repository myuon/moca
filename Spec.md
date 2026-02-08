# Spec.md

## 1. Goal
- TypeChecker が AST を可変参照（`&mut Program`）で受け取り、推論した型情報を AST ノード上に直接書き込むようにリファクタリングする。これにより、HashMap による間接的な型情報伝達を廃止し、下流パス（desugar, monomorphise, resolver, codegen）が AST から直接型を読めるようにする。

## 2. Non-Goals
- 型推論アルゴリズム自体の変更（Algorithm W / 単一化のロジックはそのまま）
- 新しい型チェック機能の追加
- パフォーマンス最適化（構造変更に伴う軽微な影響は許容）
- `Expr` をラッパー構造体（`TypedExpr`）に変える方式（各バリアントにフィールド追加する方式を採用）

## 3. Target Users
- moca コンパイラの開発者（自分自身）。AST に型情報が載ることで、将来の言語機能追加やツール開発（LSP, formatter 等）が容易になる。

## 4. Core User Flow
1. Parser が `Program`（AST）を生成する（変更なし）
2. TypeChecker が `&mut Program` を受け取り、型チェックしながら各ノードの `inferred_type` フィールドに推論結果を書き込む
3. Desugar が AST 上の型情報を直接読んで変換を行う（HashMap 引数を廃止）
4. Monomorphise が AST を処理する（型情報はノード上に載っているのでそのまま伝搬）
5. Resolver が AST 上の型情報を直接読む（`set_variable_types` を廃止）
6. Codegen が AST 上の型情報を直接読む（`set_index_object_types`, `set_len_arg_types` を廃止）

## 5. Inputs & Outputs
- **入力**: Parser が生成した `Program`（型情報フィールドは `None`）
- **出力**: TypeChecker が型情報を書き込んだ `Program`（型情報フィールドが `Some(Type)` で埋まっている）
- **エラー出力**: `Vec<TypeError>`（変更なし）

## 6. Tech Stack
- 言語: Rust
- テスト: cargo test（既存スナップショットテスト + ユニットテスト）
- Lint: cargo clippy + cargo fmt

## 7. Rules & Constraints

### AST 変更ルール
- `Expr` の各バリアントに `inferred_type: Option<Type>` フィールドを追加する
- `Statement` の以下のバリアントにも型情報フィールドを追加する:
  - `IndexAssign`: 既存の `object_type: Option<Type>` をそのまま活用（変更なし）
  - `Let`: `inferred_type: Option<Type>` を追加（変数の推論された型）
  - `ForIn`: `iterable_type: Option<Type>` を追加（イテラブルの型、必要に応じて）
- `Expr::MethodCall` に `object_type: Option<Type>` フィールドを追加する（メソッド呼び出しのオブジェクトの型）
- `Expr::Index` の既存 `object_type: Option<Type>` はそのまま活用する
- Parser は新しい型フィールドをすべて `None` で初期化する

### TypeChecker 変更ルール
- `check_program(&mut self, program: &Program)` → `check_program(&mut self, program: &mut Program)` に変更
- 内部メソッド（`infer_expr`, `infer_statement` 等）もすべて `&mut` 参照を受け取るように変更
- 推論した型を HashMap に保存する代わりに、対応する AST ノードに直接書き込む
- TypeChecker 内部の以下のフィールドと公開アクセサを削除:
  - `index_object_types: HashMap<Span, Type>` + `index_object_types()`
  - `len_arg_types: HashMap<Span, Type>` + `len_arg_types()`
  - `local_variable_types: HashMap<String, Vec<(String, Type)>>` + `local_variable_types()` + `record_local_var()`

### 下流パス変更ルール
- `desugar::desugar_program` の引数から `index_object_types` と `len_arg_types` を削除し、AST ノードから直接型を読む
- `Codegen` から `set_index_object_types()`, `set_len_arg_types()`, 対応する内部フィールドを削除し、AST（Resolved AST）経由で型を読む
- `Resolver` から `set_variable_types()` と対応する内部フィールドを削除し、AST ノードから型を読む
- `mod.rs` の各 `run*` 関数から HashMap の受け渡しコードを削除する

### コンパイルパイプライン（mod.rs）変更ルール
- `check_program` に `&mut program` を渡す
- TypeChecker の後に HashMap を取り出すコード（`.index_object_types().clone()` 等）を削除
- Desugar, Resolver, Codegen に HashMap を渡すコードを削除

## 8. Open Questions
- `Expr::Call`（通常の関数呼び出し）にも `inferred_type` を追加するが、実際に下流パスで使われるかは要確認。使われない場合でも一貫性のため追加する。
- `monomorphise` パスが型情報フィールドをどう扱うか（コピー or リセット）。基本的にはそのまま伝搬させる方針。

## 9. Acceptance Criteria（最大10個）

1. `TypeChecker::check_program` のシグネチャが `&mut Program` を受け取る
2. `Expr` の各バリアントに `inferred_type: Option<Type>` が追加されている
3. `Statement::Let` に `inferred_type: Option<Type>` が追加されている
4. `Expr::MethodCall` に `object_type: Option<Type>` が追加されている
5. TypeChecker 内部の `index_object_types`, `len_arg_types`, `local_variable_types` HashMap と公開アクセサが削除されている
6. `desugar::desugar_program` が HashMap 引数なしで動作する
7. `Codegen` の `set_index_object_types()`, `set_len_arg_types()` が削除されている
8. `Resolver` の `set_variable_types()` が削除されている
9. `cargo test` が全テストパスする
10. `cargo clippy` が警告なしでパスする

## 10. Verification Strategy

- **進捗検証**: 各タスク完了後に `cargo check` → `cargo test` → `cargo clippy` を実行し、コンパイル・テストが通ることを確認する
- **達成検証**: 全 Acceptance Criteria をチェックリストで確認。特に既存の全スナップショットテストがパスすることで、動作の等価性を保証する
- **漏れ検出**: `cargo clippy` で未使用フィールド・関数の警告がないことを確認。grep で旧 HashMap 系 API の残存呼び出しがないことを確認する

## 11. Test Plan

### Scenario 1: 基本的な型チェック + 実行が正常動作する
- **Given**: 既存のスナップショットテストファイル群（`tests/snapshots/`）
- **When**: `cargo test` を実行する
- **Then**: 全テストがパスし、出力が変わらない

### Scenario 2: Index/IndexAssign の型情報が AST 上に載る
- **Given**: 配列やVecのインデックスアクセスを含む `.mc` ファイル
- **When**: TypeChecker を実行した後の AST を確認する
- **Then**: `Expr::Index` と `Statement::IndexAssign` の `object_type` フィールドに正しい型が設定されている

### Scenario 3: MethodCall のオブジェクト型が AST 上に載る
- **Given**: 構造体のメソッド呼び出しを含む `.mc` ファイル
- **When**: TypeChecker を実行した後の AST を確認する
- **Then**: `Expr::MethodCall` の `object_type` フィールドに正しい構造体型が設定されている
