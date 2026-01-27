# Spec.md

## 1. Goal
- `.mc` ファイルを `tests/snapshots/` に追加するだけでカバレッジに反映されるようにする

## 2. Non-Goals
- 新しいテストフレームワークの導入
- 外部クレートの追加
- JIT関連のin-processテスト対応（既存のsubprocess方式を維持）

## 3. Target Users
- moca言語の開発者
- テストケースを追加するコントリビューター

## 4. Core User Flow
1. 開発者が `tests/snapshots/basic/new_feature.mc` を作成
2. 対応する `new_feature.stdout` を作成
3. `cargo test` を実行 → テストが自動的に実行される
4. `cargo llvm-cov` を実行 → カバレッジに反映される

## 5. Inputs & Outputs
- **入力**: `.mc` ファイル + `.stdout` / `.stderr` / `.exitcode` ファイル
- **出力**: テスト結果（pass/fail）、カバレッジレポート

## 6. Tech Stack
- 言語: Rust (Edition 2024)
- 追加依存: なし（標準ライブラリの `std::io::Write` のみ使用）

## 7. Rules & Constraints
- VMの `println!` を `Write` トレイトベースに変更
- デフォルトは `stdout` への出力（既存動作を維持）
- `snapshot_tests.rs` を修正してin-process実行に変更
- 既存のAPIシグネチャは可能な限り維持

## 8. Open Questions
- なし

## 9. Acceptance Criteria
1. `tests/snapshots/basic/*.mc` がin-processで実行される
2. `tests/snapshots/errors/*.mc` がin-processで実行される
3. `.stdout` ファイルとの比較が正常に動作する
4. `.stderr` ファイルとの比較が正常に動作する
5. `.exitcode` ファイルとの比較が正常に動作する
6. `cargo llvm-cov` でcompilerモジュールのカバレッジが計測される
7. 既存の `cargo test` が全てパスする
8. JITテスト（`jit/`）は既存のsubprocess方式を維持する

## 10. Verification Strategy
- **進捗検証**: 各ステップ完了後に `cargo test` でテストがパスすることを確認
- **達成検証**: `cargo llvm-cov` で `tests/snapshots/` のテストがカバレッジに反映されることを確認
- **漏れ検出**: 既存のsnapshot testが全て動作することを確認

## 11. Test Plan

### e2e シナリオ 1: 基本テストのカバレッジ反映
- **Given**: `tests/snapshots/basic/arithmetic.mc` が存在する
- **When**: `cargo llvm-cov --test snapshot_tests` を実行
- **Then**: compiler/parser.rs などのカバレッジが計測される

### e2e シナリオ 2: エラーテストのカバレッジ反映
- **Given**: `tests/snapshots/errors/type_mismatch.mc` が存在する
- **When**: `cargo llvm-cov --test snapshot_tests` を実行
- **Then**: compiler/typechecker.rs のエラーパスがカバレッジに反映される

### e2e シナリオ 3: 新規ファイル追加時の自動検出
- **Given**: 新しい `tests/snapshots/basic/new_test.mc` を追加
- **When**: `cargo test` を実行
- **Then**: 新しいテストが自動的に実行される
