# Spec.md

## 1. Goal
- compilerモジュール（parser, lexer, codegen, typechecker, resolver, ast, types）のテストカバレッジを95%以上に向上させる
- ファイルベースのスナップショットテストを追加してhappy pathを網羅する

## 2. Non-Goals
- `dump.rs` のカバレッジ向上（対象外）
- `mod.rs` のカバレッジ向上（対象外）
- エラーパスの100%カバレッジ（95%程度でよい）
- 新しいテストフレームワークの導入
- パフォーマンステストの追加

## 3. Target Users
- moca言語の開発者
- コントリビューター（テストを参考にして機能理解）

## 4. Core User Flow
1. 開発者が `cargo llvm-cov` を実行
2. compilerモジュールのカバレッジが95%以上であることを確認
3. 新機能追加時は対応するテストケースを `tests/snapshots/` に追加

## 5. Inputs & Outputs
- **入力**: `.mc` ファイル（mocaソースコード）
- **出力**:
  - `.stdout` ファイル（標準出力の期待値）
  - `.stderr` ファイル（標準エラーの期待値、エラーケース用）
  - `.exitcode` ファイル（終了コードの期待値）

## 6. Tech Stack
- 言語: Rust (Edition 2024)
- テストフレームワーク: Rust標準テスト (`#[test]`)
- カバレッジツール: cargo-llvm-cov
- テスト形式: 既存のスナップショットテスト (`tests/snapshot_tests.rs`)

## 7. Rules & Constraints
- 既存の `tests/snapshots/` ディレクトリ構造に従う
- テストケースは `.mc` + `.stdout` / `.stderr` / `.exitcode` の組み合わせ
- ファイル名は機能を表す命名（例: `array_indexing.mc`, `type_mismatch_error.mc`）
- 1つのテストケースは1つの機能/エッジケースをテスト
- 既存テストの整理・リネームは許可

## 8. Open Questions
- なし

## 9. Acceptance Criteria（最大10個）
1. parser.rs のラインカバレッジが95%以上である
2. lexer.rs のラインカバレッジが95%以上である
3. codegen.rs のラインカバレッジが95%以上である
4. typechecker.rs のラインカバレッジが95%以上である
5. resolver.rs のラインカバレッジが95%以上である
6. ast.rs のラインカバレッジが95%以上である
7. types.rs のラインカバレッジが95%以上である
8. 全テストが `cargo test` で成功する
9. 追加したテストケースが既存のスナップショット形式に準拠している

## 10. Verification Strategy
- **進捗検証**: 各モジュールのテストケース追加後に `cargo llvm-cov --summary-only` でカバレッジを確認
- **達成検証**: 最終的に対象7ファイル全てが95%以上であることを `cargo llvm-cov` で確認
- **漏れ検出**: `cargo llvm-cov` の未カバー行レポートを確認し、happy pathの漏れがないことを検証

## 11. Test Plan

### e2e シナリオ 1: 基本的な言語機能のカバレッジ
- **Given**: 算術演算、変数宣言、関数定義などの基本機能テストが存在する
- **When**: `cargo test` を実行
- **Then**: 全テストがパスし、parser/lexer/codegenのカバレッジが95%以上

### e2e シナリオ 2: 型システムのカバレッジ
- **Given**: 型推論、型エラー、ジェネリクスなどの型関連テストが存在する
- **When**: `cargo test` を実行
- **Then**: 全テストがパスし、typechecker/types/resolverのカバレッジが95%以上

### e2e シナリオ 3: エラーケースのカバレッジ
- **Given**: 構文エラー、型エラー、未定義変数などのエラーテストが存在する
- **When**: `cargo test` を実行
- **Then**: 全テストがパスし、エラーハンドリングパスが適切にカバーされている
