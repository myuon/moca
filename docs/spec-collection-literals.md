# Spec: Collection Literals (new構文)

## 1. Goal
- `new 型名 {式, ...}` 構文でVec/Mapの初期化を統一的に記述できるようにする
- 予約語エスケープ機構（バッククォート）を導入し、`new` などの予約語を識別子として使用可能にする

## 2. Non-Goals
- 型引数の推論（型引数は必須）
- ネストしたリテラルの特別な最適化
- VM最適化（`VecLiteral`/`MapLiteral` オプコードの `uninit` + `set` への置き換え）は今後の課題

## 3. Target Users
- Moca言語の利用者（言語開発者自身を含む）

## 4. Core User Flow
1. ユーザーが `new Vec<int> {1, 2, 3}` を記述
2. パーサーが `new` キーワードを認識し、NewLiteralとしてAST生成
3. 型チェック・コード生成でVecLiteral/MapLiteralオペコードを出力
4. 既存の `new` という名前の関数は `` `new` `` と記述することで使用可能

## 5. Inputs & Outputs

### 入力（構文）
```
new 型名<型引数> { 初期化式, 初期化式, ... }
```

初期化式は2種類:
| 形式 | 用途 | 例 |
|------|------|-----|
| `expr` | Vec用 | `new Vec<int> {1, 2, 3}` |
| `key: value` | Map用 | `new Map<string, int> {"a": 1, "b": 2}` |

### 予約語エスケープ
```mc
// `new` という名前の関数を定義（バッククォートでエスケープ）
fun `new`() -> int { return 42; }
let x = `new`();

// 他の予約語も同様にエスケープ可能
fun `let`(s: string) { print(s); }
let `if` = 100;
```

### 出力（コンパイル後）
- Vec: `VecLiteral` オペコードで要素を含むベクターを生成
- Map: `MapLiteral` オペコードでキー・値ペアを含むマップを生成

## 6. Tech Stack
- 言語: Rust（既存のMocaコンパイラ）
- 変更対象:
  - `src/compiler/lexer.rs` - `new` キーワードのトークン追加、バッククォートエスケープ対応
  - `src/compiler/parser.rs` - new構文のパース
  - `src/compiler/ast.rs` - `NewLiteral` Expr variant追加
  - `src/compiler/resolver.rs` - NewLiteralの解決
  - `src/compiler/codegen.rs` - VecLiteral/MapLiteralオペコード生成
  - `std/prelude.mc` - `new` 関数を `` `new` `` にエスケープ
- テスト: 既存の `cargo test` + 新規 `.mc` テストファイル

## 7. Rules & Constraints

### 構文ルール
1. `new` キーワードの後に型名（型引数含む）、その後に `{...}`
2. 型引数は必須（推論しない）
3. 空リテラル `new Vec<int> {}` は許可
4. 初期化式が全て `expr` 形式なら Vec として扱う
5. 初期化式が全て `key: value` 形式なら Map として扱う
6. 混在は禁止（コンパイルエラー）

### 予約語エスケープルール
1. バッククォートで囲むことで任意の予約語を識別子として使用可能: `` `keyword` ``
2. 対象: すべての予約語（`new`, `let`, `fun`, `if`, `while`, etc.）
3. エスケープされた識別子は通常の識別子と同等に扱う
4. エスケープ内では英数字とアンダースコアのみ使用可能
5. 改行を含むことはできない

### サポート型
| 型 | リテラル形式 | 例 |
|----|-------------|-----|
| `Vec<T>` | `expr, ...` | `new Vec<int> {1, 2, 3}` |
| `Map<K,V>` | `key: value, ...` | `new Map<string, int> {"a": 1}` |

### 破壊的変更
- 旧構文 `type 型名 {...}` は削除済み（パースエラーになる）
- 標準ライブラリの `new` 関数は `` `new` `` にリネーム済み

## 8. Open Questions
なし

## 9. Acceptance Criteria
1. `new Vec<int> {1, 2, 3}` がパースできる
2. `new Map<string, int> {"a": 1}` がパースできる
3. `new Vec<int> {}` （空リテラル）がパースできる
4. `new` が予約語として認識される
5. `` `new` `` でエスケープした識別子が関数名/変数名として使用できる
6. `` `let` ``, `` `if` `` など他の予約語もエスケープ可能
7. 旧構文 `type Vec<int> {...}` がパースエラーになる
8. `new Vec<int> {1, 2, 3}` で作成したVecの `.len()` が `3` を返す
9. `new Map<string, int> {"a": 1}` で作成したMapの `.get_string("a")` が `1` を返す
10. 全既存テストが新構文で通過する

## 10. Verification Strategy

### 進捗検証
- 各フェーズ完了時に該当する `.mc` テストファイルを実行
- パーサー変更後: ASTダンプで `NewLiteral` ノードが生成されることを確認

### 達成検証
- 全Acceptance Criteriaをテストコードで網羅
- `cargo test` が全てパス

### 漏れ検出
- 既存テストが全てパスすることで後方互換性を確認
- エッジケース（空リテラル、ネスト）のテストを追加

## 11. Test Plan

### E2E シナリオ 1: Vec リテラルの基本動作
**Given**: 以下のコードを含む `.mc` ファイル
```mc
let v: Vec<int> = new Vec<int> {1, 2, 3};
print(v.len());
print(v.get(0));
print(v.get(1));
print(v.get(2));
```
**When**: `moca run` で実行
**Then**: 出力が `3`, `1`, `2`, `3` になる

### E2E シナリオ 2: Map リテラルの基本動作
**Given**: 以下のコードを含む `.mc` ファイル
```mc
let m: Map<string, int> = new Map<string, int> {"a": 10, "b": 20};
print(m.len());
print(m.get_string("a"));
print(m.get_string("b"));
```
**When**: `moca run` で実行
**Then**: 出力が `2`, `10`, `20` になる

### E2E シナリオ 3: 予約語エスケープ
**Given**: 以下のコードを含む `.mc` ファイル
```mc
fun `new`() -> int { return 42; }
print(`new`());

fun `let`(s: string) { print(s); }
`let`("hello");

let `if` = 100;
print(`if`);
```
**When**: `moca run` で実行
**Then**: 出力が `42`, `hello`, `100` になる

### E2E シナリオ 4: 既存構文との共存
**Given**: 以下のコードを含む `.mc` ファイル
```mc
// 既存の配列リテラル
let arr = [1, 2, 3];
print(arr[0]);

// 既存の構造体リテラル
struct Point { x: int, y: int }
let p = Point { x: 10, y: 20 };
print(p.x);

// 新しいnew構文
let v: Vec<int> = new Vec<int> {100, 200};
print(v.get(0));
```
**When**: `moca run` で実行
**Then**: 出力が `1`, `10`, `100` になる
