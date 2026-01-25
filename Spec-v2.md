# Spec-v2.md — Developer Experience (DX) Tooling

## 1. Goal

- **開発者体験を実用レベルにする**
- パッケージシステムでプロジェクト管理・依存解決ができる
- LSP で IDE 連携（補完・診断・定義ジャンプ）
- TUI デバッガでステップ実行・変数確認ができる

## 2. Non-Goals

- JIT コンパイル（v3）
- 並行実行（v3）
- 高度な IDE 機能（リファクタリング、型階層表示）
- Web ベースのデバッガ UI
- パッケージレジストリ（中央リポジトリ）の運用

## 3. Target Users

- mica でアプリケーションを開発する開発者
- エディタ（VS Code, Neovim 等）で mica を書きたい開発者
- バグ調査でステップ実行したい開発者

## 4. Core User Flow

### パッケージシステム
1. `mica init myproject` で新規プロジェクト作成
2. `pkg.toml` に依存を記述、または `mica add <url>` で追加
3. `mica run` で依存解決 → ビルド → 実行
4. `pkg.lock` で再現可能なビルド

### LSP
1. エディタが `mica lsp` を起動
2. ファイル編集時にリアルタイム診断
3. 補完・定義ジャンプ・ホバーが動作

### デバッガ
1. `mica debug app.mica` で TUI デバッガ起動
2. ブレークポイント設定、ステップ実行
3. 変数・スタック確認

## 5. Inputs & Outputs

### パッケージシステム
- Input: `pkg.toml`, Git リポジトリ URL
- Output: `pkg.lock`, キャッシュされた依存

### LSP
- Input: LSP プロトコル（stdin/stdout）
- Output: 診断、補完候補、定義位置

### デバッガ
- Input: ソースファイル、ユーザーコマンド
- Output: TUI 表示、実行状態

## 6. Tech Stack

| カテゴリ | 選定 |
|----------|------|
| 言語 | Rust |
| LSP | `tower-lsp` or 自前実装 |
| TUI | `ratatui` |
| TOML パース | `toml` crate |
| Git 操作 | `git2` or CLI 呼び出し |

## 7. Rules & Constraints

### 7.1 パッケージシステム

#### pkg.toml 形式
```toml
[package]
name = "myapp"
version = "0.1.0"
entry = "src/main.mica"

[dependencies]
# Git URL + rev/tag/branch
utils = { git = "https://github.com/user/mica-utils", tag = "v1.0.0" }
json = { git = "https://github.com/user/mica-json", rev = "abc123" }

[dev-dependencies]
test-utils = { git = "https://github.com/user/mica-test" }
```

#### pkg.lock 形式
```toml
[[package]]
name = "utils"
version = "1.0.0"
source = "git+https://github.com/user/mica-utils?tag=v1.0.0"
checksum = "sha256:..."

[[package]]
name = "json"
version = "0.2.0"
source = "git+https://github.com/user/mica-json?rev=abc123"
checksum = "sha256:..."
```

#### モジュール解決
```
import utils.http;     // -> <cache>/utils/src/http.mica
import json;           // -> <cache>/json/src/main.mica (entry)
import ./local_mod;    // -> ./local_mod.mica (相対パス)
```

#### ディレクトリ構成
```
myproject/
├── pkg.toml
├── pkg.lock
└── src/
    ├── main.mica      # entry point
    └── lib/
        └── helper.mica
```

### 7.2 CLI コマンド

```
# プロジェクト操作
mica init [name]        # 新規プロジェクト作成
mica check              # 型チェック・静的解析のみ
mica build              # Bytecode 生成
mica run [file] [args]  # 実行（file 省略時は entry）
mica test               # テスト実行

# 依存管理
mica add <url>[@rev]    # 依存追加
mica remove <name>      # 依存削除
mica update [name]      # 依存更新（全部 or 指定）
mica vendor             # 依存をローカルにコピー

# 開発ツール
mica lsp                # LSP サーバ起動（stdio）
mica debug [file]       # TUI デバッガ起動
mica repl               # REPL 起動
mica fmt [file|dir]     # フォーマット
mica clean              # ビルド成果物削除

# オプション（共通）
--release               # 最適化有効
--verbose               # 詳細出力
```

### 7.3 LSP 機能

#### v2 で実装する機能
| 機能 | LSP Method |
|------|------------|
| 診断（エラー・警告） | `textDocument/publishDiagnostics` |
| 補完 | `textDocument/completion` |
| 定義ジャンプ | `textDocument/definition` |
| ホバー（型・ドキュメント） | `textDocument/hover` |
| 参照検索 | `textDocument/references` |
| フォーマット | `textDocument/formatting` |
| シンボル検索 | `workspace/symbol` |

#### 診断メッセージ例
```
error[E001]: undefined variable 'foo'
  --> src/main.mica:10:5
   |
10 |     print(foo);
   |           ^^^ not found in this scope
```

#### 補完対象
- ローカル変数
- 関数名
- モジュール名
- フィールド名（`.` の後）
- キーワード

### 7.4 デバッガ機能

#### v2 で実装する機能
| 機能 | コマンド |
|------|----------|
| ブレークポイント設定 | `b <file>:<line>` or `b <func>` |
| ブレークポイント削除 | `d <id>` |
| ブレークポイント一覧 | `bl` |
| 続行 | `c` (continue) |
| ステップイン | `s` (step) |
| ステップオーバー | `n` (next) |
| ステップアウト | `finish` |
| スタック表示 | `bt` (backtrace) |
| 変数表示 | `p <expr>` (print) |
| ローカル変数一覧 | `locals` |
| ウォッチ式追加 | `w <expr>` (watch) |
| 終了 | `q` (quit) |

#### TUI レイアウト
```
┌─────────────────────────────────────────────────────────┐
│ src/main.mica                                           │
├─────────────────────────────────────────────────────────┤
│   8 │ fn main() {                                       │
│   9 │     let x = 10;                                   │
│ ▶10 │     let y = compute(x);  // ← 現在位置            │
│  11 │     print(y);                                     │
│  12 │ }                                                 │
├─────────────────────────────────────────────────────────┤
│ Locals              │ Call Stack                        │
│ x = 10              │ #0 main (main.mica:10)            │
│ y = <uninitialized> │                                   │
├─────────────────────────────────────────────────────────┤
│ (debug) _                                               │
└─────────────────────────────────────────────────────────┘
```

### 7.5 ソースマップ・デバッグ情報

#### PC → ソース位置テーブル
```rust
struct LineTable {
    entries: Vec<LineEntry>,
}

struct LineEntry {
    pc: u32,           // Bytecode offset
    file_id: u16,      // ファイル識別子
    line: u32,         // 行番号（1-based）
    column: u16,       // 列番号（1-based）
}
```

#### ローカル変数メタデータ
```rust
struct LocalVarInfo {
    name: String,
    slot: u16,         // locals[] のインデックス
    scope_start: u32,  // 有効範囲開始 PC
    scope_end: u32,    // 有効範囲終了 PC
    type_hint: String, // 型情報（推論結果）
}
```

## 8. Open Questions

- LSP の増分解析（ファイル変更時の再パース範囲）
- デバッガの式評価（副作用のある式の扱い）
- パッケージバージョンの競合解決戦略

## 9. Acceptance Criteria

1. `mica init` でプロジェクト雛形（pkg.toml, src/main.mica）が生成される
2. `mica add <url>` で pkg.toml に依存が追加される
3. `mica run` で依存が自動解決・ダウンロードされる
4. pkg.lock により同じ依存バージョンが再現される
5. `mica lsp` が起動し、VS Code 等から接続できる
6. 構文エラー・型エラーがリアルタイムで表示される
7. 定義ジャンプ（Go to Definition）が動作する
8. 補完（Completion）が動作する
9. `mica debug` で TUI が起動し、ブレークポイントで停止できる
10. デバッガでローカル変数の値が確認できる

## 10. Test Plan

### E2E Test 1: パッケージ管理

**Given:** 空のディレクトリ

**When:** 以下を実行
```bash
mica init myapp
cd myapp
echo 'print("hello");' > src/main.mica
mica run
```

**Then:**
- `pkg.toml` が生成されている
- `src/main.mica` が存在する
- stdout に `hello` が出力される

### E2E Test 2: LSP 診断

**Given:** LSP サーバが起動している

**When:** 以下の内容のファイルを開く
```
fn main() {
    print(undefined_var);
}
```

**Then:** `undefined_var` に対してエラー診断が返る

### E2E Test 3: デバッガ

**Given:** 以下の内容の `debug_test.mica`
```
fn add(a, b) {
    return a + b;
}

let result = add(3, 4);
print(result);
```

**When:** `mica debug debug_test.mica` を起動し、以下を実行
```
b add
c
p a
p b
c
```

**Then:**
- `add` 関数でブレークする
- `a = 3`, `b = 4` が表示される
- 続行後、`7` が出力される
