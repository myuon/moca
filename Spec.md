# Spec.md

## 1. Goal
- JIT/FFIテストをin-process実行に移行し、subprocessを完全に廃止する
- すべてのスナップショットテストがカバレッジに反映されるようにする

## 2. Non-Goals
- CLIのフラグ解析テスト（CLIそのものは別途テストしない）
- JITの実際のネイティブコード生成テスト（JIT有効化のパステストのみ）

## 3. Target Users
- moca言語の開発者

## 4. Core User Flow
1. `tests/snapshots/jit/*.mc` ファイルを追加
2. `cargo test` で自動実行
3. `cargo llvm-cov` でカバレッジ反映

## 5. Inputs & Outputs
- **入力**: `.mc` ファイル + `.stdout` / `.stderr` ファイル
- **出力**: テスト結果、カバレッジレポート

## 6. Tech Stack
- 言語: Rust
- 追加依存: なし

## 7. Rules & Constraints
- `run_file_capturing_output` にJIT設定オプション追加
- dump用API（`compile_and_dump_ast`, `compile_and_dump_bytecode`）を追加
- `snapshot_tests.rs` から subprocess 呼び出しを完全削除

## 8. Open Questions
- なし

## 9. Acceptance Criteria
1. JITテスト（3ファイル）がin-processで実行される
2. FFIテスト（dump_ast, dump_bytecode）がin-processで実行される
3. `snapshot_tests.rs` から `Command::new` が完全に削除される
4. 全テストが `cargo test` でパスする
5. `cargo llvm-cov` で全テストがカバレッジに反映される

## 10. Verification Strategy
- **進捗検証**: 各機能実装後に `cargo test` でパス確認
- **達成検証**: `grep -r "Command::new" tests/` で subprocess 呼び出しがないことを確認
- **漏れ検出**: `cargo llvm-cov` でカバレッジ確認

## 11. Test Plan

### e2e シナリオ 1: JITテストのカバレッジ
- **Given**: `tests/snapshots/jit/fibonacci.mc` が存在
- **When**: `cargo llvm-cov` を実行
- **Then**: JIT関連コードパスがカバレッジに反映

### e2e シナリオ 2: dump APIテスト
- **Given**: `tests/snapshots/ffi/dump_ast.mc` が存在
- **When**: `cargo test` を実行
- **Then**: AST dump出力が期待値と一致
