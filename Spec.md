# Spec.md

## 1. Goal
- MocaスクリプトでHTTPサーバーを起動し、GETリクエストに対してレスポンスを返せるようにする

## 2. Non-Goals
- HTTPS/TLS対応
- HTTP/2対応
- Keep-Alive対応
- WebSocket対応
- HTTPリクエストの詳細なパース（メソッド、パス、ヘッダーの解析）
- 複数クライアントの同時接続対応（マルチスレッド）

## 3. Target Users
- MocaスクリプトでシンプルなHTTPサーバーを立てたい開発者
- ネットワークプログラミングの学習目的でMocaを使うユーザー

## 4. Core User Flow
1. ユーザーがMocaスクリプトでHTTPサーバーのコードを書く
2. `moca run server.mc` でサーバーを起動
3. サーバーが指定ポートでリッスン開始
4. クライアント（curl等）からHTTPリクエストを送信
5. サーバーがリクエストを受信し、HTTPレスポンスを返却
6. クライアントがレスポンスを受信

## 5. Inputs & Outputs
### Inputs
- ポート番号（Mocaスクリプト内で指定）
- クライアントからのHTTPリクエスト（生のTCPデータ）

### Outputs
- HTTPレスポンス（ステータスライン + ヘッダー + ボディ）

## 6. Tech Stack
- 言語: Rust（VM実装）、Moca（サンプルスクリプト）
- 既存のsyscallインフラを拡張
- テスト: Rust integration tests + 手動curlテスト

## 7. Rules & Constraints
- 既存の `socket()`, `connect()` と同じパターンでAPIを設計する
- syscallは `bind`, `listen`, `accept` の3つを追加
- エラーコードは既存の定数（EBADF等）を再利用
- IPv4 (AF_INET) + TCP (SOCK_STREAM) のみサポート
- シングルスレッド、1接続ずつ順次処理

## 8. Open Questions
- なし

## 9. Acceptance Criteria
1. `bind(fd, host, port)` 関数がpreludeに存在し、ソケットをアドレスにバインドできる
2. `listen(fd, backlog)` 関数がpreludeに存在し、ソケットを接続待ち状態にできる
3. `accept(fd)` 関数がpreludeに存在し、クライアント接続を受け入れて新しいfdを返す
4. 3つのsyscall（SYSCALL_BIND, SYSCALL_LISTEN, SYSCALL_ACCEPT）がvm.rsに実装されている
5. サンプルHTTPサーバー（examples/http_server.mc）が存在する
6. `moca run examples/http_server.mc` でサーバーが起動する
7. `curl http://localhost:<port>/` でHTTPレスポンスが返る
8. `cargo test` が全てパスする
9. `cargo clippy` が警告なしでパスする

## 10. Verification Strategy
### 進捗検証
- 各syscall実装後に `cargo test` でユニットテストが通ることを確認
- prelude関数追加後に型チェック（`cargo check`）が通ることを確認

### 達成検証
- サンプルサーバーを起動し、curlでリクエストを送信してレスポンスが返ることを確認
- 全Acceptance Criteriaをチェックリストで確認

### 漏れ検出
- 既存のhttp_get.mcが引き続き動作することを確認（リグレッションなし）
- `cargo test` + `cargo clippy` で品質担保

## 11. Test Plan

### E2E シナリオ 1: 基本的なHTTPサーバー起動とレスポンス
- **Given**: examples/http_server.mc が存在する
- **When**: `moca run examples/http_server.mc` を実行し、別ターミナルから `curl http://localhost:8080/` を実行
- **Then**: HTTPレスポンス（200 OK + ボディ）が返却される

### E2E シナリオ 2: サーバーの正常終了
- **Given**: HTTPサーバーが起動している
- **When**: Ctrl+C でサーバープロセスを終了
- **Then**: プロセスが正常終了し、ポートが解放される

### E2E シナリオ 3: 既存機能のリグレッションなし
- **Given**: examples/http_get.mc が存在する
- **When**: `moca run examples/http_get.mc -- httpbin.org 80 /get` を実行
- **Then**: HTTPレスポンスが正常に取得できる
