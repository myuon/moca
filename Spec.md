# Spec.md

## 1. Goal
- GCが正しく動作していることをsnapshotテストで検証できるようにする
- 同一ソースコードに対して「GCあり→成功」「GCなし→ヒープ溢れエラー」を確認

## 2. Non-Goals
- CLIオプションの追加（compiler API経由で実装）
- 並行GCの検証
- GCのパフォーマンス測定

## 3. Target Users
- moca言語の開発者（GCの動作確認）

## 4. Core User Flow
1. `tests/snapshots/gc/` に `.mc` ファイルを作成
2. `.stdout` にGCあり時の期待出力を定義
3. `.gc_disabled.stdout` にGCなし時の期待出力（エラーメッセージ）を定義
4. `cargo test snapshot_gc` を実行
5. 両方のケースがpassすることを確認

## 5. Inputs & Outputs

### Inputs
- `.mc` ファイル: ヒープオブジェクトを大量に作成→破棄するmocaコード
- `.stdout`: GCありの期待出力
- `.gc_disabled.stdout`: GCなしの期待出力（エラーメッセージを含む）
- `.gc_disabled.exitcode`: GCなし時の期待exit code（デフォルト: 1）

### Outputs
- テスト実行結果（pass/fail）

## 6. Tech Stack
- 言語: Rust（既存プロジェクト）
- テストフレームワーク: Rust標準テスト + snapshot_tests.rs

## 7. Rules & Constraints

### Heap制限
- `heap_limit` をcompiler APIで設定可能にする
- デフォルト: 無制限（`None`）
- テスト用: 小さい値（例: 10KB〜100KB程度）

### GC無効化
- `gc_enabled: bool` をcompiler APIで設定可能にする
- デフォルト: `true`（GC有効）

### エラー形式
- ヒープ溢れ時: `runtime error: heap limit exceeded (allocated: Xbytes, limit: Ybytes)`
- exit code: 1

### snapshotテストの拡張
- `.gc_disabled.stdout` が存在する場合、GC無効モードでも実行
- `.gc_disabled.stderr` でエラー出力を検証（部分一致）
- `.gc_disabled.exitcode` でexit codeを検証（デフォルト: 1）

## 8. Open Questions
なし

## 9. Acceptance Criteria

1. [ ] `Heap::new_with_limit(limit: Option<usize>)` が実装されている
2. [ ] `Heap` にGC有効/無効フラグが追加されている
3. [ ] ヒープ制限超過時に `runtime error: heap limit exceeded` エラーが発生する
4. [ ] `snapshot_tests.rs` が `.gc_disabled.*` ファイルを認識してGC無効テストを実行する
5. [ ] `tests/snapshots/gc/` にGC検証用テストケースが存在する
6. [ ] GCありで同テストが成功する（正常終了）
7. [ ] GCなしで同テストがヒープ溢れエラーになる
8. [ ] `cargo test snapshot_gc` が通る

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo test` でビルド・テスト通過を確認

### 達成検証
- `cargo test snapshot_gc` が通る
- GCなしケースでヒープ溢れエラーが出力される
- GCありケースで正常終了する

### 漏れ検出
- Acceptance Criteriaを1つずつチェック

## 11. Test Plan

### E2E Scenario 1: GCありで大量オブジェクト作成が成功
- **Given**: ヒープオブジェクトを10000回作成→破棄するコード
- **When**: GC有効（デフォルト）で実行
- **Then**: 正常終了し "done" が出力される

### E2E Scenario 2: GCなしでヒープ溢れエラー
- **Given**: 同上のコード
- **When**: GC無効 + heap_limit=小さい値 で実行
- **Then**: `heap limit exceeded` エラーで exit code 1

### E2E Scenario 3: snapshot_gc テスト通過
- **Given**: `tests/snapshots/gc/` にテストファイルが存在
- **When**: `cargo test snapshot_gc` 実行
- **Then**: 全テストがpass
