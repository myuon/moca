# Spec.md - moca test コマンド実装

## 1. Goal
- `moca test` コマンドでリポジトリ内の `_test_` プレフィックスを持つ関数を自動検出・実行し、テスト結果を表示できるようにする
- テスト用のCompiler API (`run_tests`) を提供する

## 2. Non-Goals
- テストのフィルタリング（特定テストのみ実行）
- 並列実行
- カバレッジ計測
- テスト関数名のプレフィックス変更（後で対応予定）
- watch モード

## 3. Target Users
- moca 言語でプロジェクトを開発している開発者
- 自作の関数やロジックの動作を検証したい人

## 4. Core User Flow
1. ユーザーが `_test_` プレフィックスを持つテスト関数を `.mc` ファイルに記述
2. テスト内で `assert(condition, msg)` や `assert_eq(a, b, msg)` を使用
3. `moca test` を実行
4. 全テストが実行され、各テストの成功/失敗が表示される
5. 最後に集計結果（passed/failed 数）が表示される

## 5. Inputs & Outputs

### Inputs
- `pkg.toml` のエントリーポイント設定（デフォルト: `src/`）
- `src/` 配下の全 `.mc` ファイル

### Outputs
- 各テストの実行結果: `✓ _test_xxx passed` または `✗ _test_xxx failed: {error}`
- 集計結果: `X passed, Y failed`
- 終了コード: 全成功なら 0、1つでも失敗なら 1

## 6. Tech Stack
- 言語: Rust（既存プロジェクト）
- CLI: clap（既存）
- ファイルスキャン: std::fs（再帰実装）
- assert/assert_eq: std/prelude.mc（Moca言語で実装）

## 7. Rules & Constraints
- テスト関数は `fun _test_xxx()` の形式（引数なし、戻り値なし）
- `assert(bool, string)`: 第1引数が false なら第2引数のメッセージで throw
- `assert_eq(any, any, string)`: 第1引数と第2引数が `==` で等しくなければ throw
- 1つのテストが失敗しても残りのテストは継続実行
- pkg.toml が存在すればその設定に従い、なければ `src/` をスキャン

## 8. Open Questions
- なし

## 9. Acceptance Criteria
1. `moca test` コマンドが実行できる
2. `src/` 配下の `.mc` ファイルから `_test_` プレフィックスの関数が検出される
3. 検出された全テスト関数が順に実行される
4. `assert(true, "msg")` は何も起こらない
5. `assert(false, "msg")` は "msg" を含むエラーを throw する
6. `assert_eq(1, 1, "msg")` は何も起こらない
7. `assert_eq(1, 2, "msg")` は "msg" を含むエラーを throw する
8. テスト成功時は `✓ _test_xxx passed` 形式で表示される
9. テスト失敗時は `✗ _test_xxx failed: {error}` 形式で表示される
10. 全テスト完了後に `X passed, Y failed` の集計が表示される

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo build` が通ることを確認
- assert/assert_eq 実装後、簡単なテストコードで動作確認

### 達成検証
- 実際に `_test_` 関数を含む `.mc` ファイルを作成
- `moca test` を実行し、期待通りの出力が得られることを確認
- 成功ケースと失敗ケースの両方を含むテストで検証

### 漏れ検出
- Acceptance Criteria を1つずつチェック
- エッジケース: テスト関数が0個の場合、複数ファイルにまたがる場合

## 11. Test Plan

### E2E シナリオ 1: 全テスト成功
```
Given: src/math.mc に _test_add() と _test_sub() が定義されている
       両方とも assert(true, "...") で成功する
When:  moca test を実行
Then:  "✓ _test_add passed"
       "✓ _test_sub passed"
       "2 passed, 0 failed" が表示され、終了コード 0
```

### E2E シナリオ 2: 一部テスト失敗
```
Given: src/test.mc に _test_ok() と _test_fail() が定義されている
       _test_ok は成功、_test_fail は assert(false, "expected") で失敗
When:  moca test を実行
Then:  "✓ _test_ok passed"
       "✗ _test_fail failed: expected" が表示
       "1 passed, 1 failed" が表示され、終了コード 1
```

### E2E シナリオ 3: assert_eq の検証
```
Given: src/test.mc に assert_eq(1, 1, "ok") と assert_eq(1, 2, "ng") を使うテストがある
When:  各テストを実行
Then:  前者は成功、後者は "ng" を含むエラーで失敗
```

## 12. Implementation Details

### Compiler API
- `compiler::run_tests(path: &Path, config: &RuntimeConfig) -> TestResults`
- `TestResults` 構造体: passed/failed カウント、各テストの結果リスト

### テスト検出ロジック
1. pkg.toml からエントリーディレクトリを取得（なければ `src/`）
2. ディレクトリ内の `.mc` ファイルを再帰的に収集
3. 各ファイルをパースし、`_test_` プレフィックスの関数を抽出
4. 全テスト関数のリストを返す

### テスト実行ロジック
1. 各テストファイルをコンパイル
2. 各 `_test_` 関数を呼び出すコードを生成・実行
3. エラーがなければ passed、throw されれば failed
4. 結果を集計して返す
