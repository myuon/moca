---
name: moca-guide
description: mocaコードを書く際のリファレンス。言語仕様、構文、組み込み関数などを参照できる。
allowed-tools: Read, Glob, Grep
---

# Moca Guide

mocaコードを書く際のリファレンスとして使用するスキル。言語仕様、構文、組み込み関数などを参照できる。

## 呼び出し方

```
/moca-guide              # リファレンス表示
```

---

## CLI

### 基本コマンド

```bash
moca init [name]        # 新規プロジェクト作成
moca check [file]       # 型チェックのみ
moca run [file] [args]  # 実行
moca test [dir]         # テスト実行
moca debug <file>       # TUIデバッガー起動
moca lsp                # LSPサーバー起動
```

### コード直接実行（-c / --code）

ファイルなしでコードを直接実行できる：

```bash
# 簡単な計算
moca run -c "print(1 + 2);"

# 複数行
moca run -c "
let x = 10;
let y = 20;
print(x + y);
"

# 引数付き
moca run -c "print(argv(1));" hello
```

### 実行オプション

```bash
--jit=[on|off|auto]     # JITモード（デフォルト: auto）
--jit-threshold=<n>     # JIT閾値（デフォルト: 1000）
--trace-jit             # JITコンパイル情報を出力
--gc-mode=[stw|concurrent]  # GCモード
--gc-stats              # GC統計を出力
--timeout=<秒>          # 実行タイムアウト（0=無制限）
--profile-opcodes       # オペコード実行プロファイル
```

### デバッグダンプ

```bash
--dump-ast              # ASTをstderrに出力
--dump-ast=<file>       # ASTをファイルに出力
--dump-resolved         # 名前解決済みIRを出力
--dump-resolved=<file>
--dump-bytecode         # バイトコードを出力
--dump-bytecode=<file>

# 複数同時指定可能
moca run file.mc --dump-ast --dump-bytecode
```

---

## 基本構文

### 変数宣言

```mc
// 不変変数
let x = 42;

// 可変変数
var y = 0;
y = y + 1;
```

### 関数定義

```mc
// 関数定義（戻り値の型は -> で指定）
fun add(a: int, b: int) -> int {
    return a + b;
}

// 型注釈は省略可能（推論される）
fun double(x) {
    return x * 2;
}

// 関数呼び出し
let result = add(1, 2);
```

**重要**: 戻り値の型は `-> type` 形式（`: type` ではない）

### 制御フロー

```mc
// if-else
if x > 0 {
    print(x);
} else {
    print(0);
}

// while ループ
while y < 10 {
    print(y);
    y = y + 1;
}

// for-in ループ
for item in arr {
    print(item);
}
```

### リテラル

```mc
// 整数
let a = 42;

// 真偽値
let t = true;
let f = false;

// 文字列
let s = "hello";
let escaped = "line1\nline2";

// 配列
var arr = [1, 2, 3];
let first = arr[0];
arr[1] = 42;

// nil
let nothing = nil;
```

---

## 型システム

### 基本型

| 型 | 説明 | 例 |
|----|------|-----|
| `int` | 整数（63-bit） | `42` |
| `float` | 浮動小数点 | `3.14` |
| `bool` | 真偽値 | `true`, `false` |
| `string` | 文字列（UTF-8） | `"hello"` |
| `nil` | null相当 | `nil` |
| `any` | 任意の型（型チェックをバイパス） | |
| `array<T>` | 配列 | `[1, 2, 3]` |
| `T?` | nullable型 | `int?` |

### 型注釈

```mc
// 変数
let x: int = 1;
let name: string? = nil;

// 関数
fun greet(name: string) -> string {
    return "Hello, " + name;
}
```

### any型

```mc
let x: any = 42;        // どんな型も代入可能
let y: any = "hello";
let z: int = x;         // anyから具体的な型に代入可能
```

---

## 構造体

### 定義

```mc
struct Point {
    x: int,
    y: int,
}
```

### インスタンス作成

```mc
let p = Point { x: 10, y: 20 };
```

### フィールドアクセス・代入

```mc
let x = p.x;     // 読み取り
p.x = 100;       // 書き込み
```

### メソッド（impl ブロック）

```mc
impl Point {
    fun distance(self) -> float {
        return sqrt(self.x * self.x + self.y * self.y);
    }

    fun move(self, dx: int, dy: int) {
        self.x = self.x + dx;
        self.y = self.y + dy;
    }
}

var p = Point { x: 3, y: 4 };
let d = p.distance();  // 5.0
p.move(1, 1);          // p は { x: 4, y: 5 }
```

---

## 組み込み関数

### 基本

| 関数 | 説明 |
|------|------|
| `print(v)` | 値を出力 |
| `len(arr)` | 配列の長さ |
| `push(arr, v)` | 配列に追加 |
| `pop(arr)` | 配列から削除 |
| `type_of(v)` | 型名を文字列で取得 |
| `to_string(v)` | 文字列に変換 |
| `parse_int(s)` | 文字列を整数に変換 |

### Vector

```mc
var vec = vec_new();
vec_push(vec, 10);
vec_push(vec, 20);

let first = vec[0];    // インデックスアクセス
vec[1] = 25;           // インデックス代入

let length = vec_len(vec);
let last = vec_pop(vec);
```

### HashMap

```mc
let m = map_new_any();

// string キー
map_put_string(m, "x", 10);
let x = map_get_string(m, "x");
let has = map_has_string(m, "x");

// int キー
map_put_int(m, 1, "value");
let v = map_get_int(m, 1);

// ユーティリティ
let size = map_size(m);
let keys = map_keys(m);
let values = map_values(m);
```

### コマンドライン引数

```mc
print(argc());     // 引数の数
print(argv(0));    // スクリプトパス（-c使用時は "<code>"）
print(argv(1));    // 第1引数
var all = args();  // 全引数の配列
```

---

## 例外処理

```mc
// 例外を投げる
throw "error message";

// try-catch
try {
    risky_operation();
} catch e {
    print(e);
}
```

---

## サンプルコード

### FizzBuzz

```mc
fun fizzbuzz(n) {
    var i = 1;
    while i <= n {
        if i % 15 == 0 {
            print("FizzBuzz");
        } else if i % 3 == 0 {
            print("Fizz");
        } else if i % 5 == 0 {
            print("Buzz");
        } else {
            print(i);
        }
        i = i + 1;
    }
}

fizzbuzz(15);
```

### Fibonacci

```mc
fun fib(n) {
    if n <= 1 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

var i = 0;
while i < 10 {
    print(fib(i));
    i = i + 1;
}
```

---

## 注意事項

- 戻り値の型は `-> type` 形式を使用する（`: type` ではない）
- 構造体の比較（`==`, `!=`）は不可
- 配列の全要素は同一型である必要がある
- `let` は不変、`var` は可変
