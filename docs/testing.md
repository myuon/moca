---
title: Testing
description: moca のテスト機能。ユーザーコード用のテストフレームワークと、コンパイラ開発用のスナップショットテスト。
---

# Testing

moca には2種類のテスト機能があります：

1. **moca test**: ユーザーコード用のテストフレームワーク
2. **Snapshot Testing**: コンパイラ開発用のテストインフラ

---

## moca test

`moca test` コマンドでプロジェクト内のテスト関数を自動検出・実行できます。

### テスト関数の書き方

`_test_` プレフィックスを持つ引数なし・戻り値なしの関数がテスト対象になります。

```moca
fun add(a: int, b: int) -> int {
    return a + b;
}

fun _test_add() {
    assert_eq(add(1, 2), 3, "1 + 2 should be 3");
    assert_eq(add(0, 0), 0, "0 + 0 should be 0");
}

fun _test_add_negative() {
    assert_eq(add(-1, -2), -3, "-1 + -2 should be -3");
}
```

### アサーション関数

標準ライブラリ（std/prelude.mc）で提供されるアサーション関数：

| 関数 | 説明 |
|------|------|
| `assert(condition, msg)` | 条件が false なら msg でエラーを throw |
| `assert_eq(actual, expected, msg)` | int 値が等しくなければエラーを throw |
| `assert_eq_str(actual, expected, msg)` | string 値が等しくなければエラーを throw |
| `assert_eq_bool(actual, expected, msg)` | bool 値が等しくなければエラーを throw |

失敗時のエラーメッセージには expected/actual の値が含まれます：
```
runtime error: values should match (expected: 3, actual: 2)
```

### テストの実行

```bash
# プロジェクトの src/ 配下を検索して実行
moca test

# 特定のディレクトリを指定
moca test src/math/
```

### 出力形式

```
✓ _test_add passed
✓ _test_add_negative passed
✗ _test_divide_by_zero failed: runtime error: division by zero

2 passed, 1 failed
```

- 全テスト成功: 終了コード `0`
- 1つでも失敗: 終了コード `1`
- 1つ失敗しても残りのテストは継続実行

### Compiler API

Rust から直接テストを実行する API も提供されています：

```rust
use moca::compiler::{run_tests, TestResults};
use moca::config::RuntimeConfig;

let results: TestResults = run_tests(&path, &RuntimeConfig::default())?;
println!("{} passed, {} failed", results.passed, results.failed);
```

---

## Snapshot Testing

moca コンパイラ開発用のスナップショットテストは、`.mc`ファイルとその期待出力を外部ファイルで管理する仕組みです。

## ディレクトリ構造

```
tests/snapshots/
├── basic/           # 基本テスト（デフォルトオプション）
├── errors/          # エラー系テスト（exitcode != 0）
├── jit/             # JITテスト（--jit=on で実行）
├── modules/         # 複数ファイルテスト（import）
└── ffi/             # FFI関連テスト（--dump-bytecode 等）
```

## ファイル形式

### 単一ファイルテスト

```
tests/snapshots/basic/
├── arithmetic.mc        # テスト対象のソースコード
├── arithmetic.stdout    # 期待される標準出力（完全一致）
├── arithmetic.stderr    # 期待される標準エラー（部分一致、省略可）
└── arithmetic.exitcode  # 期待される終了コード（省略時は0）
```

### 複数ファイルテスト

ディレクトリに`main.mc`を配置すると、複数ファイルのテストとして認識されます。

```
tests/snapshots/modules/
├── relative_import/          # テストディレクトリ
│   ├── main.mc              # エントリポイント
│   └── helper.mc            # インポートされるモジュール
└── relative_import.stdout   # 期待出力（ディレクトリと同名）
```

### カスタムCLI引数

`.args`ファイルでテストごとのCLI引数を指定できます。

```
tests/snapshots/ffi/
├── dump_bytecode.mc
├── dump_bytecode.args    # 内容: --dump-bytecode
├── dump_bytecode.stdout
└── dump_bytecode.stderr
```

## 検証ルール

| ファイル | 検証方法 |
|----------|----------|
| `.stdout` | 完全一致 |
| `.stderr` | 部分一致（期待文字列が含まれていればOK） |
| `.exitcode` | 完全一致（省略時は0を期待） |

## テストの追加方法

### 基本テストの追加

1. `tests/snapshots/basic/`に`.mc`ファイルを作成
2. 同名の`.stdout`ファイルに期待出力を記述
3. `cargo test snapshot_basic`で実行

```bash
# 例: 新しいテスト "my_test" を追加
echo 'print(1 + 2);' > tests/snapshots/basic/my_test.mc
echo '3' > tests/snapshots/basic/my_test.stdout
cargo test snapshot_basic
```

### エラーテストの追加

1. `tests/snapshots/errors/`に`.mc`ファイルを作成
2. `.stderr`に期待されるエラーメッセージの一部を記述
3. `.exitcode`に`1`を記述

```bash
# 例: ゼロ除算エラーのテスト
echo 'print(1 / 0);' > tests/snapshots/errors/div_zero.mc
echo 'division by zero' > tests/snapshots/errors/div_zero.stderr
echo '1' > tests/snapshots/errors/div_zero.exitcode
```

### JIT一致検証

`basic/`と`jit/`に同じテストを配置することで、JIT有無で結果が変わらないことを検証できます。

## テスト実行

```bash
# 全スナップショットテストを実行
cargo test snapshot

# 特定カテゴリのみ実行
cargo test snapshot_basic
cargo test snapshot_errors
cargo test snapshot_jit
cargo test snapshot_modules
cargo test snapshot_ffi
```

## 実装

テストランナーは `tests/snapshot_tests.rs` に実装されています。
