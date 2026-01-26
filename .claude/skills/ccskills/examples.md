# Claude Code カスタマイズ 実装例集

## ユースケース別の実装例

---

## 1. 「ファイル保存時に自動でlint/formatしたい」

**使う機能**: Hooks (PostToolUse)

**.claude/settings.json**:
```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "npx prettier --write \"$FILE\" && npx eslint --fix \"$FILE\"",
            "timeout": 30
          }
        ]
      }
    ]
  }
}
```

---

## 2. 「このプロジェクトのビルド・テストコマンドを覚えさせたい」

**使う機能**: CLAUDE.md

**.claude/CLAUDE.md**:
```markdown
# MyProject

## コマンド
- ビルド: `pnpm build`
- 開発サーバー: `pnpm dev`
- テスト: `pnpm test`
- 単一テスト: `pnpm test -- -t "テスト名"`
- 型チェック: `pnpm typecheck`
- lint: `pnpm lint`

## 注意
- Node.js 20以上が必要
- `.env.local` をコピーして `.env` を作成すること
```

---

## 3. 「特定のディレクトリには触らせたくない」

**使う機能**: Hooks (PreToolUse) + CLAUDE.md

**.claude/settings.json**:
```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/check-protected-paths.sh"
          }
        ]
      }
    ]
  }
}
```

**.claude/hooks/check-protected-paths.sh**:
```bash
#!/bin/bash
INPUT=$(cat)
FILE=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.path // empty')

# 保護対象パス
PROTECTED_PATHS=(
  "src/legacy/"
  "vendor/"
  ".env"
  "secrets/"
)

for path in "${PROTECTED_PATHS[@]}"; do
  if [[ "$FILE" == *"$path"* ]]; then
    echo "ブロック: $path は保護されています" >&2
    exit 2
  fi
done

exit 0
```

**.claude/CLAUDE.md** にも追記:
```markdown
## 触らないファイル/ディレクトリ
- `src/legacy/` - 別チームが管理、移行予定
- `vendor/` - 外部ライブラリ
- `.env` - 本番認証情報
```

---

## 4. 「PRレビューを定型化したい」

**使う機能**: Skills

**.claude/skills/pr-review/SKILL.md**:
```markdown
---
name: pr-review
description: PRのコードレビューを実施。品質、セキュリティ、テストをチェック。
allowed-tools: Read, Grep, Glob, Bash
---

# PRレビュー

## 手順
1. `git diff main...HEAD --stat` で変更概要を確認
2. `git diff main...HEAD` で詳細差分を確認
3. 変更されたファイルを読み込んで分析

## チェック項目

### 必須 (Critical)
- [ ] セキュリティ脆弱性がないか
- [ ] 機密情報がハードコードされていないか
- [ ] エラーハンドリングが適切か

### 推奨 (Warning)
- [ ] テストが追加/更新されているか
- [ ] コードの重複がないか
- [ ] 命名が適切か

### 提案 (Suggestion)
- [ ] パフォーマンス改善の余地
- [ ] より良い実装パターン
- [ ] ドキュメントの追加

## 出力形式
各項目について以下の形式で報告:

🔴 **Critical**: [問題の説明]
- ファイル: path/to/file.ts:123
- 修正案: [具体的な修正方法]

🟡 **Warning**: [問題の説明]
...

🟢 **Suggestion**: [提案内容]
...
```

**使い方**: `/pr-review` または「PRレビューして」

---

## 5. 「データベースには読み取り専用でアクセスさせたい」

**使う機能**: Subagents + Hooks

**.claude/agents/db-reader.md**:
```markdown
---
name: db-reader
description: 読み取り専用でデータベースをクエリ。SELECT文のみ許可。
tools: Bash
model: haiku
hooks:
  PreToolUse:
    - matcher: "Bash"
      hooks:
        - type: command
          command: ".claude/hooks/validate-readonly-sql.sh"
---

あなたはデータベースアナリストです。
ユーザーの質問に答えるためにSELECTクエリを実行してください。

## 接続情報
- ホスト: localhost
- データベース: myapp_dev
- ユーザー: readonly_user

## 制約
- SELECT文のみ使用可能
- INSERT/UPDATE/DELETE/DROP/CREATE/ALTER は禁止
```

**.claude/hooks/validate-readonly-sql.sh**:
```bash
#!/bin/bash
INPUT=$(cat)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

# 書き込み操作をブロック
if echo "$COMMAND" | grep -iE '\b(INSERT|UPDATE|DELETE|DROP|CREATE|ALTER|TRUNCATE)\b' > /dev/null; then
  echo "ブロック: SELECT クエリのみ許可されています" >&2
  exit 2
fi

exit 0
```

---

## 6. 「API設計のルールを特定ファイルにだけ適用したい」

**使う機能**: Rules (パス固有)

**.claude/rules/api-design.md**:
```markdown
---
paths:
  - "src/api/**/*.ts"
  - "src/routes/**/*.ts"
---

# API設計ルール

## エンドポイント命名
- URL: kebab-case (`/user-profiles`)
- メソッド: RESTful (GET/POST/PUT/DELETE)

## レスポンス形式
```typescript
// 成功
{ "data": T, "meta"?: { pagination } }

// エラー
{ "error": { "code": string, "message": string } }
```

## バリデーション
- すべての入力は zod でバリデーション
- エラーは 400 Bad Request で返す

## 認証
- 認証が必要なエンドポイントは `requireAuth` ミドルウェアを使用
```

---

## 7. 「コミット前にテストを必ず通したい」

**使う機能**: Skills (disable-model-invocation)

**.claude/skills/safe-commit/SKILL.md**:
```markdown
---
name: safe-commit
description: テストとlintを通してからコミット
disable-model-invocation: true
allowed-tools: Bash, Read
---

# 安全なコミット手順

1. まずテストを実行
   ```bash
   pnpm test
   ```

2. lintを実行
   ```bash
   pnpm lint
   ```

3. 型チェックを実行
   ```bash
   pnpm typecheck
   ```

4. すべて通ったらコミット
   ```bash
   git add -A
   git commit -m "メッセージ"
   ```

**注意**: 上記のいずれかが失敗した場合、コミットしないこと。
```

**使い方**: `/safe-commit` で明示的に呼び出し

---

## 8. 「セッション開始時に環境をセットアップしたい」

**使う機能**: Hooks (SessionStart)

**.claude/settings.json**:
```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/setup-env.sh"
          }
        ]
      }
    ]
  }
}
```

**.claude/hooks/setup-env.sh**:
```bash
#!/bin/bash

# 環境変数を設定
if [ -n "$CLAUDE_ENV_FILE" ]; then
  echo 'export NODE_ENV=development' >> "$CLAUDE_ENV_FILE"
  echo 'export PATH="$PATH:./node_modules/.bin"' >> "$CLAUDE_ENV_FILE"
fi

# 依存関係チェック
if [ ! -d "node_modules" ]; then
  echo "Warning: node_modules not found. Run 'pnpm install' first." >&2
fi

exit 0
```

---

## 9. 「GitHub/外部サービスと連携したい」

**使う機能**: MCP

```bash
# GitHub連携
claude mcp add --transport http github https://api.githubcopilot.com/mcp/

# Notion連携
claude mcp add --transport http notion https://mcp.notion.com/mcp

# PostgreSQL連携
claude mcp add --transport stdio postgres -- npx -y @bytebase/dbhub \
  --dsn "postgresql://user:pass@localhost:5432/mydb"

# プロジェクト共有設定として保存
claude mcp add --transport http sentry --scope project https://mcp.sentry.dev/mcp
```

---

## 10. 「並列で複数の調査をさせたい」

**使う機能**: Subagents (プロンプトで指示)

```
サブエージェントを使って以下を並列で調査して:
1. 認証フローの実装詳細
2. データベーススキーマの構造
3. API エンドポイントの一覧

それぞれ別のサブエージェントで実行し、結果をまとめて報告して。
```

---

## 11. 「機密情報を含むプロンプトをブロックしたい」

**使う機能**: Hooks (UserPromptSubmit)

**.claude/settings.json**:
```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/check-secrets.sh"
          }
        ]
      }
    ]
  }
}
```

**.claude/hooks/check-secrets.sh**:
```bash
#!/bin/bash
INPUT=$(cat)
PROMPT=$(echo "$INPUT" | jq -r '.prompt // empty')

# 機密パターンをチェック
if echo "$PROMPT" | grep -iE '(password|secret|api[_-]?key|token)\s*[:=]\s*["\x27]?[a-zA-Z0-9]' > /dev/null; then
  echo '{"decision": "block", "reason": "機密情報が含まれている可能性があります。確認してください。"}'
  exit 0
fi

exit 0
```

---

## クイックリファレンス: どれを使う？

| やりたいこと | 機能 | ファイル |
|-------------|------|---------|
| コマンドを教える | CLAUDE.md | `.claude/CLAUDE.md` |
| 規約を適用 | CLAUDE.md / Rules | `.claude/rules/*.md` |
| 自動lint/format | Hooks | `.claude/settings.json` |
| 操作をブロック | Hooks | `.claude/settings.json` + スクリプト |
| ワークフロー定型化 | Skills | `.claude/skills/*/SKILL.md` |
| 読み取り専用エージェント | Subagents | `.claude/agents/*.md` |
| 外部連携 | MCP | `claude mcp add` |
| まとめて配布 | Plugins | `.claude-plugin/plugin.json` |
