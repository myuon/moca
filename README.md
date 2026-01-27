# Moca

**Moca** は軽量で組み込み可能な静的型付きスクリプト言語です。Lua のようなシンプルさと、静的型付けによる安全性を両立します。

## Features

- **静的型推論** - Hindley-Milner 型推論により、型注釈なしでも型安全性を保証
- **軽量組み込み** - C API 経由でアプリケーションに組み込み可能
- **高速実行** - 2段階 JIT（インタプリタ → ベースライン JIT）による最適化
- **豊富な開発ツール** - LSP、TUI デバッガー、パッケージマネージャ

## Quick Start

```bash
# プロジェクト作成
moca init myproject
cd myproject

# 実行
moca run

# 型チェックのみ
moca check
```

## Hello World

```mc
fun greet(name) {
    print("Hello, " + name + "!");
}

greet("World");
```

## Language Overview

```mc
// 変数（let = 不変、var = 可変）
let x = 42;
var y = 0;

// 関数
fun add(a, b) {
    return a + b;
}

// 構造体
struct Point {
    x: int,
    y: int,
}

let p = Point { x: 10, y: 20 };

// 制御フロー
if x > 0 {
    print(x);
}

while y < 10 {
    y = y + 1;
}

// 例外処理
try {
    risky_operation();
} catch e {
    print(e);
}
```

## Embedding

C/C++ アプリケーションに組み込む：

```c
#include <moca.h>

int main() {
    MocaVm *vm = moca_vm_new();

    moca_load_file(vm, "script.mocac");

    moca_push_i64(vm, 42);
    moca_call(vm, "process", 1);

    int64_t result = moca_to_i64(vm, -1);

    moca_vm_free(vm);
    return 0;
}
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                  Host Application (C/C++)                │
├─────────────────────────────────────────────────────────┤
│                       C FFI Layer                        │
├─────────────────────────────────────────────────────────┤
│                       Rust Core                          │
│  ┌───────────┐  ┌────────────┐  ┌───────────────────┐   │
│  │  Compiler │  │ Interpreter│  │  JIT (AArch64/x64)│   │
│  └───────────┘  └────────────┘  └───────────────────┘   │
│  ┌───────────┐  ┌────────────┐  ┌───────────────────┐   │
│  │  Verifier │  │  GC (STW)  │  │      LSP/Debug    │   │
│  └───────────┘  └────────────┘  └───────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `moca init [name]` | プロジェクト作成 |
| `moca run [file]` | プログラム実行 |
| `moca check` | 型チェック |
| `moca build` | バイトコード生成 |
| `moca test` | テスト実行 |
| `moca lsp` | LSP サーバー起動 |
| `moca debug [file]` | デバッガー起動 |

## Documentation

詳細なドキュメントは [docs/](docs/README.md) を参照してください：

- [Language Specification](docs/language.md) - 言語仕様
- [Type System](docs/types.md) - 静的型システム
- [VM Architecture](docs/vm.md) - 仮想マシン
- [C API](docs/c-api.md) - 組み込み API
- [CLI Reference](docs/cli.md) - コマンドライン

## Building

```bash
# ビルド
cargo build --release

# テスト
cargo test

# JIT 有効でビルド
cargo build --release --features jit
```

## License

MIT
