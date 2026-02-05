# Spec.md

## 1. Goal
- snapshot_tests.rsのパフォーマンステストにおいて、RustとmocaのI/O条件を揃え、公平な比較ができるようにする

## 2. Non-Goals
- ベンチマーク自体のリファクタリング
- 新しいテストケースの追加
- CI/CDへの変更
- パフォーマンス改善（計測の公平性のみが目的）

## 3. Target Users
- mocaの開発者
- パフォーマンス比較レポートを参照する人

## 4. Core User Flow
1. `cargo test snapshot_performance --features jit` を実行
2. Rust版・moca版ともにstdoutへの出力＋キャプチャを含む条件で計測される
3. 両者の実行時間が表示され、公平に比較できる

## 5. Inputs & Outputs
- **入力**: 各ベンチマークのパラメータ（既存と同じ）
- **出力**:
  - 計算結果（stdout経由でキャプチャ）
  - 実行時間の計測結果

## 6. Tech Stack
- 言語: Rust（既存）
- I/Oキャプチャ: `std::io::Cursor<Vec<u8>>` + `Write` trait
- 新規依存: なし

## 7. Rules & Constraints
- mocaの実装方式に合わせる（`Cursor` によるインプロセスキャプチャ）
- 既存の出力一致チェック（`assert_eq!`）は維持する
- 計測対象に含めるもの：
  - 計算処理
  - stdoutへの書き込み（`write!`/`writeln!`）
  - 出力のキャプチャ

## 8. Open Questions
なし

## 9. Acceptance Criteria
1. [ ] `rust_sum_loop`関数がWriterに出力し、その出力がキャプチャされる形式になっている
2. [ ] `rust_nested_loop`関数がWriterに出力し、その出力がキャプチャされる形式になっている
3. [ ] `rust_fibonacci`関数がWriterに出力し、その出力がキャプチャされる形式になっている
4. [ ] `rust_mandelbrot`関数がWriterに出力し、その出力がキャプチャされる形式になっている
5. [ ] Rust版の計測時間にstdout出力＋キャプチャのコストが含まれている
6. [ ] moca版とRust版の出力一致チェックが引き続き動作する
7. [ ] `cargo test snapshot_performance --features jit` がパスする
8. [ ] `cargo test` が全てパスする

## 10. Verification Strategy
- **進捗検証**: 各関数修正後に `cargo check` を実行して動作確認
- **達成検証**: `cargo test --features jit` が全てパスすること
- **漏れ検出**: 出力一致チェック（`assert_eq!`）により、実装の正しさを担保

## 11. Test Plan

### E2E シナリオ 1: 基本動作確認
- **Given**: 修正後のコード
- **When**: `cargo test snapshot_performance --features jit` を実行
- **Then**: テストがパスし、Rust/mocaの両方に妥当な実行時間が表示される

### E2E シナリオ 2: 出力一致確認
- **Given**: 修正後のコード
- **When**: 各パフォーマンステストを実行
- **Then**: Rust版とmoca版の出力が一致する

### E2E シナリオ 3: 全テスト通過
- **Given**: 修正後のコード
- **When**: `cargo test` を実行
- **Then**: 全てのテストがパスする
