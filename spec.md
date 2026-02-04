# Spec.md - Hot Loop JIT (Tracing JIT)

## 1. Goal
- ホットループ（頻繁に実行されるループ）を検出し、JITコンパイルすることで、`mandelbrot`のような1回しか呼ばれない関数内のループでも高速化できるようにする

## 2. Non-Goals
- OSR (On-Stack Replacement): ループ実行中のJITコード切り替えは行わない（次回ループ開始時から適用）
- 呼び出し先関数のインライン展開: トレースはループ内のみ
- ガード失敗時の高度なdeoptimization
- 複数パスのトレース合成
- 型プロファイリングによる投機的最適化

## 3. Target Users
- mocaで数値計算やシミュレーションを行うユーザー
- 1回しか呼ばれないが内部で大量のループを回す関数を持つプログラムの実行者

## 4. Core User Flow
1. ユーザーがmocaプログラムを`--jit=auto`または`--jit=on`で実行
2. VMがループのbackward branchを検出し、実行回数をカウント
3. カウントが閾値に達したらループをホットと判定
4. ループ本体をJITコンパイル（トレース記録→ネイティブコード生成）
5. 次回のループ開始時からJITコードを実行
6. ループ内から呼び出される関数が別途ホットになれば、その関数もJITコンパイルされる（JIT→JIT呼び出し）

## 5. Inputs & Outputs

### Inputs
- bytecode: ループを含む関数のバイトコード
- backward branch実行回数: ホット判定の基準

### Outputs
- JITコンパイルされたループのネイティブコード
- `--trace-jit`オプション時のデバッグ出力:
  - `[JIT] Hot loop detected at PC <n> (iterations: <count>)`
  - `[JIT] Compiled loop at PC <n> (<size> bytes)`

## 6. Tech Stack
- 言語: Rust
- JIT基盤: 既存の`src/jit/compiler_x86_64.rs`（x86_64）、`src/jit/compiler.rs`（aarch64）
- テスト: `cargo test` + snapshot tests
- ベンチマーク: `bench/moca/mandelbrot.mc`

## 7. Rules & Constraints

### ホット検出ルール
- backward branch（`Op::Jmp`でtarget < current_pc）の実行回数をカウント
- 閾値（デフォルト1000、`--jit-threshold`で設定可能）に達したらホットと判定
- 既存の関数単位JITの閾値設定と共用してよい

### トレース範囲
- ループ開始（backward branchのターゲット）からbackward branchまで
- ループ内の関数呼び出しはインライン展開しない（通常のCall命令として処理）

### 既存JITとの共存
- 関数単位JIT: 関数呼び出し回数でホット判定（既存のまま）
- ループJIT: backward branch実行回数でホット判定（新規）
- 両方が独立して動作する

### アーキテクチャ
- x86_64とaarch64の両方をサポート
- 既存のJITコード生成ロジックを最大限再利用

### 制約
- サポートされていないOp（Dup, Mod, Not, Heap操作など）がループ内にある場合はJITコンパイルをスキップ（インタプリタ実行にフォールバック）
- GC safepointは既存の設計に従う

## 8. Open Questions
- なし

## 9. Acceptance Criteria

1. **backward branchでループ実行回数がカウントされる**
2. **閾値に達したループが「ホット」として検出される**
3. **ホットループがx86_64向けにJITコンパイルされる**
4. **ホットループがaarch64向けにJITコンパイルされる**
5. **`--trace-jit`でホットループ検出とコンパイルのログが出力される**
6. **JITコンパイルされたループが正しく実行される（既存テストがパス）**
7. **mandelbrot.mcでループJITが適用される**
8. **既存の関数単位JITが引き続き動作する**
9. **サポートされていないOpを含むループはスキップされ、インタプリタで実行される**
10. **`cargo test`、`cargo clippy`がパスする**

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に`cargo test`でリグレッションがないことを確認
- `--trace-jit`オプションでホットループ検出・コンパイルの動作を目視確認

### 達成検証
- mandelbrot.mcを`--trace-jit`付きで実行し、ループJITが適用されることを確認
- Acceptance Criteriaの全項目をチェックリストで確認

### 漏れ検出
- 既存のJITテスト（`src/jit/compiler_x86_64.rs`、`src/jit/compiler.rs`内のテスト）が全てパス
- snapshot testsで出力が期待通りであることを確認

## 11. Test Plan

### E2E Scenario 1: ホットループの検出とJITコンパイル
```
Given: 単純なwhileループを含むプログラム
When: --jit=auto --trace-jit で実行
Then: "[JIT] Hot loop detected" と "[JIT] Compiled loop" がログに出力される
```

### E2E Scenario 2: mandelbrotでのループJIT適用
```
Given: examples/mandelbrot.mc
When: --jit=auto --trace-jit で実行
Then: mandelbrot関数内のwhileループがJITコンパイルされ、正しい出力が得られる
```

### E2E Scenario 3: 未サポートOpを含むループのフォールバック
```
Given: Dup操作（論理演算子&&など）を含むwhileループ
When: --jit=auto --trace-jit で実行
Then: JITコンパイルがスキップされ、インタプリタで正しく実行される
```
