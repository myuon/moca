# Spec.md

## 1. Goal
- `moca run --timings` オプションでコンパイラパイプラインの各フェーズ（import, lexer, parser, typecheck, desugar, monomorphise, resolve, codegen, execution）の実行時間を計測・出力できるようにする

## 2. Non-Goals
- `check` や `test` コマンドへの `--timings` 対応（今回は `run` のみ）
- パフォーマンス最適化の実施（計測機能の追加のみ）
- 計測結果のファイル出力機能
- 計測結果の統計分析機能（平均、中央値など）

## 3. Target Users
- moca言語の開発者（コンパイラのボトルネック調査）
- moca言語のユーザー（実行時間の内訳確認）

## 4. Core User Flow
1. ユーザーが `moca run --timings examples/hello.moca` を実行
2. プログラムが通常通り実行される
3. 実行完了後、stderrに各フェーズの計測結果がテーブル形式で出力される

### バリエーション
- `--timings` または `--timings=human`: テーブル形式で出力
- `--timings=json`: JSON形式で出力

## 5. Inputs & Outputs

### Inputs
- CLIオプション: `--timings[=human|json]`
- 対象ソースファイル

### Outputs（stderr）

**human形式（デフォルト）:**
```
=== Compiler Timings ===
lexer:             2.12ms
parser:            2.13ms
typecheck:         1.79ms
desugar:           0.30ms
monomorphise:      2.57ms
resolve:           1.84ms
codegen:           0.82ms
execution:         0.19ms
------------------------
total:            11.77ms
```

**json形式:**
```json
{"lexer_ms":2.12,"parser_ms":2.13,"typecheck_ms":1.79,"desugar_ms":0.30,"monomorphise_ms":2.57,"resolve_ms":1.84,"codegen_ms":0.82,"execution_ms":0.19,"total_ms":11.77}
```

### 時間表示の精度
- 1秒以上: `1.23s` 形式
- 1秒未満: `123.45ms` 形式

## 6. Tech Stack
- 言語: Rust（既存）
- CLI引数処理: clap（既存）
- 時間計測: `std::time::Instant`（標準ライブラリ）
- JSON出力: 手動フォーマットまたは既存のserde_json

## 7. Rules & Constraints
- 計測結果はstderrに出力する（プログラムのstdoutを汚さない）
- `--timings` がない場合は計測処理自体をスキップしてオーバーヘッドを最小化
- 各フェーズの時間の合計がtotalと一致すること
- 既存の `--dump-*` オプションとの併用が可能であること
- 既存のテストが壊れないこと

## 8. Open Questions
なし

## 9. Acceptance Criteria
1. `moca run --timings file.moca` で計測結果がstderrにテーブル形式で出力される
2. `moca run --timings=human file.moca` で計測結果がstderrにテーブル形式で出力される
3. `moca run --timings=json file.moca` で計測結果がstderrにJSON形式で出力される
4. 計測対象は lexer, parser, typecheck, desugar, monomorphise, resolve, codegen, execution の8フェーズ
5. totalが各フェーズの合計と一致する
6. 1秒以上の時間は `Xs` 形式、1秒未満は `Xms` 形式で表示される
7. `--timings` なしで実行した場合、計測結果は出力されない
8. 既存の `--dump-ast` などのオプションと併用できる
9. `cargo test` が全てパスする
10. `cargo clippy` で警告が出ない

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo check` でコンパイルエラーがないことを確認
- 実装途中でも `moca run --timings examples/hello.moca` を実行して動作確認

### 達成検証
- Acceptance Criteria の10項目をチェックリストで確認
- 実際に `--timings` と `--timings=json` の両方を実行して出力を目視確認

### 漏れ検出
- `cargo test` で既存テストが壊れていないことを確認
- `cargo clippy` で品質チェック

## 11. Test Plan

### E2E シナリオ 1: 基本的な計測出力
- **Given**: 有効なmocaソースファイルが存在する
- **When**: `moca run --timings examples/hello.moca` を実行
- **Then**: stderrに9フェーズ + totalの計測結果がテーブル形式で出力され、プログラムは正常終了する

### E2E シナリオ 2: JSON形式出力
- **Given**: 有効なmocaソースファイルが存在する
- **When**: `moca run --timings=json examples/hello.moca` を実行
- **Then**: stderrに有効なJSON形式で計測結果が出力される

### E2E シナリオ 3: 他オプションとの併用
- **Given**: 有効なmocaソースファイルが存在する
- **When**: `moca run --timings --dump-ast examples/hello.moca` を実行
- **Then**: ASTダンプと計測結果の両方が出力される
