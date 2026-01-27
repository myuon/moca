---
title: Moca Documentation
description: Moca プログラミング言語とバイトコード VM のドキュメント一覧。言語仕様、VM アーキテクチャ、ツールチェーンを網羅。
---

# Moca Documentation

このディレクトリには Moca プログラミング言語と mocaVM の技術ドキュメントが含まれています。

## Documents

### Language

| Document | Description |
|----------|-------------|
| [language.md](language.md) | Moca プログラミング言語の構文とセマンティクス。型システム、制御フロー、並行処理、例外処理を定義。 |
| [types.md](types.md) | 静的型システムの仕様。Hindley-Milner 型推論による型安全性保証、nullable 型、配列・オブジェクト型を定義。 |
| [structs.md](structs.md) | 構造体型の仕様。固定フィールドを持つデータ型、impl ブロックによるメソッド定義、名義的型付けを定義。 |

### Virtual Machine

| Document | Description |
|----------|-------------|
| [vm.md](vm.md) | 仮想マシンアーキテクチャの仕様。バイトコード命令セット、64ビットタグ付き値表現、Mark-Sweep GC を定義。 |
| [vm-core.md](vm-core.md) | VM コア機能の仕様。Value 表現、命令セット、Verifier ルール、StackMap フォーマット、GC 統合を定義。 |
| [jit.md](jit.md) | JIT コンパイルと実行時最適化機能の仕様。Tier 0 インタプリタと Tier 1 ベースライン JIT の2段階実行モデル。 |
| [c-api.md](c-api.md) | C 言語 API の仕様。VM ライフサイクル、スタック操作、関数呼び出し、バイトコードのシリアライズを定義。 |

### Toolchain

| Document | Description |
|----------|-------------|
| [cli.md](cli.md) | Moca ツールチェーンのコマンドラインインターフェース仕様。プロジェクト操作、依存関係管理、開発ツールのコマンドを定義。 |
| [package.md](package.md) | パッケージ管理システムの仕様。プロジェクト構造、依存関係解決、ロックファイルによる再現可能ビルドを定義。 |
| [lsp.md](lsp.md) | Language Server Protocol による IDE 統合機能の仕様。診断、補完、定義ジャンプ、ホバー情報などをサポート。 |
| [debugger.md](debugger.md) | TUI デバッガーの仕様。ブレークポイント、ステップ実行、変数検査、コールスタック確認機能を提供。 |

### Testing

| Document | Description |
|----------|-------------|
| [testing.md](testing.md) | スナップショットテストの仕様。.mc ファイルと期待出力を外部ファイルで管理するテストインフラストラクチャ。 |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Host Application (C/C++)                 │
├─────────────────────────────────────────────────────────────┤
│                        C FFI Layer                           │
│   moca_vm_new(), moca_call(), moca_push_*(), moca_to_*()    │
├─────────────────────────────────────────────────────────────┤
│                        Rust Core                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │  Verifier   │  │  Interpreter│  │  GC (Precise, STW)  │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                   Bytecode / StackMap                    ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

## Quick Reference

### Value Types

```
Value = I64(i64) | F64(f64) | Bool(bool) | Ref(GcRef) | Null
```

### Core Instructions

| Category | Instructions |
|----------|-------------|
| Constants | `CONST`, `GETL`, `SETL` |
| Stack | `POP`, `DUP` |
| Arithmetic | `ADD_I64`, `SUB_I64`, `MUL_I64`, `DIV_I64`, `ADD_F64`, `SUB_F64`, `MUL_F64`, `DIV_F64` |
| Comparison | `EQ`, `LT_I64`, `LT_F64` |
| Control | `JMP`, `JMP_IF_TRUE`, `JMP_IF_FALSE` |
| Calls | `CALL`, `RET` |
| Heap | `NEW`, `GETF`, `SETF` |

### C API Example

```c
#include <moca.h>

int main() {
    moca_vm *vm = moca_vm_new();

    moca_load_file(vm, "program.mocac");

    moca_push_i64(vm, 42);
    moca_call(vm, "process", 1);

    int64_t result = moca_to_i64(vm, -1);

    moca_vm_free(vm);
    return 0;
}
```
