# Spec-v1.md — Full Language Frontend + VM + GC

## 1. Goal

- v0 の整数のみから拡張し、**実用的なスクリプト言語**として動作する
- 浮動小数点・文字列・配列・オブジェクトをサポート
- Mark-Sweep GC により長時間実行可能
- High IR / Mid IR / Low IR の3層 IR アーキテクチャを確立

## 2. Non-Goals

- JIT コンパイル（v3）
- 並行実行・マルチスレッド（v3）
- LSP / デバッガ / パッケージシステム（v2）
- Moving / Compacting GC
- 高度な型推論・ジェネリクス

## 3. Target Users

- mica 言語でアプリケーションを書きたい開発者
- 組み込みスクリプトとして mica を利用したいホストアプリ開発者

## 4. Core User Flow

1. ユーザーが `.mica` ファイルを作成（文字列・配列・オブジェクトを使用）
2. `mica run app.mica` を実行
3. コンパイラ: Source → High IR → Mid IR → Low IR → Bytecode
4. VM が Bytecode を実行（GC が自動でメモリ管理）
5. 結果が stdout に出力される

## 5. Inputs & Outputs

### Inputs
- `.mica` ソースファイル（UTF-8）

### Outputs
- stdout: `print` 文の出力（整数・浮動小数点・文字列・bool）
- stderr: コンパイルエラー / ランタイムエラー
- exit code: 0（成功）/ 1（エラー）

## 6. Tech Stack

| カテゴリ | 選定 |
|----------|------|
| 言語 | Rust (edition 2021) |
| パーサ | 手書き再帰下降（v0 から継続） |
| テスト | `cargo test` + integration tests |
| CLI | `clap` |

## 7. Rules & Constraints

### 7.1 追加される型

| 型 | 表現 | 備考 |
|----|------|------|
| `int` | SMI (63-bit) / boxed i64 | v0 から継続 |
| `float` | boxed f64 | IEEE 754 倍精度 |
| `bool` | タグ値 | `true` / `false` |
| `nil` | タグ値 | null 相当 |
| `string` | heap object | UTF-8、イミュータブル |
| `array` | heap object | 可変長、Value の配列 |
| `object` | heap object | フィールド名 → Value |

### 7.2 追加される構文

```
// 文字列リテラル
let s = "hello, world";
let escaped = "line1\nline2";

// 文字列結合
let greeting = "hello" + " " + "world";

// 配列
let arr = [1, 2, 3];
let first = arr[0];
arr[1] = 42;
let len = arr.len();

// オブジェクト
let obj = { x: 10, y: 20 };
let x = obj.x;
obj.y = 30;

// nil
let nothing = nil;

// for-in（配列イテレーション）
for item in arr {
    print(item);
}

// 例外（v1 で導入）
throw "error message";

try {
    risky_operation();
} catch e {
    print(e);
}
```

### 7.3 Value 表現（64-bit Tagged Pointer）

```
下位3bit    種別
-------    ----
000        PTR (heap object)
001        SMI (signed 61-bit integer)
010        BOOL (true=1, false=0 in upper bits)
011        NIL
100        UNDEF
101-111    reserved
```

### 7.4 Heap Object レイアウト

```
+----------------+
| header (64bit) |  - type_id (16bit) + gc_mark (1bit) + flags
+----------------+
| field_count    |  - オブジェクトのフィールド数
+----------------+
| fields[]       |  - Value の配列
+----------------+
```

### 7.5 IR アーキテクチャ

#### High IR
- 言語の意味論を表現
- 参照の読み書きは専用命令（`read_ref`, `write_ref`）
- GC/JIT のフック挿入点を固定
- Verifier で型・スタック整合性を検証可能

#### Mid IR
- GC・最適化向けに正規化
- 参照型と非参照型が明確
- 副作用分類（pure / effectful）
- Safepoint 候補が明示

#### Low IR
- VM / 将来の JIT に近い表現
- 生ポインタ操作
- オブジェクトレイアウト確定
- 明示的 write barrier
- Stack map 生成情報

### 7.6 GC 仕様

#### アルゴリズム
- Mark-Sweep（non-moving）
- STW (Stop-The-World) mark
- Incremental sweep（将来）

#### ルート集合
- VM Value stack
- VM globals
- Call stack 上の locals

#### トリガー
- ヒープ使用量が閾値を超えた時
- `gc_collect()` 明示呼び出し

#### Safepoint
- 関数呼び出し前後
- ループ back-edge
- 大きな alloc 前

### 7.7 Bytecode 追加命令

```
// 定数
PUSH_FLOAT <f64>
PUSH_STRING <const_idx>
PUSH_NIL

// オブジェクト操作
ALLOC_OBJ <type_id> <n_fields>
READ_FIELD <field_idx>
WRITE_FIELD <field_idx>

// 配列操作
ALLOC_ARR <len>
ARR_LEN
ARR_GET
ARR_SET

// 型チェック
IS_PTR
IS_SMI
IS_NIL
TYPE_ID

// 例外
THROW
TRY_BEGIN <handler_offset>
TRY_END

// GC
GC_HINT <bytes>
```

### 7.8 組み込み関数

| 関数 | 説明 |
|------|------|
| `print(v)` | 値を stdout に出力 |
| `len(arr)` | 配列の長さ |
| `push(arr, v)` | 配列に要素追加 |
| `pop(arr)` | 配列から要素削除 |
| `type_of(v)` | 型名を文字列で返す |
| `to_string(v)` | 値を文字列に変換 |
| `parse_int(s)` | 文字列を整数にパース |

## 8. Open Questions

- 文字列の内部表現（UTF-8 直接 vs rope）
- 例外の詳細仕様（スタックトレースの形式）
- オブジェクトのプロパティアクセス最適化（inline cache は v3?）

## 9. Acceptance Criteria

1. 浮動小数点リテラル・演算が正しく動作する
2. 文字列リテラル・結合・比較が正しく動作する
3. 配列の生成・アクセス・変更・`len()` が動作する
4. オブジェクトの生成・フィールドアクセス・変更が動作する
5. `for-in` ループが配列をイテレートできる
6. `throw` / `try-catch` で例外処理ができる
7. GC が動作し、長時間実行でメモリが安定する
8. High IR → Mid IR → Low IR → Bytecode のパイプラインが動作する
9. 組み込み関数（print, len, push, pop, type_of）が動作する
10. 複雑なプログラム（例：簡易 JSON パーサ）が動作する

## 10. Test Plan

### E2E Test 1: 文字列操作

**Given:** 以下の内容の `string.mica`
```
let hello = "Hello";
let world = "World";
let msg = hello + ", " + world + "!";
print(msg);
print(len(msg));
```

**When:** `mica run string.mica` を実行

**Then:** stdout に以下が出力される
```
Hello, World!
13
```

### E2E Test 2: 配列とオブジェクト

**Given:** 以下の内容の `data.mica`
```
let arr = [1, 2, 3];
push(arr, 4);
print(len(arr));

for x in arr {
    print(x);
}

let point = { x: 10, y: 20 };
print(point.x + point.y);
```

**When:** `mica run data.mica` を実行

**Then:** stdout に以下が出力される
```
4
1
2
3
4
30
```

### E2E Test 3: 例外処理と GC

**Given:** 以下の内容の `exception.mica`
```
fn divide(a, b) {
    if b == 0 {
        throw "division by zero";
    }
    return a / b;
}

try {
    print(divide(10, 2));
    print(divide(10, 0));
} catch e {
    print("caught: " + e);
}

// GC stress test
let mut i = 0;
while i < 10000 {
    let arr = [i, i + 1, i + 2];
    i = i + 1;
}
print("done");
```

**When:** `mica run exception.mica` を実行

**Then:** stdout に以下が出力される
```
5
caught: division by zero
done
```
