# Spec.md - JIT Call命令と再帰関数サポート

## 1. Goal
- JITコンパイルされた関数内からの関数呼び出し（Call命令）をサポートし、再帰関数を含むユーザー定義関数をJIT実行できるようにする

## 2. Non-Goals
- AArch64のCall命令サポート（x86-64のみ）
- 組み込み関数（push, len等）のJIT内呼び出し最適化
- クロージャのJITサポート
- 末尾呼び出し最適化
- スタックオーバーフロー対策（OSのスタック制限に任せる）

## 3. Target Users
- mocaプログラムを実行するユーザー
- 再帰アルゴリズムや関数呼び出しを多用するコードでJIT最適化の恩恵を受けたいユーザー

## 4. Core User Flow
1. ユーザーがmocaプログラムを実行（`moca run --jit on`）
2. 関数が呼び出し閾値に達するとJITコンパイルされる
3. JITコンパイルされた関数内でCall命令が実行される
4. 呼び出し先がJITコンパイル済みなら、JITコードを直接呼び出す
5. 呼び出し先が未コンパイルなら、VMヘルパー経由でインタプリタ実行
6. 戻り値がJITスタックにプッシュされ、実行継続

## 5. Inputs & Outputs
### Inputs
- Call命令を含むmoca関数のバイトコード
- JitCallContext（VMポインタ、chunkポインタ、ヘルパー関数ポインタ）

### Outputs
- JITコンパイルされたネイティブコード（Call命令対応）
- 関数の戻り値（JitReturn: tag + payload）

## 6. Tech Stack
- 言語: Rust
- アーキテクチャ: x86-64
- JITアセンブラ: 自前実装（src/jit/x86_64.rs）
- テスト: cargo test --features jit

## 7. Rules & Constraints
### 振る舞いのルール
- JIT関数からの呼び出し先がJITコンパイル済みの場合、直接JITコードを呼び出す
- 呼び出し先が未コンパイルの場合、jit_call_helper経由でVM実行
- 組み込み関数の呼び出しはインタプリタにフォールバック

### 技術的制約
- System V AMD64 ABIに準拠（RDI, RSI, RDX, RCX で引数渡し）
- callee-savedレジスタ（R12-R15, RBX, RBP）を保存・復元
- JITスタック（VSTACK）は16バイトアラインメントのValue構造

### 前提
- 現在のCall命令スキップ（compiler_x86_64.rs:102-108）を削除する
- emit_call実装は既存のものをベースに修正

## 8. Open Questions
なし

## 9. Acceptance Criteria（最大10個）
1. [ ] Call命令を含む関数がJITコンパイルされる（エラーにならない）
2. [ ] JIT関数から別のJIT関数を呼び出せる
3. [ ] JIT関数から未コンパイル関数を呼び出せる（インタプリタ実行）
4. [ ] 再帰関数（fib等）がJIT実行で正しい結果を返す
5. [ ] `perf_fibonacci`ベンチマークで10%以上の改善
6. [ ] `perf_hot_function`ベンチマークで10%以上の改善
7. [ ] `--trace-jit`で再帰呼び出しがJIT経由であることを確認できる
8. [ ] 既存のJITテスト（sum_loop, nested_loop）が引き続きパスする

## 10. Verification Strategy
### 進捗検証
- 各タスク完了時に`cargo test --features jit`を実行
- `--trace-jit`オプションでJIT呼び出しログを確認

### 達成検証
- `cargo test --features jit perf_`で全ベンチマークテストがパス
- trace-jitで`fib`関数が複数回JIT実行されていることを確認

### 漏れ検出
- 既存テストスイート（`cargo test --features jit`）が全てパス
- 手動で`fib(20)`等を実行し、正しい結果と高速化を確認

## 11. Test Plan

### e2e シナリオ 1: 再帰関数のJIT実行
```
Given: fib(n)を定義したmocaプログラム
When: --jit on --jit-threshold 1で実行
Then: fib(20)が正しい結果(6765)を返し、baselineより高速
```

### e2e シナリオ 2: 相互再帰関数
```
Given: is_even(n)とis_odd(n)が相互に呼び出すプログラム
When: --jit on --jit-threshold 1で実行
Then: is_even(10)がtrueを返し、両関数がJIT実行される
```

### e2e シナリオ 3: ホット関数呼び出し
```
Given: ループ内でdo_work(n)を10000回呼び出すプログラム
When: --jit on --jit-threshold 1で実行
Then: 正しい結果を返し、perf_hot_functionが10%以上改善
```
