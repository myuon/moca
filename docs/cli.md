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

## Exit Codes

- `0`: Success
- `1`: Error (compile error, runtime error, etc.)

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

### Dump Compiler IR

```bash
# AST を stderr に出力
moca run example.mc --dump-ast

# バイトコードをファイルに出力
moca run example.mc --dump-bytecode=out.txt

# 複数同時出力
moca run example.mc --dump-ast --dump-resolved --dump-bytecode
```
