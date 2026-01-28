# Spec.md

## 1. Goal
- JITの有無でパフォーマンスが悪化していないことをCIで自動検証し、JIT有効時に10%以上の改善がなければテスト失敗とする

## 2. Non-Goals
- 詳細なパフォーマンスレポート生成（Criterionの役割）
- 複数回実行による統計的分析
- プラットフォーム別の閾値設定

## 3. Target Users
- 開発者がPR作成時にJIT最適化の効果を自動検証
- CIがマージ前にパフォーマンス劣化を検出

## 4. Core User Flow
1. `cargo test --features jit` を実行
2. 各ベンチマークシナリオでJIT無効版を実行し、実行時間を計測
3. 同シナリオでJIT有効版を実行し、実行時間を計測
4. `optimized <= baseline * 0.9` を検証
5. 閾値未達成の場合、テスト失敗（CI red）

## 5. Inputs & Outputs
**入力:**
- ベンチマークソースコード（Rust内で文字列として定義）
- RuntimeConfig（JIT on/off、threshold設定）

**出力:**
- テスト結果（pass/fail）
- 失敗時: baseline時間、optimized時間、改善率を表示

## 6. Tech Stack
- 言語: Rust
- テストフレームワーク: cargo test（標準）
- 時間計測: `std::time::Instant`
- JIT制御: `RuntimeConfig` + compiler API (`run_file_capturing_output`)
- 配置: `tests/perf_benchmark.rs`

## 7. Rules & Constraints
- JITは `--features jit` フラグで有効化
- JIT無効時は `jit_threshold = u32::MAX` で実質無効化
- JIT有効時は `jit_threshold = 1` で即座にJIT化
- 閾値: optimized <= baseline * 0.9（10%以上改善）
- 1回の実行で判定（複数回実行による平均化はしない）
- 既存の `benches/vm_benchmark.rs` は削除

## 8. Open Questions
- なし

## 9. Acceptance Criteria
1. `cargo test --features jit perf_` でベンチマークテストが実行される
2. JIT有効時に10%以上改善していればテストがpass
3. JIT有効時に10%未満の改善（または悪化）ならテストがfail
4. 失敗時にbaseline時間、optimized時間、改善率が出力される
5. 以下のシナリオがテストされる: sum_loop, nested_loop, hot_function, fibonacci, array_operations
6. 既存の `benches/vm_benchmark.rs` が削除されている
7. GitHub Actions CIで `cargo test --features jit` が実行可能

## 10. Verification Strategy
**進捗検証:**
- 各シナリオ実装後に `cargo test --features jit perf_<scenario>` で個別確認

**達成検証:**
- `cargo test --features jit` で全テストがpassすること
- Acceptance Criteriaのチェックリストを手動確認

**漏れ検出:**
- 既存ベンチマークのシナリオが全て移行されていることを確認
- `benches/vm_benchmark.rs` が削除されていることを確認

## 11. Test Plan

### e2e シナリオ 1: JIT改善が閾値を超える場合
- **Given**: hot_function シナリオ（JITの恩恵が大きい）
- **When**: `cargo test --features jit perf_hot_function` を実行
- **Then**: テストがpassし、改善率が表示される

### e2e シナリオ 2: テスト失敗時の出力確認
- **Given**: 閾値を0.5（50%改善必須）に一時変更
- **When**: テストを実行
- **Then**: 失敗し、baseline時間、optimized時間、改善率が出力される

### e2e シナリオ 3: CIでの実行
- **Given**: GitHub Actions workflow
- **When**: PRを作成
- **Then**: `cargo test --features jit` が実行され、結果がCI上で確認できる
