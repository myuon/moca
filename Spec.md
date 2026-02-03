# Spec.md - JIT拡張: PushString, ArrayLen, Syscall対応

## 1. Goal
- `examples/mandelbrot.mc` がJITコンパイルされ、インタプリタより高速に実行できるようになる
- JITコンパイラが `PushString`, `ArrayLen`, `Syscall` 操作をサポートする

## 2. Non-Goals
- JIT対応操作の全網羅（今回は mandelbrot.mc に必要な操作のみ）
- JIT最適化（インライン化、ループ最適化等）
- aarch64の動作確認（ユーザーが実施）

## 3. Target Users
- moca言語ユーザー（JITによる高速実行を期待）
- moca開発者（JITコンパイラの拡張）

## 4. Core User Flow
1. ユーザーが `moca run examples/mandelbrot.mc` を実行
2. ホット関数（print_char等）が閾値に達する
3. JITコンパイラが関数をネイティブコードにコンパイル
4. 以降の呼び出しはJITコードで実行される
5. 出力結果はインタプリタ実行と完全一致

## 5. Inputs & Outputs
### Inputs
- moca バイトコード（Op::PushString, Op::ArrayLen, Op::Syscall を含む）
- 文字列定数テーブル（chunk.strings）

### Outputs
- ネイティブコード（x86_64, aarch64）
- 実行結果（インタプリタと同一）

## 6. Tech Stack
- 言語: Rust
- アーキテクチャ: x86_64, aarch64
- 実装方式: ヘルパー関数経由（既存の jit_call_helper パターン）
- テスト: cargo test

## 7. Rules & Constraints
- 既存の `jit_call_helper` パターンを踏襲
- ヘルパー関数は `unsafe extern "C"` で定義
- JitCallContext 経由で VM/Chunk にアクセス
- 結果は JitReturn/JitValue 形式で返す
- 既存テストが全て通ること

## 8. Open Questions
- なし（VMの実装を踏襲）

## 9. Acceptance Criteria
1. `cargo run -- run examples/mandelbrot.mc --trace-jit` で `print_char` 関数がJITコンパイルされる
2. JITコンパイル時に "Unsupported operation" エラーが出ない
3. JIT実行時の出力がインタプリタ実行と完全一致する
4. `cargo test mandelbrot` が全てパスする
5. x86_64 でJITコンパイル・実行が成功する
6. aarch64 でJITコンパイル・実行が成功する（ユーザー確認）
7. JIT実行がインタプリタより高速（目安: 2倍以上）
8. `cargo test` が全てパスする（既存テストへの影響なし）
9. `cargo clippy` が警告なしでパスする

## 10. Verification Strategy
### 進捗検証
- 各操作の実装後に `--trace-jit` で該当操作のコンパイル成功を確認
- 単体テストで出力一致を確認

### 達成検証
- `examples/mandelbrot.mc` がJITコンパイル・実行され、出力が一致
- インタプリタより高速であることを計測

### 漏れ検出
- `cargo test` 全パス
- `cargo clippy` 警告なし

## 11. Test Plan

### Scenario 1: JITコンパイル成功
- **Given**: JIT機能が有効
- **When**: `examples/mandelbrot.mc` を `--trace-jit --jit-threshold 100` で実行
- **Then**: `print_char` 関数が "[JIT] Compiled function" と表示される

### Scenario 2: 出力一致
- **Given**: JIT機能が有効
- **When**: `examples/mandelbrot.mc` を実行
- **Then**: 出力が `mandelbrot_rust(100)` と完全一致

### Scenario 3: 性能向上
- **Given**: JIT機能が有効、閾値を1に設定
- **When**: `mandelbrot.mc` を実行し時間計測
- **Then**: インタプリタのみの実行より高速（目安: 2倍以上）
