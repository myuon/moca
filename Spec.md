# Spec.md

## 1. Goal
- VM に syscall 機構を追加し、`write(1, str, len)` で stdout に出力できるようにする
- これにより `print` を stdlib で自前実装可能にする

## 2. Non-Goals
- ファイル操作（open/close/read）は対象外
- fd=1, 2 以外のファイルディスクリプタは対象外
- 高度なエラーハンドリング（errno 等）は対象外
- JIT での syscall 対応は対象外（インタプリタのみ）

## 3. Target Users
- moca 言語の開発者（自分自身）
- moca で低レベル I/O を扱いたいユーザー

## 4. Core User Flow
1. ユーザーが moca コードで `syscall_write(1, "hello", 5)` を呼ぶ
2. コンパイラが `Op::Syscall` 命令を生成
3. VM が syscall を実行し、stdout に "hello" を出力
4. 戻り値（書き込んだバイト数 or -1）がスタックに積まれる

## 5. Inputs & Outputs
### Inputs
- fd: i64（1 = stdout, 2 = stderr）
- buf: 文字列型
- count: i64（書き込むバイト数）

### Outputs
- 成功時: 書き込んだバイト数（i64）
- 失敗時: -1（i64）

## 6. Tech Stack
- 言語: Rust
- 変更対象ファイル:
  - `src/vm/ops.rs` - Syscall 命令追加
  - `src/vm/vm.rs` - syscall ハンドラ実装
  - `src/compiler/codegen.rs` - syscall_write ビルトイン追加
  - `src/vm/bytecode.rs` - シリアライズ対応
  - `src/compiler/builtin.rs`（存在すれば）- ビルトイン定義
- テスト: 既存 snapshot テスト + 新規 .mc ファイル

## 7. Rules & Constraints
- POSIX 互換の引数順序: `write(fd, buf, count)`
- fd は 1（stdout）と 2（stderr）のみ許可、それ以外は -1 を返す
- count が文字列長を超える場合は文字列長までで切り詰める
- 既存の `Op::Print` は `Op::PrintDebug` に改名してデバッグ用に残す
- 将来的に他の syscall（read, exit 等）を追加できる汎用設計にする

## 8. Open Questions
- なし

## 9. Acceptance Criteria
1. `syscall_write(1, "hello", 5)` で stdout に "hello" が出力される
2. `syscall_write(2, "error", 5)` で stderr に "error" が出力される
3. 戻り値として書き込んだバイト数が返る
4. 無効な fd（0, 3 など）では -1 が返り、何も出力されない
5. `print_debug(x)` が従来の print と同じ動作をする
6. 既存のテストが全て通る
7. 新規テストファイルで write syscall の動作が確認できる

## 10. Verification Strategy
- **進捗検証**: 各ステップで `cargo build` が通ること、既存テストが壊れないこと
- **達成検証**: `syscall_write(1, "hello", 5)` を含む .mc ファイルを実行し、"hello" が出力されること
- **漏れ検出**: Acceptance Criteria を1つずつ手動確認

## 11. Test Plan

### E2E シナリオ 1: 基本的な write syscall
- **Given**: `syscall_write(1, "hello", 5)` を含む .mc ファイル
- **When**: moca で実行する
- **Then**: stdout に "hello" が出力され、戻り値 5 が返る

### E2E シナリオ 2: stderr への出力
- **Given**: `syscall_write(2, "err", 3)` を含む .mc ファイル
- **When**: moca で実行する
- **Then**: stderr に "err" が出力される

### E2E シナリオ 3: 無効な fd
- **Given**: `syscall_write(99, "x", 1)` を含む .mc ファイル
- **When**: moca で実行する
- **Then**: 何も出力されず、戻り値 -1 が返る
