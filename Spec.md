# Spec.md

## 1. Goal
- VMにread syscallを追加し、ファイルからデータを読み込めるようになる

## 2. Non-Goals
- stdin (fd=0) からの読み込み
- バイナリデータの読み込み（文字列として扱う）
- 非同期I/O

## 3. Target Users
- mocaプログラムからファイル入力を行いたい開発者

## 4. Core User Flow
1. `syscall_open(path, O_RDONLY)` でファイルを読み込みモードで開く
2. `syscall_read(fd, count)` でfdからデータを読み込む
3. `syscall_close(fd)` でfdを閉じる

## 5. Inputs & Outputs

### Inputs
- `syscall_read(fd: int, count: int)`
  - fd: ファイルディスクリプタ (>=3)
  - count: 読み込む最大バイト数

### Outputs
- 成功時: 読み込んだ文字列 (heap上に確保)
- EOF時: 空文字列
- 失敗時: 負のエラーコード (EBADF = -1)

### 追加するOpenフラグ
| 値 | 名前 | 意味 |
|----|------|------|
| 0 | O_RDONLY | 読み込み専用 |

## 6. Tech Stack
- 言語: Rust
- 既存VM (`src/vm/vm.rs`) を拡張
- テスト: 既存のユニットテスト

## 7. Rules & Constraints
- fd=0, 1, 2 は予約済み（read不可）
- countが負の場合はEBADFを返す
- 読み込んだデータはheap上に文字列として確保
- 読み込み中のエラーはEBADFを返す

## 8. Open Questions
- (なし)

## 9. Acceptance Criteria
1. `syscall_open(path, O_RDONLY)` でファイルを読み込みモードで開ける
2. `syscall_read(fd, 100)` が文字列を返す
3. 読み込んだ文字列の内容がファイル内容と一致する
4. EOF時に空文字列を返す
5. 無効なfdへのreadはEBADF (-1) を返す
6. 既存のwrite/close syscallが引き続き動作する

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo test` でテスト通過を確認

### 達成検証
- Rustでtempファイルに書き込み → read syscallで読み出し → 内容一致を確認するテスト

### 漏れ検出
- `cargo clippy` で警告なし
- 既存テストが全てパス

## 11. Test Plan

### E2E シナリオ 1: 基本的なファイル読み込み
- **Given**: "hello world" を含むtempファイル
- **When**: open(O_RDONLY) → read(fd, 100) → close(fd)
- **Then**: 読み込んだ文字列が "hello world" と一致

### E2E シナリオ 2: 部分読み込み
- **Given**: "hello world" を含むtempファイル
- **When**: open(O_RDONLY) → read(fd, 5) → close(fd)
- **Then**: 読み込んだ文字列が "hello" と一致

### E2E シナリオ 3: 無効なfd
- **Given**: VMが起動している
- **When**: read(99, 10)
- **Then**: EBADF (-1) が返る
