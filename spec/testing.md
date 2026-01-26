# Snapshot Testing

moca のスナップショットテストは、`.mc`ファイルとその期待出力を外部ファイルで管理する仕組みです。

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
