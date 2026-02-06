# Spec.md

## 1. Goal
- mocaコンパイラにlinterサブコマンド (`moca lint <file>`) を追加し、typecheck後のASTを解析してコード改善の提案をstdoutに出力する

## 2. Non-Goals
- 自動修正（autofix）機能
- LSP統合（既存LSPへのlint結果の組み込み）
- 設定ファイルによるルールの有効/無効切り替え
- `moca check` や `moca run` 実行時の自動lint

## 3. Target Users
- mocaでコードを書く開発者
- CI/CDパイプラインでコード品質チェックを行いたい場合

## 4. Core User Flow
1. ユーザーが `moca lint <file.mc>` を実行する
2. コンパイラがファイルをparse → typecheckする
3. typecheck後のASTに対してlintルールを実行する
4. lint警告があればtypecheckerと同じフォーマットでstdoutに出力する
5. lint警告があれば終了コード1、なければ終了コード0で終了する

## 5. Inputs & Outputs
- **入力**: mocaソースファイル（`.mc`）のパス
- **出力**: lint診断メッセージ（stdout）。フォーマットは以下の通り:
  ```
  warning: {RULE_NAME}: {MESSAGE}
    --> {FILENAME}:{LINE}:{COLUMN}
  ```

## 6. Tech Stack
- 言語: Rust（既存プロジェクト）
- 既存コンパイラ基盤: lexer, parser, typechecker を再利用
- テスト: `cargo test`（既存テスト基盤）

## 7. Rules & Constraints
- lintはtypecheck成功後に実行する（typecheckがエラーの場合はlintを実行しない）
- lint結果はstdoutに出力する（typecheckのエラーはstderrだが、lint警告はstdout）
- 各lintルールは独立した関数として実装し、後からルールを追加しやすい構造にする
- 初期実装ルール:
  - `prefer-new-literal`: `vec::new()` の呼び出しを検出し、`new Vec<T> {}` 構文の使用を提案する

### `prefer-new-literal` ルールの詳細
- **検出対象**: `Expr::AssociatedFunctionCall` で `type_name` が `"vec"` かつ `function` が `"new"` かつ引数が0個のパターン
- **出力メッセージ**: `use \`new Vec<T> {}\` instead of \`vec::new()\``
- **severity**: warning

## 8. Open Questions
- なし

## 9. Acceptance Criteria
1. `moca lint <file>` サブコマンドが実行できる
2. `vec::new()` を含むコードに対してlintを実行すると、warning が stdout に出力される
3. 出力フォーマットが `warning: prefer-new-literal: use ...` + `  --> file:line:column` の形式である
4. lint警告がある場合、終了コードが1である
5. lint警告がない場合、終了コードが0である
6. typecheckエラーがあるファイルに対しては、typecheckエラーを表示しlintは実行しない
7. 新しいlintルールを追加する際、既存コードへの変更が最小限で済む構造になっている
8. `cargo test` で lint 機能のテストが通る
9. `cargo clippy` が警告なしで通る

## 10. Verification Strategy
- **進捗検証**: 各タスク完了後に `cargo check` と `cargo test` を実行し、コンパイル・テストが通ることを確認
- **達成検証**: `vec::new()` を含むテスト用 `.mc` ファイルに対して `moca lint` を実行し、期待する警告が出力されることを確認。また、警告のないファイルで終了コード0を確認
- **漏れ検出**: Acceptance Criteria の各項目に対応するテストケースが存在することを確認

## 11. Test Plan

### Test 1: vec::new() の検出
- **Given**: `let v = vec::new();` を含む `.mc` ファイル
- **When**: `moca lint <file>` を実行
- **Then**: stdout に `warning: prefer-new-literal: use `new Vec<T> {}` instead of `vec::new()`` と該当行の位置情報が出力され、終了コードが1

### Test 2: 警告なしのファイル
- **Given**: `vec::new()` を含まない正常な `.mc` ファイル
- **When**: `moca lint <file>` を実行
- **Then**: stdout に何も出力されず、終了コードが0

### Test 3: typecheckエラーのあるファイル
- **Given**: 型エラーを含む `.mc` ファイル
- **When**: `moca lint <file>` を実行
- **Then**: typecheckエラーが表示され、lint結果は出力されない
