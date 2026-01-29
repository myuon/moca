---
title: Static Type System Specification
description: 静的型システムの仕様。Hindley-Milner 型推論による型安全性保証、nullable 型、配列・オブジェクト型を定義。
---

# Static Type System Specification

## 1. Goal

- mocaを静的型付き言語にする
- Hindley-Milner型推論により、型注釈を省略しても型安全性を保証する
- 型情報を持つIRを設計し、将来のJIT最適化の基盤とする

## 2. Non-Goals

- ジェネリクス（多相型）: 将来対応、今回は実装しない
- クラス / トレイト / インターフェース
- 型エイリアス (`type ID = int`)
- ユニオン型 (`int | string`)
- リテラル型 (`"hello"` 型)
- 構造的部分型（`{x: int, y: int}` が `{x: int}` の部分型にはならない）

## 3. Target Users

- moca言語の利用者: 型注釈なしでも型安全なコードを書きたい
- moca言語の開発者: 型情報を活用した最適化を実装したい

## 4. Core User Flow

1. ユーザーがソースコードを書く（型注釈あり/なし混在可）
2. `moca run file.mc` または `moca check file.mc` を実行
3. 型推論が走り、全ての式・変数に型が付与される
4. 型エラーがあれば、エラー位置と理由を表示して終了（実行しない）
5. 型エラーがなければ、型付きIRからバイトコード生成→実行

## 5. Inputs & Outputs

### Inputs
- moca ソースコード（`.mc` ファイル）
- 型注釈（オプション）: `let x: int = 1`, `fun f(a: int) -> string`

### Outputs
- 成功時: 型付きIR → バイトコード → 実行結果
- 失敗時: 型エラーメッセージ（ファイル名、行、列、エラー内容）

## 6. Tech Stack

- 言語: Rust（既存コードベース）
- 型推論: Algorithm W（Hindley-Milner）
- 新規モジュール:
  - `src/compiler/types.rs`: 型の定義
  - `src/compiler/typechecker.rs`: 型推論・型検査
- 既存モジュール変更:
  - `src/compiler/ast.rs`: 型注釈のAST拡張
  - `src/compiler/parser.rs`: 型注釈構文のパース
  - `src/compiler/resolver.rs`: 型情報の伝播
- テスト: Rustユニットテスト + 型エラーケースのintegrationテスト

## 7. Rules & Constraints

### 型システムのルール

#### 基本型
| 型 | 構文 | 例 |
|----|------|-----|
| 整数 | `int` | `42` |
| 浮動小数点 | `float` | `3.14` |
| 真偽値 | `bool` | `true`, `false` |
| 文字列 | `string` | `"hello"` |
| nil | `nil` | `nil` |
| any | `any` | 任意の値（型チェックをバイパス） |
| 配列 | `array<T>` | `[1, 2, 3]` |
| オブジェクト | `{field: T, ...}` | `{x: 1, y: 2}` |
| nullable | `T?` | `int?` は `int` または `nil` |
| 関数 | `(T1, T2) -> R` | `(int, int) -> int` |

#### 型注釈構文
```
// 変数
let x: int = 1;
let y = 2;  // 推論で int

// 関数
fun add(a: int, b: int) -> int {
  a + b
}

fun double(x) {  // 引数・戻り値とも推論
  x * 2
}

// nullable
let name: string? = nil;
```

#### 型推論ルール
- リテラルは対応する型: `42` → `int`, `"a"` → `string`
- 変数は初期化式から推論
- 関数引数は使用箇所から推論（制約収集 → 単一化）
- 戻り値は return 式から推論
- 推論できない場合はコンパイルエラー

#### any型
`any`型は型チェックをバイパスするための特殊な型です。

```
let x: any = 42;        // OK: int を any に代入
let y: any = "hello";   // OK: string を any に代入
let z: int = x;         // OK: any を int に代入
```

**振る舞いルール:**
- `any`は全ての型と単一化(unify)できる
- `any ~ T` の単一化では、結果は `T` になる（anyが相手の型に合わせる）
- `any ~ any` の単一化では、結果は `any` になる
- 演算子の型ルールに従い、`any`が具体的な型に単一化される
  - 例: `x: any` で `x + 1` → `+: (int, int) -> int` により `x ~ int` → 結果は `int`
- 関数の引数・戻り値に`:any`を使用可能
- `any`型は`nil`を含む

**使用例:**
```
// 関数で any を使用
fun identity(x: any) -> any {
    return x;
}

let a = identity(42);      // a は any 型
let b = identity("hello"); // b は any 型
```

#### 型検査ルール
- 代入: 左辺と右辺の型が一致
- 関数呼び出し: 引数の型がパラメータ型と一致
- 演算子: オペランドの型が演算子の期待する型と一致
  - `+`, `-`, `*`, `/`, `%`: `int` 同士または `float` 同士
  - `+`: `string` 同士（連結）
  - `==`, `!=`: 同じ型同士
  - `<`, `<=`, `>`, `>=`: `int` 同士または `float` 同士
  - `&&`, `||`, `!`: `bool`
- 配列: 全要素が同一型
- オブジェクト: フィールド名と型が完全一致

#### nil安全性
- `T` と `T?` は異なる型
- `T?` から `T` への暗黙変換は不可
- nil チェック後のナローイング（今回は実装しない、将来課題）

### 技術的制約
- 既存の動的型付けコードとの互換性は考慮しない（全コード型付け必須）
- 型情報はResolvedIRに付与（バイトコードには型情報なし）
- 循環参照の型推論はエラー

### エラーメッセージ形式
```
error[E001]: type mismatch
  --> file.mc:10:5
   |
10 |     let x: int = "hello";
   |                  ^^^^^^^ expected `int`, found `string`
```

## 8. Open Questions

なし（全て決定済み）

## 9. Acceptance Criteria

1. `let x = 1;` と書いたとき、`x` の型が `int` と推論される
2. `let x: int = "hello";` と書いたとき、型エラーになる
3. `fun f(a, b) { a + b }` を `f(1, 2)` と呼んだとき、`a`, `b`, 戻り値が `int` と推論される
4. `fun f(a, b) { a + b }` を `f(1, "x")` と呼んだとき、型エラーになる
5. `let x: string? = nil;` が型エラーにならない
6. `let x: string = nil;` が型エラーになる
7. `{x: 1, y: "a"}` の型が `{x: int, y: string}` になる
8. `[1, 2, 3]` の型が `array<int>` になる
9. `[1, "a"]` が型エラーになる（要素型不一致）
10. `moca check file.mc` で型チェックのみ実行できる

## 10. Test Plan

### E2E Scenario 1: 型推論の基本動作
```
Given: 型注釈なしの算術関数
  fun add(a, b) { a + b }
  let result = add(1, 2);

When: moca check を実行

Then: エラーなしで終了
  - a, b は int と推論
  - result は int と推論
```

### E2E Scenario 2: 型エラーの検出
```
Given: 型不一致のコード
  fun greet(name: string) -> string {
    "Hello, " + name
  }
  greet(123);

When: moca check を実行

Then: コンパイルエラー
  - エラー位置: greet(123) の 123
  - エラー内容: expected `string`, found `int`
```

### E2E Scenario 3: nullable型の動作
```
Given: nullable型を使ったコード
  let x: int? = nil;
  let y: int = x;  // エラー: int? を int に代入

When: moca check を実行

Then: コンパイルエラー
  - エラー内容: expected `int`, found `int?`
```
