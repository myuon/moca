# Spec-v0.md — Minimal Mica Compiler & VM

## 1. Goal

- `mica run <file>` で FizzBuzz プログラムが実行できる
- 最小限の言語機能（整数・変数・関数・制御構文・print）を備えた動作するコンパイラと VM を構築する

## 2. Non-Goals

- 浮動小数点数（f64）
- 文字列型（print は整数のみ出力）
- 配列・オブジェクト・ヒープ割当
- GC（v0 ではメモリリークを許容）
- 複数ファイル・モジュールシステム
- LSP / デバッガ / パッケージシステム
- JIT コンパイル
- エラーリカバリ（最初のエラーで停止）

## 3. Target Users

- mica 言語の開発者自身（セルフホスト前の検証用）
- 言語処理系に興味のある開発者

## 4. Core User Flow

1. ユーザーが `.mica` ファイルを作成
2. `mica run fizzbuzz.mica` を実行
3. コンパイラがソースを読み込み → Lexer → Parser → AST → Bytecode 生成
4. VM が Bytecode を実行
5. `print` 文の結果が stdout に出力される
6. 正常終了時は exit code 0、エラー時は非ゼロ + stderr にメッセージ

## 5. Inputs & Outputs

### Inputs
- `.mica` ソースファイル（UTF-8）

### Outputs
- stdout: `print` 文の出力（整数は10進数、改行付き）
- stderr: コンパイルエラー / ランタイムエラー
- exit code: 0（成功）/ 1（エラー）

## 6. Tech Stack

| カテゴリ | 選定 |
|----------|------|
| 言語 | Rust (edition 2021) |
| パーサ | 手書き再帰下降 |
| テスト | `cargo test` |
| CLI | `clap` |
| ビルド | `cargo` |

## 7. Rules & Constraints

### 7.1 ソース言語構文（v0 最小）

```
// 行コメント

// 変数宣言（イミュータブル）
let x = 42;

// 変数宣言（ミュータブル）
var y = 0;

// 代入（var 変数のみ）
y = y + 1;

// 関数定義
fun add(a, b) {
    return a + b;
}

// 関数呼び出し
let result = add(1, 2);

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

// print（組み込み関数）
print(42);
```

### 7.2 トークン一覧

| カテゴリ | トークン |
|----------|----------|
| キーワード | `let`, `var`, `fun`, `if`, `else`, `while`, `return`, `true`, `false` |
| リテラル | 整数（`0`, `42`, `-1`）, bool（`true`, `false`） |
| 識別子 | `[a-zA-Z_][a-zA-Z0-9_]*` |
| 演算子 | `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, `||`, `!` |
| 区切り | `(`, `)`, `{`, `}`, `,`, `;`, `=` |
| コメント | `//` から行末まで |

### 7.3 演算子優先順位（低→高）

1. `||`
2. `&&`
3. `==`, `!=`
4. `<`, `<=`, `>`, `>=`
5. `+`, `-`
6. `*`, `/`, `%`
7. `!`, `-`（単項）

### 7.4 文法（EBNF 簡易）

```ebnf
program     = { item } ;
item        = fn_def | statement ;

fn_def      = "fun" IDENT "(" [ params ] ")" block ;
params      = IDENT { "," IDENT } ;

block       = "{" { statement } "}" ;

statement   = let_stmt
            | var_stmt
            | assign_stmt
            | if_stmt
            | while_stmt
            | return_stmt
            | expr_stmt ;

let_stmt    = "let" IDENT "=" expr ";" ;
var_stmt    = "var" IDENT "=" expr ";" ;
assign_stmt = IDENT "=" expr ";" ;
if_stmt     = "if" expr block [ "else" block ] ;
while_stmt  = "while" expr block ;
return_stmt = "return" [ expr ] ";" ;
expr_stmt   = expr ";" ;

expr        = or_expr ;
or_expr     = and_expr { "||" and_expr } ;
and_expr    = eq_expr { "&&" eq_expr } ;
eq_expr     = cmp_expr { ( "==" | "!=" ) cmp_expr } ;
cmp_expr    = add_expr { ( "<" | "<=" | ">" | ">=" ) add_expr } ;
add_expr    = mul_expr { ( "+" | "-" ) mul_expr } ;
mul_expr    = unary_expr { ( "*" | "/" | "%" ) unary_expr } ;
unary_expr  = ( "!" | "-" ) unary_expr | call_expr ;
call_expr   = primary [ "(" [ args ] ")" ] ;
args        = expr { "," expr } ;
primary     = INT | "true" | "false" | IDENT | "(" expr ")" ;
```

### 7.5 意味論

- 整数は 63-bit 符号付き（SMI として 64-bit Value に埋め込み）
- bool は `true` = 1, `false` = 0 として扱う
- ゼロ除算はランタイムエラー
- 未定義変数の参照はコンパイルエラー
- 関数は定義前に呼び出し可能（hoisting あり）
- `print` は組み込み関数（引数1つ、整数を stdout に出力）
- トップレベルの文は暗黙の `main` として順次実行

### 7.6 Bytecode 命令セット（v0 最小）

```
// スタック操作
PUSH_INT <i64>      // 整数をプッシュ
PUSH_TRUE           // true をプッシュ
PUSH_FALSE          // false をプッシュ
POP                 // スタックトップを破棄

// ローカル変数
LOAD_LOCAL <idx>    // locals[idx] をプッシュ
STORE_LOCAL <idx>   // スタックトップを locals[idx] に格納（pop）

// グローバル/関数
LOAD_GLOBAL <idx>   // globals[idx] をプッシュ
CALL <argc>         // スタック上の関数を argc 引数で呼び出し

// 算術
ADD, SUB, MUL, DIV, MOD
NEG                 // 単項マイナス

// 比較
EQ, NE, LT, LE, GT, GE

// 論理
NOT                 // 論理否定
// && と || は短絡評価のため JMP 系で実装

// 制御
JMP <offset>        // 無条件ジャンプ
JMP_IF_FALSE <offset>  // false なら offset へ
JMP_IF_TRUE <offset>   // true なら offset へ（短絡評価用）
RET                 // 関数から戻る

// 組み込み
PRINT               // スタックトップを stdout に出力
```

### 7.7 VM アーキテクチャ

- スタックベース VM
- Value: 64-bit（下位 1-bit でタグ、0=SMI, 1=その他は v0 では未使用）
- フレーム: `pc`, `locals[]`, `stack_base`
- コールスタック: Vec<Frame>
- 組み込み関数 `print` は VM 内で直接処理

### 7.8 エラー処理

- Lexer エラー: 不正な文字、未終端文字列等
- Parser エラー: 構文エラー（位置情報付き）
- 名前解決エラー: 未定義変数/関数
- Runtime エラー: ゼロ除算、スタックオーバーフロー

エラーフォーマット:
```
error: <message>
  --> <file>:<line>:<column>
```

## 8. Open Questions

なし（v0 は最小スコープで確定）

## 9. Acceptance Criteria

1. `mica run` コマンドが存在し、`.mica` ファイルを引数に取れる
2. 整数リテラル・四則演算・剰余が正しく計算される
3. 比較演算子（`==`, `!=`, `<`, `<=`, `>`, `>=`）が正しく動作する
4. `let` で immutable 変数、`var` で mutable 変数が宣言できる
5. `if-else` 文が条件に応じて正しく分岐する
6. `while` 文がループとして正しく動作する
7. `fun` で関数定義、呼び出しが正しく動作する
8. `return` 文が値を返す
9. `print` で整数が stdout に出力される（改行付き）
10. FizzBuzz プログラム（下記）が正しく動作する

## 10. Test Plan

### E2E Test 1: 基本演算

**Given:** 以下の内容の `arith.mica`
```
let x = 10 + 20 * 2;
print(x);
let y = x % 7;
print(y);
```

**When:** `mica run arith.mica` を実行

**Then:** stdout に以下が出力される
```
50
1
```

### E2E Test 2: 制御構文

**Given:** 以下の内容の `control.mica`
```
var i = 0;
while i < 5 {
    if i % 2 == 0 {
        print(i);
    }
    i = i + 1;
}
```

**When:** `mica run control.mica` を実行

**Then:** stdout に以下が出力される
```
0
2
4
```

### E2E Test 3: FizzBuzz

**Given:** 以下の内容の `fizzbuzz.mica`
```
fun fizzbuzz(n) {
    var i = 1;
    while i <= n {
        if i % 15 == 0 {
            print(-3);  // FizzBuzz を -3 で表現（文字列なしのため）
        } else if i % 3 == 0 {
            print(-1);  // Fizz を -1 で表現
        } else if i % 5 == 0 {
            print(-2);  // Buzz を -2 で表現
        } else {
            print(i);
        }
        i = i + 1;
    }
}

fizzbuzz(15);
```

**When:** `mica run fizzbuzz.mica` を実行

**Then:** stdout に以下が出力される
```
1
2
-1
4
-2
-1
7
8
-1
-2
11
-1
13
14
-3
```

---

## Appendix: サンプルプログラム

### fibonacci.mica
```
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

### factorial.mica
```
fun fact(n) {
    if n <= 1 {
        return 1;
    }
    return n * fact(n - 1);
}

print(fact(5));  // 120
print(fact(10)); // 3628800
```
