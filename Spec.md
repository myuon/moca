# Spec.md

## 1. Goal
- VMのwrite syscallが任意のファイルディスクリプタをサポートし、open/write/closeでファイル書き込みができるようになる

## 2. Non-Goals
- read syscall（後回し）
- stdin (fd=0) のサポート（後回し）
- セキュリティ制約（サンドボックス、パス制限など）
- 高度なopenフラグ（O_APPEND, O_RDONLY, O_RDWR など）

## 3. Target Users
- mocaプログラムからファイル出力を行いたい開発者

## 4. Core User Flow
1. `syscall_open(path, flags)` でファイルを開き、fdを取得
2. `syscall_write(fd, buf, count)` でfdにデータを書き込む
3. `syscall_close(fd)` でfdを閉じる
4. ファイルシステム上にデータが永続化される

## 5. Inputs & Outputs

### Inputs
- `syscall_open(path: string, flags: int) -> int`
  - path: ファイルパス（相対・絶対両方）
  - flags: オープンフラグ（後述）
- `syscall_write(fd: int, buf: string, count: int) -> int`
  - fd: ファイルディスクリプタ
  - buf: 書き込むデータ
  - count: 書き込むバイト数
- `syscall_close(fd: int) -> int`
  - fd: 閉じるファイルディスクリプタ

### Outputs
- open: 成功時は正のfd、失敗時は負のエラーコード
- write: 成功時は書き込んだバイト数、失敗時は負のエラーコード
- close: 成功時は0、失敗時は負のエラーコード

### エラーコード
| 値 | 名前 | 意味 |
|----|------|------|
| -1 | EBADF | 無効なファイルディスクリプタ |
| -2 | ENOENT | ファイルが存在しない（O_CREATなしの場合） |
| -3 | EACCES | アクセス権限エラー |

### オープンフラグ
| 値 | 名前 | 意味 |
|----|------|------|
| 1 | O_WRONLY | 書き込み専用 |
| 64 | O_CREAT | ファイルがなければ作成 |
| 512 | O_TRUNC | 既存内容を切り詰め |

(仮) フラグ値はLinuxの値に準拠。組み合わせはビットOR。

## 6. Tech Stack
- 言語: Rust
- 既存VM (`src/vm/vm.rs`) を拡張
- fdテーブル: `HashMap<i64, std::fs::File>`
- テスト: 既存のスナップショットテスト + 新規ユニットテスト

## 7. Rules & Constraints
- fd=1 (stdout) と fd=2 (stderr) は現行の挙動を維持する
- fd=0, 1, 2 は予約済みとし、openで返さない（fd >= 3 から割り当て）
- closeされていないfdはVM終了時に自動close
- fdテーブルはVM内部で管理（外部からの注入は不要）
- 書き込み時、countがbuf長を超える場合はbuf長に切り詰め

## 8. Open Questions
- (なし)

## 9. Acceptance Criteria
1. `syscall_open("test.txt", O_WRONLY | O_CREAT | O_TRUNC)` が正のfdを返す
2. `syscall_write(fd, "hello", 5)` が5を返す
3. `syscall_close(fd)` が0を返す
4. close後、ファイルシステム上に "hello" が書き込まれている
5. fd=1への書き込みは従来通りstdoutに出力される
6. fd=2への書き込みは従来通りstderrに出力される
7. 無効なfdへの書き込みは-1 (EBADF) を返す
8. 存在しないパスへのopen（O_CREATなし）は-2 (ENOENT) を返す
9. 既存のsyscall_writeテスト (`tests/snapshots/basic/syscall_write.mc`) が引き続きパスする

## 10. Verification Strategy

### 進捗検証
- 各syscall実装後、個別のユニットテストで動作確認
- `cargo test` が全てパスすることを確認

### 達成検証
- E2Eテスト: tmpファイルを作成し、open→write→close後にファイル内容を検証
- Acceptance Criteria のチェックリストを全て確認

### 漏れ検出
- 既存テスト (`cargo test`) が全てパス
- `cargo clippy` で警告なし

## 11. Test Plan

### E2E シナリオ 1: 基本的なファイル書き込み
- **Given**: 空の一時ディレクトリ
- **When**: open("test.txt", O_WRONLY|O_CREAT|O_TRUNC) → write(fd, "hello", 5) → close(fd)
- **Then**: test.txt の内容が "hello" である

### E2E シナリオ 2: 既存ファイルの上書き
- **Given**: "old content" を含む既存ファイル
- **When**: open(同ファイル, O_WRONLY|O_CREAT|O_TRUNC) → write(fd, "new", 3) → close(fd)
- **Then**: ファイル内容が "new" である（"old content" は消えている）

### E2E シナリオ 3: stdout/stderr との共存
- **Given**: VMが起動している
- **When**: write(1, "stdout", 6) と write(2, "stderr", 6) を実行
- **Then**: それぞれstdout/stderrに出力される（従来通り）
