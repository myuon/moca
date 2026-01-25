# Claude Code カスタマイズ リファレンス

> **出典について**: 各セクションに公式ドキュメントのURLを記載しています。
> `/cc-custom-upgrade` で最新情報に更新できます。

---

## 1. Memory (CLAUDE.md)

> **出典**: https://code.claude.com/docs/en/memory
> **最終更新**: 2025-01

### 概要
セッション開始時に自動読み込みされる永続的な指示。プロジェクトの文脈やルールをClaudeに伝える。

### いつ使う？
- プロジェクト固有のコマンド（ビルド、テスト等）を教えたい
- コーディング規約を常に適用したい
- 「これは触るな」「これを使え」等の制約を設けたい
- チームで共有する情報を記述したい

### ファイル構造

```
project/
├── CLAUDE.md                    # プロジェクトメモリ（チーム共有）
├── CLAUDE.local.md              # ローカル専用（自動.gitignore）
└── .claude/
    ├── CLAUDE.md                # 代替配置場所
    └── rules/                   # モジュール化ルール
        ├── code-style.md
        ├── testing.md
        └── api-design.md

~/.claude/
└── CLAUDE.md                    # ユーザーメモリ（全プロジェクト共通）
```

### 書き方のコツ

```markdown
# プロジェクト: MyApp

## よく使うコマンド
- ビルド: `pnpm build`
- テスト: `pnpm test`
- 単一テスト: `pnpm test -- --grep "テスト名"`

## コーディング規約
- TypeScript strict mode
- 関数はアロー関数で統一
- エラーはResult型でハンドリング

## 注意点
- `/src/legacy/` は非推奨。新コードは `/src/v2/` に
- 環境変数は `.env.example` を参照

## 外部ドキュメント
@docs/api-spec.md
@docs/architecture.md
```

### パス固有ルール (.claude/rules/)

特定ファイルパスにのみ適用するルール：

```markdown
---
paths:
  - "src/api/**/*.ts"
  - "src/services/**/*.ts"
---

# API/サービス層のルール

- すべての公開メソッドにJSDocを書く
- エラーは AppError クラスを使用
- ログは logger.info/error を使用（console.log禁止）
```

---

## 2. Skills

> **出典**: https://code.claude.com/docs/en/skills
> **最終更新**: 2025-01

### 概要
再利用可能なワークフローや知識。Claudeが自動選択するか、`/skill-name` で明示呼び出し。

### いつ使う？
- 繰り返し行うワークフローを定型化したい
- 特定タスクに専門知識を適用したい
- コードレビュー、デプロイ等の手順を標準化したい

### ファイル構造

```
.claude/skills/
└── <skill-name>/
    ├── SKILL.md        # 必須：スキル定義
    ├── reference.md    # 任意：詳細ドキュメント
    └── examples.md     # 任意：使用例
```

### SKILL.md の書き方

```markdown
---
name: pr-review
description: PRのコードレビューを実施。diff確認、品質チェック、改善提案を行う。
allowed-tools: Read, Grep, Glob, Bash
---

# PRレビュー手順

1. `git diff main...HEAD` で変更を確認
2. 変更ファイルを読み込み
3. 以下の観点でレビュー：
   - コード品質と可読性
   - エラーハンドリング
   - セキュリティリスク
   - テストカバレッジ
4. 優先度別にフィードバック：
   - 🔴 Critical: 修正必須
   - 🟡 Warning: 修正推奨
   - 🟢 Suggestion: 検討事項
```

### フロントマターオプション

| フィールド | 値 | 説明 |
|-----------|-----|------|
| `name` | 文字列 | スキル名（小文字・ハイフン） |
| `description` | 文字列 | いつ使うか（Claude判断用） |
| `allowed-tools` | カンマ区切り | 使用可能ツール |
| `disable-model-invocation` | true/false | trueでユーザー呼び出しのみ |
| `user-invocable` | true/false | falseでメニュー非表示 |
| `context` | fork | サブエージェントとして実行 |
| `model` | sonnet/opus/haiku | 使用モデル指定 |

### 呼び出し制御

```markdown
# ユーザーのみ呼び出し可能（デプロイ等の副作用あり操作）
---
disable-model-invocation: true
---

# Claudeのみ呼び出し可能（バックグラウンド知識）
---
user-invocable: false
---
```

---

## 3. Subagents

> **出典**: https://code.claude.com/docs/en/sub-agents
> **最終更新**: 2025-01

### 概要
独立したコンテキストで動作する特化エージェント。メイン会話を汚さず調査・実行できる。

### いつ使う？
- 大量のファイル探索が必要（メインコンテキストを節約）
- 特定ツールに制限したい（読み取り専用等）
- 並列で複数調査を実行したい
- セキュリティ制約を強制したい

### 組み込みエージェント

| 名前 | 用途 | 特徴 |
|------|------|------|
| Explore | コード検索・分析 | 読み取り専用、高速(Haiku) |
| Plan | 計画のためのリサーチ | コンテキスト収集 |
| General-purpose | 複雑なマルチステップ | 全ツール利用可 |

### カスタムエージェント作成

```
.claude/agents/<name>.md
```

```markdown
---
name: security-auditor
description: セキュリティ監査を実施。脆弱性スキャン、依存関係チェック、コード分析を行う。
tools: Read, Grep, Glob, Bash
model: sonnet
permissionMode: default
---

あなたはセキュリティ監査の専門家です。

## 監査項目
1. 依存関係の脆弱性（npm audit / pip-audit）
2. ハードコードされた認証情報
3. SQLインジェクション、XSS等の脆弱性パターン
4. 不適切なファイルパーミッション

## 出力形式
- 重大度: Critical / High / Medium / Low
- 該当箇所: ファイルパスと行番号
- 説明: 問題の内容
- 修正案: 推奨される対応
```

### permissionMode

| モード | 説明 |
|--------|------|
| `default` | 標準の許可チェック |
| `acceptEdits` | ファイル編集を自動許可 |
| `dontAsk` | 許可プロンプトを自動拒否 |
| `bypassPermissions` | すべてのチェックをバイパス（注意） |
| `plan` | 読み取り専用（プランモード） |

---

## 4. Settings & Permissions

> **出典**: https://code.claude.com/docs/ja/settings
> **最終更新**: 2025-01

### 概要
`settings.json` でツール許可、環境変数、各種動作を設定。

### いつ使う？
- 特定のBashコマンドを事前許可/拒否したい
- プロジェクト固有の環境変数を設定したい
- チームで共通の権限設定を共有したい
- 機密ファイルへのアクセスを制限したい

### 設定ファイルの場所

| スコープ | 場所 | 対象 | 共有 |
|---------|------|------|------|
| **User** | `~/.claude/settings.json` | 全プロジェクト | いいえ |
| **Project** | `.claude/settings.json` | リポジトリ全員 | はい（git） |
| **Local** | `.claude/settings.local.json` | 自分のみ | いいえ |

### 設定の優先順位（高→低）

1. コマンドライン引数
2. Local設定（`.claude/settings.local.json`）
3. Project設定（`.claude/settings.json`）
4. User設定（`~/.claude/settings.json`）

### 基本設定例

```json
{
  "permissions": {
    "allow": [
      "Bash(npm run lint)",
      "Bash(npm run test:*)",
      "Bash(git commit:*)",
      "Read(~/.zshrc)"
    ],
    "deny": [
      "Bash(curl:*)",
      "Read(./.env)",
      "Read(./secrets/**)"
    ]
  },
  "env": {
    "NODE_ENV": "development"
  }
}
```

### 権限ルール（permissions）

```json
{
  "permissions": {
    "allow": [],    // 許可（確認なし）
    "ask": [],      // 確認を求める
    "deny": [],     // 拒否
    "additionalDirectories": [],  // 追加アクセス許可ディレクトリ
    "defaultMode": "acceptEdits"  // デフォルト権限モード
  }
}
```

**評価順序**: `deny` → `ask` → `allow`（最初に一致したルールが適用）

### ツール指定パターン

```json
{
  "permissions": {
    "allow": [
      "Bash",                    // すべてのBashコマンド
      "Bash(npm run build)",     // 完全一致
      "Bash(npm run:*)",         // プレフィックスマッチ（推奨）
      "Bash(git * main)",        // グロブマッチ
      "Read(./.env)",            // 特定ファイル
      "Read(./secrets/**)",      // ディレクトリ配下すべて
      "WebFetch(domain:github.com)"  // 特定ドメイン
    ]
  }
}
```

#### `:*` プレフィックスマッチ（推奨）

単語境界を考慮したマッチング：

```json
"Bash(ls:*)"     // ✅ ls -la に一致、❌ lsof に一致しない
"Bash(git commit:*)"  // git commit -m "msg" に一致
```

#### `*` グロブマッチ

```json
"Bash(git * main)"    // git push main, git pull main 等に一致
"Bash(* --version)"   // すべての --version コマンドに一致
```

### 主な設定プロパティ

| キー | 説明 | 例 |
|------|------|-----|
| `permissions` | ツール使用許可/拒否 | 上記参照 |
| `env` | 環境変数 | `{"NODE_ENV": "dev"}` |
| `hooks` | イベントフック | 次セクション参照 |
| `model` | デフォルトモデル | `"claude-sonnet-4-5-20250929"` |
| `language` | 応答言語 | `"japanese"` |
| `attribution` | コミット/PR属性 | `{"commit": "...", "pr": "..."}` |
| `sandbox` | サンドボックス設定 | 下記参照 |

### サンドボックス設定

```json
{
  "sandbox": {
    "enabled": true,
    "autoAllowBashIfSandboxed": true,
    "excludedCommands": ["docker", "git"]
  }
}
```

### 完全な設定例

```json
{
  "permissions": {
    "allow": [
      "Bash(npm run:*)",
      "Bash(git status:*)",
      "Bash(git diff:*)",
      "Bash(git log:*)",
      "Bash(git add:*)",
      "Bash(git commit:*)",
      "Bash(gh pr:*)",
      "Bash(gh issue:*)",
      "Edit(./src/**)",
      "Edit(./tests/**)"
    ],
    "ask": [
      "Bash(git push:*)",
      "Write(./package.json)"
    ],
    "deny": [
      "Bash(rm -rf:*)",
      "Read(./.env)",
      "Read(./.env.*)",
      "Read(./secrets/**)"
    ],
    "additionalDirectories": ["../docs/"]
  },
  "env": {
    "NODE_ENV": "development"
  },
  "attribution": {
    "commit": "🤖 Generated with Claude Code\n\nCo-Authored-By: Claude <noreply@anthropic.com>",
    "pr": "🤖 Generated with Claude Code"
  }
}
```

---

## 5. Hooks

> **出典**: https://code.claude.com/docs/en/hooks
> **最終更新**: 2025-01

### 概要
特定イベント時に自動実行されるシェルコマンドまたはLLM評価。

### いつ使う？
- ファイル保存時に自動lint/format
- 特定コマンドの実行を検証/ブロック
- 機密ファイルへのアクセスを防止
- セッション開始時に環境セットアップ

### 設定場所

```
.claude/settings.json        # プロジェクト設定
.claude/settings.local.json  # ローカル設定（gitignore推奨）
~/.claude/settings.json      # ユーザー設定
```

### イベント一覧

| イベント | タイミング | 用途例 |
|---------|----------|--------|
| `PreToolUse` | ツール実行前 | コマンド検証、ブロック |
| `PostToolUse` | ツール実行後 | lint、format、テスト |
| `UserPromptSubmit` | プロンプト送信時 | 機密情報チェック |
| `Stop` | 応答終了時 | 完了確認、クリーンアップ |
| `SessionStart` | セッション開始時 | 環境セットアップ |
| `SessionEnd` | セッション終了時 | ログ記録 |

### 設定例

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "npm run lint:fix -- $FILE",
            "timeout": 30
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/validate-bash.sh"
          }
        ]
      }
    ]
  }
}
```

### 終了コード

| コード | 動作 |
|--------|------|
| 0 | 成功（続行） |
| 2 | ブロック（操作拒否） |
| 他 | 警告表示（続行） |

### マッチャーパターン

```json
{ "matcher": "Write" }           // 完全一致
{ "matcher": "Edit|Write" }      // OR条件
{ "matcher": "Notebook.*" }      // 正規表現
{ "matcher": "*" }               // 全ツール
{ "matcher": "mcp__github__.*" } // MCPツール
```

---

## 6. MCP (Model Context Protocol)

> **出典**: https://code.claude.com/docs/en/mcp
> **最終更新**: 2025-01

### 概要
外部ツール・データソースとの統合プロトコル。

### いつ使う？
- GitHub/GitLab等と連携したい
- データベースにクエリしたい
- Slack/Notion等の外部サービスと連携
- 監視ツール（Sentry等）のデータを参照

### インストール

```bash
# リモートHTTPサーバー
claude mcp add --transport http notion https://mcp.notion.com/mcp

# 認証ヘッダー付き
claude mcp add --transport http github https://api.githubcopilot.com/mcp/ \
  --header "Authorization: Bearer $GITHUB_TOKEN"

# ローカルstdioサーバー
claude mcp add --transport stdio postgres -- npx -y @bytebase/dbhub \
  --dsn "postgresql://user:pass@localhost:5432/mydb"
```

### スコープ

```bash
--scope local    # 現プロジェクトのみ（デフォルト）
--scope project  # .mcp.json に保存（チーム共有）
--scope user     # 全プロジェクトで利用可能
```

### 管理コマンド

```bash
claude mcp list              # 一覧
claude mcp get <name>        # 詳細
claude mcp remove <name>     # 削除
/mcp                         # Claude Code内で確認
```

---

## 7. Plugins

> **出典**: https://code.claude.com/docs/en/plugins
> **最終更新**: 2025-01

### 概要
Skills、Agents、Hooks、MCPをバンドルして配布可能にするパッケージ。

### いつ使う？
- チームで設定を共有したい
- 複数の機能をまとめて管理したい
- 社外に配布したい

### 構造

```
my-plugin/
├── .claude-plugin/
│   └── plugin.json      # マニフェスト（必須）
├── commands/            # スラッシュコマンド
├── agents/              # サブエージェント
├── skills/              # スキル
├── hooks/
│   └── hooks.json       # フック設定
├── .mcp.json            # MCPサーバー
└── .lsp.json            # LSPサーバー
```

### plugin.json

```json
{
  "name": "my-team-plugin",
  "description": "チーム共通のClaude Code設定",
  "version": "1.0.0",
  "author": { "name": "Team Name" }
}
```

### 使用

```bash
# ローカルテスト
claude --plugin-dir ./my-plugin

# 複数読み込み
claude --plugin-dir ./plugin-a --plugin-dir ./plugin-b
```

---

## 8. Best Practices

> **出典**: https://code.claude.com/docs/en/best-practices
> **最終更新**: 2025-01

### コンテキスト管理
- `/clear` でタスク切替時にリセット
- `/compact` で手動コンパクション
- サブエージェントで大量探索を分離

### プロンプトの書き方
- 検証方法を提供（テスト実行、スクリーンショット比較等）
- 「探索 → 計画 → コード」の3段階ワークフロー
- 具体的なコンテキストを含める

### CLAUDE.md のコツ
- Claudeが推測できないコマンドを書く
- 非標準の規約のみ記載
- 冗長な説明は避ける
