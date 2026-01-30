# Spec.md

## 1. Goal
- syscall の実装ロジックを VM (`vm.rs`) から runtime モジュール (`src/runtime/`) に移動し、VM を薄く保つ

## 2. Non-Goals
- syscall の機能追加（新しい syscall の追加）
- stdout/stderr のモック機構の変更（VM側に残す）
- パフォーマンス最適化

## 3. Target Users
- moca 言語の開発者
- VM のコードをシンプルに保ちたい

## 4. Core User Flow
1. moca コードが `Op::Syscall(num, argc)` を実行
2. VM がスタックから引数を取得
3. VM が `runtime::syscall::handle(num, args, heap)` を呼び出し
4. runtime モジュールが syscall を実行し、結果を返す
5. VM が結果をスタックに積む

## 5. Inputs & Outputs

### Inputs
- `runtime::syscall::handle(syscall_num: usize, args: Vec<Value>, heap: &mut Heap)`
- stdout/stderr は VM から渡す（write syscall 用）

### Outputs
- `Result<Value, String>` を VM に返す

## 6. Tech Stack
- 言語: Rust
- テスト: cargo test + snapshot tests

## 7. Rules & Constraints

### アーキテクチャ
```
src/
  runtime/
    mod.rs
    syscall.rs      # syscall ロジック + fd state (thread_local)
  vm/
    vm.rs           # handle_syscall は runtime に委譲するだけ
```

### fd 管理
- `file_descriptors: HashMap<i64, File>` → `runtime::syscall` に移動
- `socket_descriptors: HashMap<i64, TcpStream>` → `runtime::syscall` に移動
- `pending_sockets: HashSet<i64>` → `runtime::syscall` に移動
- `next_fd: i64` → `runtime::syscall` に移動
- `thread_local!` マクロで thread-safe に管理

### stdout/stderr
- VM が持つ `output: Box<dyn Write>` は維持
- write syscall (fd=1,2) の場合は VM から writer を渡す

### 既存テスト
- vm.rs 内の syscall ユニットテストは削除
- 代わりに `tests/snapshots/syscall/` にスナップショットテストを追加

## 8. Open Questions
- なし

## 9. Acceptance Criteria

1. `cargo test` が全てパスする
2. `src/runtime/syscall.rs` に syscall ロジックが移動している
3. `vm.rs` の `handle_syscall` が 50 行以下になっている
4. `file_descriptors`, `socket_descriptors`, `pending_sockets`, `next_fd` が VM struct から削除されている
5. `tests/snapshots/syscall/` にスナップショットテストが存在する
6. 既存の機能（file I/O, socket I/O）が動作する

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo test` でリグレッションチェック

### 達成検証
- 全 Acceptance Criteria をチェック
- E2E テスト（HTTP GET）がパス

### 漏れ検出
- `cargo clippy` で警告なし
- vm.rs 内に syscall ロジックが残っていないことを grep で確認

## 11. Test Plan

### E2E シナリオ 1: ファイル I/O
```
Given: tests/snapshots/syscall/file_io.mc が存在
When: moca file_io.mc を実行
Then: ファイルの書き込み・読み込みが成功し、期待する出力が得られる
```

### E2E シナリオ 2: ソケット I/O（ローカルサーバー）
```
Given: tests/snapshots/syscall/socket_io.mc が存在
When: moca socket_io.mc を実行
Then: ローカルサーバーへの接続・送受信が成功する
```

### E2E シナリオ 3: エラーハンドリング
```
Given: tests/snapshots/syscall/error_handling.mc が存在
When: 無効な fd で read/write を実行
Then: 適切なエラーコード（EBADF）が返る
```
