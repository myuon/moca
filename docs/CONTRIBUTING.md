---
title: Documentation Guidelines
description: ドキュメントの書き方ガイド。frontmatter の形式、構造、スタイルガイドを定義。
---

# Documentation Guidelines

このガイドでは、`docs/` ディレクトリ内のドキュメントの書き方を説明します。

## Frontmatter

すべてのドキュメントには YAML frontmatter を含める必要があります。

```yaml
---
title: Document Title
description: 1-2文でドキュメントの内容を要約。検索やインデックス表示に使用される。
---
```

### Required Fields

| Field | Description |
|-------|-------------|
| `title` | ドキュメントのタイトル。h1 と同じでも良い。 |
| `description` | 1-2文の要約。ファイルを開かなくても内容が分かるようにする。 |

### Optional Fields

| Field | Description |
|-------|-------------|
| `status` | `draft`, `implemented`, `deprecated` など |
| `version` | 仕様のバージョン |

## Document Structure

### Heading Levels

```markdown
# Document Title (h1) - 1つのみ

## Major Section (h2)

### Subsection (h3)

#### Minor Section (h4) - 必要な場合のみ
```

### Recommended Sections

仕様ドキュメントの推奨構造：

1. **Overview** - 機能の概要と目的
2. **Scope** - In Scope / Out of Scope
3. **Specification** - 詳細仕様
4. **Examples** - コード例
5. **Implementation** - 実装ファイルへの参照

## Code Blocks

言語を明示してシンタックスハイライトを有効にする：

````markdown
```rust
fn example() {
    println!("Hello");
}
```

```mc
let x = 42;
print(x);
```
````

Moca 言語のコードブロックには `mc` を使用。

## Tables

| 要素 | 説明 |
|------|------|
| Document | ファイルへのリンクと説明 |
| API | 関数シグネチャと説明 |
| Instructions | 命令と動作 |

## Links

ドキュメント間のリンクは相対パスを使用：

```markdown
See [VM Specification](vm.md) for details.
See [C API](c-api.md#stack-operations) for stack operations.
```

## Language

- 技術仕様は日本語または英語（統一推奨）
- コード、API名、型名は英語のまま
- 簡潔で明確な表現を心がける

## File Naming

- 小文字、ハイフン区切り: `vm-core.md`, `c-api.md`
- 内容を表す名前: `language.md`, `testing.md`
- 略語は一般的なもののみ: `lsp.md`, `jit.md`, `cli.md`

## README.md

`docs/README.md` はドキュメントインデックスとして機能します：

- 全ドキュメントをカテゴリ別にリスト
- 各ドキュメントの説明を frontmatter の `description` から引用
- 新規ドキュメント追加時は README.md も更新する

## Example

完全なドキュメントの例：

```markdown
---
title: Feature Specification
description: 機能の仕様。概要、API、使用例を定義。
---

# Feature Specification

## Overview

この機能は〇〇を提供します。

## API

### `function_name(arg: Type) -> ReturnType`

説明...

## Examples

\`\`\`mc
let result = function_name(value);
\`\`\`

## Implementation

| File | Description |
|------|-------------|
| `src/feature/mod.rs` | メイン実装 |
```
