---
title: CLI Specification
description: Moca ツールチェーンのコマンドラインインターフェース仕様。プロジェクト操作、依存関係管理、開発ツールのコマンドを定義。
---

# Moca CLI Specification

This document defines the command-line interface for the Moca toolchain.

## Commands

### Project Operations

```bash
moca init [name]        # Create new project
moca check              # Type check and static analysis only
moca lint [file]        # Lint source file (after type check)
moca build              # Generate bytecode
moca run [file] [args]  # Execute (uses entry if file omitted)
moca test               # Run tests
```

### Dependency Management

```bash
moca add <url>[@rev]    # Add dependency
moca remove <name>      # Remove dependency
moca update [name]      # Update dependencies (all or specified)
moca vendor             # Copy dependencies locally
```

### Development Tools

```bash
moca lsp                # Start LSP server (stdio)
moca debug [file]       # Start TUI debugger
moca repl               # Start REPL
moca fmt [file|dir]     # Format code
moca clean              # Remove build artifacts
```

### Common Options

```bash
--release               # Enable optimizations
--verbose               # Verbose output
--jit=[on|off|auto]     # JIT compilation mode
--jit-threshold=<n>     # JIT compilation threshold (default: 1000)
--gc-mode=[stw|concurrent]  # GC mode
--trace-jit             # Output JIT compilation info
--gc-stats              # Output GC statistics
```

### Debug Dump Options

コンパイラパイプラインの中間表現を出力するオプション。

```bash
--dump-ast              # AST（抽象構文木）を stderr に出力
--dump-ast=<file>       # AST をファイルに出力
--dump-resolved         # 名前解決済みプログラムを stderr に出力
--dump-resolved=<file>  # 名前解決済みプログラムをファイルに出力
--dump-bytecode         # バイトコードを stderr に出力
--dump-bytecode=<file>  # バイトコードをファイルに出力
```

複数同時指定可能。出力順序は AST → Resolved → Bytecode（パイプライン順）。
ダンプ後もプログラムは通常実行される。

## Accessing CLI Arguments

Scripts can access command-line arguments using built-in functions:

```bash
moca run script.mc hello world 123
```

```moca
// argc() - Returns the number of arguments (including script path)
print(argc());      // 4

// argv(index) - Returns the argument at the given index
print(argv(0));     // "script.mc"
print(argv(1));     // "hello"
print(argv(2));     // "world"
print(argv(3));     // "123"

// args() - Returns all arguments as an array
var all = args();   // ["script.mc", "hello", "world", "123"]
```

- `argv(0)` is always the script file path
- `argv(n)` returns empty string for out-of-bounds index
- Arguments are always strings; use `parse_int()` to convert to numbers

## Linter

`moca lint` はtypecheck成功後のASTを解析し、コード改善の提案を行う。

```bash
moca lint app.mc        # ファイルを指定してlint
moca lint               # pkg.toml の entry をlint
```

- typecheckエラーがある場合はlintを実行しない
- lint警告は stdout に出力される
- 警告がある場合は終了コード `1`、ない場合は `0`

### 出力フォーマット

```
warning: {RULE_NAME}: {MESSAGE}
  --> {FILENAME}:{LINE}:{COLUMN}
```

### Lint Rules

| ルール名 | 検出内容 | サジェスト |
|----------|---------|-----------|
| `prefer-new-literal` | `vec::\`new\`()` の呼び出し | `new Vec<T> {}` 構文の使用 |
| `prefer-index-access` | vec/mapの `.get()` / `.set()` / `.put()` 呼び出し | `[]` インデックス記法の使用 |

### ルールの追加

新しいルールは `src/compiler/linter.rs` で `LintRule` トレイトを実装し、`default_rules()` に追加する。

## Exit Codes

- `0`: Success
- `1`: Error (compile error, runtime error, lint warning, etc.)

## Standard I/O

### stdout

- `print` statement output
- Integer: decimal format with newline
- Float: decimal format with newline
- String: as-is with newline
- Bool: `true` or `false` with newline

### stderr

- Compile errors
- Runtime errors

## Error Format

```
error: <message>
  --> <file>:<line>:<column>
```

## Examples

### Run a Program

```bash
moca run hello.mc
```

### Run with JIT Disabled

```bash
moca run --jit=off app.mc
```

### Run with JIT Tracing

```bash
moca run --trace-jit app.mc
```

### Create New Project

```bash
moca init myapp
cd myapp
moca run
```

### Add a Dependency

```bash
moca add https://github.com/user/moca-utils@v1.0.0
```

### Format Code

```bash
moca fmt src/
```

### Run Tests

```bash
# Run all tests in the project
moca test

# Run tests in a specific directory
moca test src/tests/
```

Test output format:
```
✓ _test_add passed
✓ _test_sub passed
✗ _test_divide failed: runtime error: division by zero

2 passed, 1 failed
```

Exit code is `0` if all tests pass, `1` if any test fails.

### Lint Code

```bash
$ moca lint app.mc
warning: prefer-new-literal: use `new Vec<T> {}` instead of `vec::`new`()`
  --> app.mc:3:9
```

### Dump Compiler IR

```bash
# AST を stderr に出力
moca run example.mc --dump-ast

# バイトコードをファイルに出力
moca run example.mc --dump-bytecode=out.txt

# 複数同時出力
moca run example.mc --dump-ast --dump-resolved --dump-bytecode
```
