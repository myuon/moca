# Spec.md — 時刻取得syscallの追加

## 1. Goal
- mocaプログラムから現在時刻を取得できるsyscallを追加し、秒単位・ナノ秒単位での取得と簡易フォーマット表示を可能にする

## 2. Non-Goals
- タイムゾーン対応（UTCのみ）
- 日付パース（文字列→時刻への変換）
- sleep / timer 系の機能
- モノトニッククロック
- 高度な日付フォーマット（strftime等）

## 3. Target Users
- mocaプログラムの開発者が、プログラム内で現在時刻の取得・表示を行いたい場合に使用

## 4. Core User Flow
1. mocaコード内で `time()` を呼び出し、Unix epoch からの秒数（int）を取得する
2. mocaコード内で `time_nanos()` を呼び出し、Unix epoch からのナノ秒数（int）を取得する
3. `time_format(seconds)` に秒数を渡し、`"YYYY-MM-DD HH:MM:SS"` 形式のUTC文字列を取得する
4. 取得した値を `print` 等で出力する

## 5. Inputs & Outputs

### Syscall 10: `time`
- 入力: なし（引数0個）
- 出力: `int` — Unix epoch からの秒数

### Syscall 11: `time_nanos`
- 入力: なし（引数0個）
- 出力: `int` — Unix epoch からのナノ秒数（i64範囲内、2262年頃まで）

### Syscall 12: `time_format`
- 入力: `int` — Unix epoch からの秒数
- 出力: `string` — `"YYYY-MM-DD HH:MM:SS"` 形式のUTC文字列

## 6. Tech Stack
- 言語: Rust（VM実装）、moca（標準ライブラリラッパー）
- テスト: `cargo test`（snapshot_tests.rs 内にカスタムテスト関数を追加）
- 時刻取得: `std::time::SystemTime` / `std::time::UNIX_EPOCH`（外部クレート不要）
- UTCフォーマット: 手動計算（chrono等の外部クレート不使用）

## 7. Rules & Constraints
- 既存のsyscall体系（1-9）に続く番号（10, 11, 12）を使用する
- `std::time::SystemTime::now()` を使用し、外部クレートに依存しない
- UTC フォーマットの年月日時分秒計算は Rust 側で手動実装する（うるう年考慮）
- `prelude.mc` に追加するラッパー関数は既存パターン（`__syscall` 呼び出し）に従う
- スナップショットテストは stdout 完全一致ではなく、Rust側テスト関数で時刻の近似比較を行う

## 8. Open Questions
なし

## 9. Acceptance Criteria

1. `__syscall(10)` を呼び出すと、現在のUnix epoch秒（int）が返る
2. `__syscall(11)` を呼び出すと、現在のUnix epochナノ秒（int）が返る
3. `__syscall(12, seconds)` を呼び出すと、`"YYYY-MM-DD HH:MM:SS"` 形式の文字列が返る
4. `prelude.mc` に `time() -> int`、`time_nanos() -> int`、`time_format(secs: int) -> string` のラッパー関数が存在する
5. `prelude.mc` のsyscall番号コメントに syscall 10, 11, 12 が追記されている
6. スナップショットテストで、mocaの `time()` とRustの `SystemTime::now()` の差が ±2秒以内である
7. スナップショットテストで、mocaの `time_nanos()` が妥当な範囲（Rustのナノ秒と大きく乖離しない）である
8. スナップショットテストで、mocaの `time_format()` とRustで同じ秒数をフォーマットした結果が一致する
9. `cargo fmt && cargo check && cargo test && cargo clippy` が全てパスする

## 10. Verification Strategy

- **進捗検証**: 各syscallの実装後に個別に `cargo test` を実行し、コンパイルエラー・テスト失敗がないことを確認
- **達成検証**: 全Acceptance Criteriaをチェックリストで確認。特にスナップショットテストが時刻の近似比較で合格すること
- **漏れ検出**: `cargo clippy` による静的解析、`moca lint` による `.mc` ファイルのチェック

## 11. Test Plan

### Test 1: 時刻取得の正確性
- **Given**: mocaプログラムが `time()` と `time_nanos()` を呼び出して出力する
- **When**: テストがmocaプログラムを実行し、出力をキャプチャする
- **Then**: Rust側で取得した `SystemTime::now()` との差が秒単位で±2秒以内、ナノ秒も妥当な範囲内

### Test 2: フォーマット出力の一致
- **Given**: mocaプログラムが `time()` で秒数を取得し `time_format()` でフォーマットする
- **When**: テストがmocaプログラムの出力をキャプチャする
- **Then**: mocaの出力した秒数をRust側で同じロジックでフォーマットした結果と一致する

### Test 3: time_format の固定値テスト
- **Given**: mocaプログラムが `time_format(0)` を呼び出す（Unix epoch = 1970-01-01 00:00:00）
- **When**: テストがmocaプログラムの出力をキャプチャする
- **Then**: 出力が `"1970-01-01 00:00:00"` と一致する
