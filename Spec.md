# Spec.md

## 1. Goal
- テストコードを外部ファイル（`.mica` + `.stdout`/`.stderr`）形式に移行し、テスト対象のコードと期待結果を明示的に管理できるようにする

## 2. Non-Goals
- e2e.rsの完全削除（段階的に縮小、今回は一部移行のみ）
- 新しいテストケースの追加（既存テストの移行のみ）
- stderrの完全一致検証（部分一致で対応）
- 複雑なテストフィクスチャやセットアップ機構

## 3. Target Users
- mica開発者
- テストケースを追加・修正する貢献者

## 4. Core User Flow
1. `tests/snapshots/`配下に`.mica`ファイルを作成
2. 同名の`.stdout`（または`.stderr`, `.exitcode`）で期待結果を記述
3. `cargo test`で自動実行・検証

## 5. Inputs & Outputs
- **入力**: `.mica`ファイル（テスト対象コード）
- **入力**: `.stdout`/`.stderr`/`.exitcode`ファイル（期待結果）
- **出力**: テスト結果（pass/fail）

## 6. Tech Stack
- 言語: Rust
- テストフレームワーク: Rust標準 (`#[test]`)
- テストファイル: `tests/snapshot_tests.rs`（新規作成）
- テストデータ配置: `tests/snapshots/`

## 7. Rules & Constraints

### ファイル形式
- `.mica`: テスト対象のMicaソースコード
- `.stdout`: 標準出力の期待値（完全一致）
- `.stderr`: 標準エラーの期待値（部分一致）
- `.exitcode`: 終了コード（省略時は0を期待）

### ディレクトリ構造とオプション
```
tests/snapshots/
├── basic/      # デフォルトオプションで実行
├── errors/     # エラー系テスト（exitcode != 0）
└── jit/        # --jit=on オプションで実行
```

### 検証ルール
- stdout: 完全一致
- stderr: 部分一致（期待文字列が含まれていればOK）
- exitcode: 完全一致（ファイルがなければ0を期待）

### JIT動作一致検証
- `basic/`と`jit/`に同じテストを配置することで、JIT有無で結果が変わらないことを検証可能

## 8. Open Questions
- なし

## 9. Acceptance Criteria（最大10個）
1. `tests/snapshot_tests.rs`が存在し、`cargo test`で実行される
2. `tests/snapshots/basic/`配下の`.mica`ファイルが自動検出・実行される
3. `.stdout`ファイルと実際の標準出力が完全一致で検証される
4. `.stderr`ファイルと実際の標準エラーが部分一致で検証される
5. `.exitcode`ファイルで終了コードが検証される（省略時は0）
6. `tests/snapshots/errors/`配下のエラー系テストが正しく検証される
7. `tests/snapshots/jit/`配下のテストが`--jit=on`オプションで実行される
8. 既存e2e.rsのテストのうち、最低5個が新形式に移行されている
9. `cargo test`がCIで成功する

## 10. Verification Strategy

### 進捗検証
- 各ディレクトリ（basic/, errors/, jit/）ごとにテストが動作することを確認
- `cargo test snapshot` で新テストのみ実行して確認

### 達成検証
- 全Acceptance Criteriaをチェックリストで確認
- `cargo test`が成功することを確認

### 漏れ検出
- 移行したテストと元のe2e.rsテストの対応表を作成
- 移行したテストの結果が元と同じであることを確認

## 11. Test Plan

### E2E シナリオ 1: 基本テストの実行
- **Given**: `tests/snapshots/basic/arithmetic.mica`と`arithmetic.stdout`が存在
- **When**: `cargo test`を実行
- **Then**: arithmeticテストがpassする

### E2E シナリオ 2: エラーテストの実行
- **Given**: `tests/snapshots/errors/division_by_zero.mica`と`.stderr`, `.exitcode`が存在
- **When**: `cargo test`を実行
- **Then**: division_by_zeroテストがpassする（stderr部分一致、exitcode一致）

### E2E シナリオ 3: JITモードテストの実行
- **Given**: `tests/snapshots/jit/fibonacci.mica`と`fibonacci.stdout`が存在
- **When**: `cargo test`を実行
- **Then**: `--jit=on`で実行され、stdoutが一致する
