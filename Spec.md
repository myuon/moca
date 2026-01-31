# Spec.md

## 1. Goal
- mocaスクリプト内からコマンドライン引数を取得できるようにする

## 2. Non-Goals
- 環境変数の取得
- オプションパーサー（`--flag=value` 等の解析ライブラリ）
- 標準入力からの読み取り（既存の `read` 関数で対応済み）

## 3. Target Users
- mocaでCLIツールを作成したい開発者

## 4. Core User Flow
1. ユーザーが `moca run script.mc arg1 arg2 arg3` を実行
2. スクリプト内で `argc()` や `argv(index)` または `args()` を呼び出す
3. コマンドライン引数を文字列として取得できる

## 5. Inputs & Outputs
- **入力**: `moca run` コマンドに渡されたスクリプトパス以降の引数
- **出力**:
  - `argc()` → 引数の個数（int）
  - `argv(index)` → 指定インデックスの引数（string）
  - `args()` → 全引数の配列（[string]）

## 6. Tech Stack
- 言語: Rust（VM/コンパイラ側）、moca（標準ライブラリ側）
- 既存のビルトイン関数の仕組みを利用

## 7. Rules & Constraints
- `argv(0)` はスクリプトファイルのパスを返す（一般的な慣習に従う）
- `argv(1)` 以降がユーザー指定の引数
- 引数はすべて `string` 型として取得（数値変換は `parse_int` 等をユーザーが使用）
- 範囲外のインデックスを指定した場合の挙動: 空文字列を返す（仮）
- `moca run script.mc arg1 arg2` の形式で引数を渡す（`--` セパレータ不要）

## 8. Open Questions
- なし

## 9. Acceptance Criteria（最大10個）
1. `moca run script.mc` 実行時、スクリプト内で `argc()` を呼ぶと引数の個数が取得できる
2. `argv(0)` でスクリプトファイルのパスが取得できる
3. `argv(1)`, `argv(2)` 等でユーザー指定の引数が取得できる
4. `args()` で全引数を配列として取得できる
5. 引数がない場合、`argc()` は 1 を返す（スクリプトパスのみ）
6. 範囲外のインデックスを `argv()` に渡した場合、空文字列が返る
7. `cargo test` が通る
8. `cargo clippy` が通る

## 10. Verification Strategy
- **進捗検証**: 各ビルトイン関数実装後にサンプルスクリプトで動作確認
- **達成検証**: 以下のテストスクリプトが期待通り動作する
  ```bash
  moca run examples/cli_args.mc hello world 123
  # 期待出力:
  # argc: 4
  # argv(0): examples/cli_args.mc
  # argv(1): hello
  # argv(2): world
  # argv(3): 123
  ```
- **漏れ検出**: Acceptance Criteria のチェックリスト確認

## 11. Test Plan

### E2E シナリオ 1: 引数なし実行
- **Given**: `examples/cli_args.mc` が存在する
- **When**: `moca run examples/cli_args.mc` を実行
- **Then**: `argc()` が 1 を返し、`argv(0)` がスクリプトパスを返す

### E2E シナリオ 2: 複数引数あり実行
- **Given**: `examples/cli_args.mc` が存在する
- **When**: `moca run examples/cli_args.mc foo bar baz` を実行
- **Then**: `argc()` が 4 を返し、`argv(1)` が "foo"、`argv(2)` が "bar"、`argv(3)` が "baz" を返す

### E2E シナリオ 3: args() 関数
- **Given**: `examples/cli_args.mc` が存在する
- **When**: `moca run examples/cli_args.mc a b` を実行し、`args()` を呼ぶ
- **Then**: 長さ3の配列が返り、要素は [スクリプトパス, "a", "b"]
