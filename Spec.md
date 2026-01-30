# Spec.md

## 1. Goal
- VMからTCPクライアントとして外部サーバーに接続し、HTTP GETリクエストを送受信できるようにする

## 2. Non-Goals
- サーバー機能（listen / accept）は今回やらない
- UDP対応はやらない
- ノンブロッキングI/O、async対応はやらない
- TLS/SSL対応はやらない

## 3. Target Users
- mocaスクリプトから外部HTTPサーバーにアクセスしたい開発者

## 4. Core User Flow
1. `socket(AF_INET, SOCK_STREAM)` でソケットfdを取得
2. `connect(fd, host, port)` でサーバーに接続
3. `write(fd, request, len)` でHTTPリクエストを送信
4. `read(fd, len)` でレスポンスを受信
5. `close(fd)` でソケットを閉じる

## 5. Inputs & Outputs

### Inputs
- `syscall_socket(domain, type)` → domain: AF_INET(2), type: SOCK_STREAM(1)
- `syscall_connect(fd, host, port)` → host: 文字列, port: 整数

### Outputs
- `socket` → ソケットfd (正の整数) またはエラー (負の整数)
- `connect` → 0 (成功) またはエラー (負の整数)
- 既存の `write` / `read` / `close` をソケットfdでも使用

## 6. Tech Stack
- 言語: Rust
- ソケット: `std::net::TcpStream`
- テスト: cargo test

## 7. Rules & Constraints

### ソケット管理
- ソケットfdも既存の `file_descriptors: HashMap<i64, File>` と同様に管理
- 新たに `socket_descriptors: HashMap<i64, TcpStream>` を追加
- fd番号は `next_fd` を共有して衝突を防ぐ

### 定数 (Linux互換)
- `AF_INET = 2`
- `SOCK_STREAM = 1`

### エラーコード
- `EBADF = -1`: 無効なfd
- `ECONNREFUSED = -4`: 接続拒否
- `ETIMEDOUT = -5`: 接続タイムアウト
- `EAFNOSUPPORT = -6`: 非対応のアドレスファミリ
- `ESOCKTNOSUPPORT = -7`: 非対応のソケットタイプ

### write/read/closeの拡張
- `write`: ソケットfdの場合は `TcpStream::write` を使用
- `read`: ソケットfdの場合は `TcpStream::read` を使用
- `close`: ソケットfdの場合は `socket_descriptors` から削除

## 8. Open Questions
- なし

## 9. Acceptance Criteria

1. `socket(AF_INET, SOCK_STREAM)` が正のfdを返す
2. `socket(999, 1)` が `EAFNOSUPPORT` を返す
3. `socket(2, 999)` が `ESOCKTNOSUPPORT` を返す
4. `connect(fd, "example.com", 80)` が0を返す（成功時）
5. `connect(invalid_fd, ...)` が `EBADF` を返す
6. `write(socket_fd, data, len)` がソケットに書き込める
7. `read(socket_fd, len)` がソケットから読み込める
8. `close(socket_fd)` がソケットを正しく閉じる
9. HTTP GET リクエストを送信してレスポンスを受信できる

## 10. Verification Strategy

### 進捗検証
- 各syscall実装後にユニットテストを実行

### 達成検証
- `http://example.com` へのGET リクエストが成功し、HTMLレスポンスが返る

### 漏れ検出
- cargo test で全テストパス
- cargo clippy でwarningなし

## 11. Test Plan

### E2E シナリオ 1: 基本的なHTTP GET
```
Given: example.com:80 が到達可能
When: socket → connect → write("GET / HTTP/1.0\r\nHost: example.com\r\n\r\n") → read → close
Then: レスポンスに "<!doctype html>" または "HTTP/1" が含まれる
```

### E2E シナリオ 2: エラーハンドリング
```
Given: 無効なfd
When: connect(999, "example.com", 80)
Then: EBADF (-1) が返る
```

### E2E シナリオ 3: ソケットタイプエラー
```
Given: なし
When: socket(999, 1)
Then: EAFNOSUPPORT (-6) が返る
```
