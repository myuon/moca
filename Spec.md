# Spec.md

## 1. Goal
- VMから不要な動的最適化機構（Quickening, Inline Cache）を削除し、静的型付け言語として適切なシンプルな構造にする

## 2. Non-Goals
- JIT機能の変更・削除（維持する）
- パフォーマンス改善（今回は対象外）
- コンパイラによる型特殊化命令の生成（将来課題）
- 新機能の追加

## 3. Target Users
- mocaコンパイラ/VMの開発者
- コードベースを理解しようとする貢献者

## 4. Core User Flow
1. 開発者がVMコードを読む
2. 不要な動的最適化コードがなく、シンプルに理解できる
3. `cargo test` で全テストがパスする
4. 既存のmocaプログラムが正常に動作する

## 5. Inputs & Outputs
- **入力**: 現在のVMコード（quickening, inline cache, 特殊化命令を含む）
- **出力**: クリーンアップされたVMコード（上記を削除済み）

## 6. Tech Stack
- 言語: Rust
- テスト: cargo test + insta (snapshot tests)

## 7. Rules & Constraints
- JIT機能は一切変更しない
- 既存の全テスト（unit, snapshot）をパスさせる
- 既存のmocaプログラムの動作を維持する
- 削除対象:
  - Quickening関連コード（`run_with_quickening`, `execute_op_quickening`, `quicken_instruction`）
  - Inline Cache関連コード（`ic.rs`, `ic_tables`, `main_ic`）
  - 型特殊化命令（`AddI64`, `SubI64`, `MulI64`, `DivI64`, `AddF64`, `SubF64`, `MulF64`, `DivF64`）
  - 比較特殊化命令（`LtI64`, `LeI64`, `GtI64`, `GeI64`, `LtF64`）
  - Cached命令（`GetFCached`, `SetFCached`）
  - `ArrayGetInt`（Quickening用の特殊化命令）

## 8. Open Questions
- なし

## 9. Acceptance Criteria
1. `cargo test` が全てパスする
2. `src/vm/ic.rs` が削除されている
3. `Op::AddI64`, `Op::SubI64` 等の型特殊化命令が `ops.rs` から削除されている
4. `Op::GetFCached`, `Op::SetFCached` が削除されている
5. `VM::run_with_quickening` メソッドが削除されている
6. `VM` 構造体から `ic_tables`, `main_ic` フィールドが削除されている
7. コンパイラが `VM::run` を直接呼び出すようになっている（quickeningモード分岐なし）
8. JIT関連コード（`should_jit_compile`, `jit_threshold`, `call_counts`等）が維持されている
9. 既存のスナップショットテストが全てパスする
10. `#![allow(dead_code)]` の不要な箇所が削除されている

## 10. Verification Strategy
- **進捗検証**: 各削除作業後に `cargo test` を実行し、テストがパスすることを確認
- **達成検証**: 全Acceptance Criteriaをチェックリストで確認
- **漏れ検出**: `grep -r "quicken\|inline_cache\|AddI64\|GetFCached"` で残存コードがないことを確認

## 11. Test Plan

### Scenario 1: 基本的な算術演算
- **Given**: 整数・浮動小数点の四則演算を含むmocaプログラム
- **When**: コンパイル・実行する
- **Then**: 正しい計算結果が得られる（既存スナップショットテストで検証）

### Scenario 2: オブジェクトプロパティアクセス
- **Given**: オブジェクトのフィールドアクセスを含むmocaプログラム
- **When**: コンパイル・実行する
- **Then**: 正しくプロパティが取得・設定できる

### Scenario 3: JIT機能の維持確認
- **Given**: JitMode::On でホット関数を含むプログラム
- **When**: 実行する
- **Then**: JITコンパイルが発動する（trace_jit有効時にログ出力）
