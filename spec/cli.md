---
title: CLI Specification
description: Mica ツールチェーンのコマンドラインインターフェース仕様。プロジェクト操作、依存関係管理、開発ツールのコマンドを定義。
---

# Mica CLI Specification

This document defines the command-line interface for the Mica toolchain.

## Commands

### Project Operations

```bash
mica init [name]        # Create new project
mica check              # Type check and static analysis only
mica build              # Generate bytecode
mica run [file] [args]  # Execute (uses entry if file omitted)
mica test               # Run tests
```

### Dependency Management

```bash
mica add <url>[@rev]    # Add dependency
mica remove <name>      # Remove dependency
mica update [name]      # Update dependencies (all or specified)
mica vendor             # Copy dependencies locally
```

### Development Tools

```bash
mica lsp                # Start LSP server (stdio)
mica debug [file]       # Start TUI debugger
mica repl               # Start REPL
mica fmt [file|dir]     # Format code
mica clean              # Remove build artifacts
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
mica run hello.mica
```

### Run with JIT Disabled

```bash
mica run --jit=off app.mica
```

### Run with JIT Tracing

```bash
mica run --trace-jit app.mica
```

### Create New Project

```bash
mica init myapp
cd myapp
mica run
```

### Add a Dependency

```bash
mica add https://github.com/user/mica-utils@v1.0.0
```

### Format Code

```bash
mica fmt src/
```

### Dump Compiler IR

```bash
# AST を stderr に出力
mica run example.mica --dump-ast

# バイトコードをファイルに出力
mica run example.mica --dump-bytecode=out.txt

# 複数同時出力
mica run example.mica --dump-ast --dump-resolved --dump-bytecode
```
