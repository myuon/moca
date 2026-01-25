---
title: LSP Specification
description: Language Server Protocol による IDE 統合機能の仕様。診断、補完、定義ジャンプ、ホバー情報などをサポート。
---

# Mica LSP Specification

This document defines the Language Server Protocol support for the Mica language.

## Overview

The Mica LSP server provides IDE integration features including:
- Real-time diagnostics
- Code completion
- Go to definition
- Hover information
- Find references
- Code formatting
- Symbol search

## Starting the Server

```bash
mica lsp
```

The server communicates via stdin/stdout using the LSP protocol.

## Supported LSP Methods

| Feature | LSP Method |
|---------|------------|
| Diagnostics | `textDocument/publishDiagnostics` |
| Completion | `textDocument/completion` |
| Go to Definition | `textDocument/definition` |
| Hover | `textDocument/hover` |
| Find References | `textDocument/references` |
| Formatting | `textDocument/formatting` |
| Symbol Search | `workspace/symbol` |

## Diagnostics

Diagnostics are published automatically when files are opened or modified.

### Error Format

```
error[E001]: undefined variable 'foo'
  --> src/main.mica:10:5
   |
10 |     print(foo);
   |           ^^^ not found in this scope
```

### Diagnostic Severity

- Error: Syntax errors, type errors, undefined references
- Warning: Unused variables, unreachable code
- Info: Style suggestions

## Completion

Completion is triggered automatically or by `Ctrl+Space`.

### Completion Targets

- Local variables
- Function names
- Module names
- Field names (after `.`)
- Keywords

### Example

```
let point = { x: 10, y: 20 };
point.|  // Suggests: x, y
```

## Go to Definition

Jump to the definition of:
- Variables
- Functions
- Imported modules

## Hover

Displays information when hovering over:
- Variables (type, value if constant)
- Functions (signature, documentation)
- Types (definition)

## Find References

Find all usages of:
- Variables
- Functions
- Types

## Formatting

Format code according to Mica style guidelines:
- 4-space indentation
- Consistent brace placement
- Appropriate whitespace

## Symbol Search

Search for symbols across the workspace:
- Functions
- Global variables
- Types

## Configuration

LSP settings can be configured in the editor or via `pkg.toml`:

```toml
[lsp]
format_on_save = true
diagnostics_delay_ms = 300
```

## Editor Integration

### VS Code

Install the Mica extension or configure manually:

```json
{
  "mica.server.path": "mica",
  "mica.server.args": ["lsp"]
}
```

### Neovim (with nvim-lspconfig)

```lua
require('lspconfig').mica.setup({
  cmd = { 'mica', 'lsp' },
  filetypes = { 'mica' },
})
```
