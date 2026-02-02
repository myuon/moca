# Spec: Collection Literals (type構文)

## 1. Goal
- `type 型名 {式, ...}` 構文でarray/Vec/Mapの初期化を統一的に記述できるようにする

## 2. Non-Goals
- 型引数の推論（型引数は必須）
- ネストしたリテラルの特別な最適化
- 既存の配列リテラル `[1, 2, 3]` の即時廃止（当面は両方サポート）

## 3. Target Users
- Moca言語の利用者（言語開発者自身を含む）

## 4. Core User Flow
1. ユーザーが `type Vec<int> {1, 2, 3}` を記述
2. パーサーが `type` キーワードを認識し、TypeLiteralとしてAST生成
3. Desugarフェーズで以下に展開:
   ```
   {
     var __tmp = Vec<int>.uninit(3);
     __tmp.set(0, 1);
     __tmp.set(1, 2);
     __tmp.set(2, 3);
     __tmp
   }
   ```
4. 型チェック・コード生成は通常通り処理

## 5. Inputs & Outputs

### 入力（構文）
```
type 型名 { 初期化式, 初期化式, ... }
```

初期化式は2種類:
| 形式 | 用途 | 例 |
|------|------|-----|
| `expr` | array/Vec用 | `type Vec<int> {1, 2, 3}` |
| `key: value` | Map用 | `type Map<string, int> {"a": 1, "b": 2}` |

### 出力（desugar後）

**array/Vec用** (`expr` 形式):
```
// type array<int> {1, 2, 3}
{
  var __tmp = array<int>.uninit(3);
  __tmp.set(0, 1);
  __tmp.set(1, 2);
  __tmp.set(2, 3);
  __tmp
}

// type Vec<int> {1, 2, 3}
{
  var __tmp = Vec<int>.uninit(3);
  __tmp.set(0, 1);
  __tmp.set(1, 2);
  __tmp.set(2, 3);
  __tmp
}
```

**Map用** (`key: value` 形式):
```
// type Map<string, int> {"a": 1, "b": 2}
{
  var __tmp = Map<string, int>.uninit(2);
  __tmp.insert("a", 1);
  __tmp.insert("b", 2);
  __tmp
}
```

## 6. Tech Stack
- 言語: Rust（既存のMocaコンパイラ）
- 変更対象:
  - `src/compiler/lexer.rs` - `type` キーワードのトークン追加
  - `src/compiler/parser.rs` - type構文のパース
  - `src/compiler/ast.rs` - `TypeLiteral` Expr variant追加
  - `src/compiler/resolver.rs` - 構文糖衣の展開（desugar）
  - `std/prelude.mc` - `uninit`, `insert` メソッド追加
- テスト: 既存の `cargo test` + 新規 `.mc` テストファイル

## 7. Rules & Constraints

### 構文ルール
1. `type` キーワードの後に型名（型引数含む）、その後に `{...}`
2. 型引数は必須（推論しない）
3. 空リテラル `type Vec<int> {}` は許可（`uninit(0)` と同等）
4. 初期化式が全て `expr` 形式なら `.set(index, value)` にdesugar
5. 初期化式が全て `key: value` 形式なら `.insert(key, value)` にdesugar
6. 混在は禁止（コンパイルエラー）

### 対象型の要件
`type` 構文を使うには、型が以下のメソッドを持つ必要がある:
- **expr形式の場合**: `uninit(capacity: int)` と `set(index: int, value: T)`
- **key:value形式の場合**: `uninit(capacity: int)` と `insert(key: K, value: V)`

### stdlib追加メソッド
| 型 | 追加メソッド | 説明 |
|----|-------------|------|
| `array<T>` | `uninit(cap: int)` | 指定サイズの未初期化配列を作成 |
| `array<T>` | `set(idx: int, val: T)` | 要素を設定（既存の `[]` 代入と同等） |
| `Vec<T>` | `uninit(cap: int)` | 指定サイズ・長さのVecを作成 |
| `Map<K,V>` | `uninit(cap: int)` | 指定キャパシティのMapを作成 |
| `Map<K,V>` | `insert(key: K, val: V)` | キー・値ペアを挿入（`put`のエイリアス） |

### 技術的制約
1. Desugarは型チェック前に行う（resolver段階）
2. 一時変数名は既存の変数と衝突しない名前を使用（例: `__type_lit_0`）
3. 既存の配列リテラル `[1, 2, 3]` は当面サポート継続

## 8. Open Questions
なし

## 9. Acceptance Criteria
1. `type array<int> {1, 2, 3}` をパースしてコンパイル・実行できる
2. `type Vec<int> {1, 2, 3}` をパースしてコンパイル・実行できる
3. `type Map<string, int> {"a": 1, "b": 2}` をパースしてコンパイル・実行できる
4. 空リテラル `type Vec<int> {}` が動作する
5. 生成したVecに対して `.len()` で正しい長さが返る
6. 生成したMapに対して `.get()` で正しい値が返る
7. ユーザー定義型で `uninit` と `set` を持つ型でも動作する
8. 既存の配列リテラル `[1, 2, 3]` が引き続き動作する
9. 既存の構造体リテラル `Point { x: 1, y: 2 }` が引き続き動作する
10. `cargo test` が全てパスする

## 10. Verification Strategy

### 進捗検証
- 各フェーズ完了時に該当する `.mc` テストファイルを実行
- パーサー変更後: ASTダンプで `TypeLiteral` ノードが生成されることを確認
- Desugar後: 展開されたコードが期待通りか確認

### 達成検証
- 全Acceptance Criteriaをテストコードで網羅
- `cargo test` が全てパス
- 手動で `examples/type_literals.mc` を実行して動作確認

### 漏れ検出
- 既存テストが全てパスすることで後方互換性を確認
- エッジケース（空リテラル、ネスト）のテストを追加

## 11. Test Plan

### E2E シナリオ 1: array/Vec リテラルの基本動作
**Given**: 以下のコードを含む `.mc` ファイル
```
let arr = type array<int> {1, 2, 3};
assert_eq(arr[0], 1, "array[0] should be 1");
assert_eq(arr[2], 3, "array[2] should be 3");

let v = type Vec<int> {10, 20, 30};
assert_eq(v.len(), 3, "vec length should be 3");
assert_eq(v.get(1), 20, "vec[1] should be 20");
```
**When**: `moca run` で実行
**Then**: アサーションが全て成功し、正常終了

### E2E シナリオ 2: Map リテラルの基本動作
**Given**: 以下のコードを含む `.mc` ファイル
```
let m = type Map<string, int> {"foo": 10, "bar": 20};
assert_eq(m.len(), 2, "length should be 2");
assert_eq(m.get("foo"), 10, "foo should be 10");
assert_eq(m.get("bar"), 20, "bar should be 20");
```
**When**: `moca run` で実行
**Then**: アサーションが全て成功し、正常終了

### E2E シナリオ 3: 既存構文との共存
**Given**: 以下のコードを含む `.mc` ファイル
```
// 既存の配列リテラル（当面サポート）
let arr = [1, 2, 3];
assert_eq(arr[0], 1, "legacy array literal works");

// 既存の構造体リテラル
struct Point { x: int, y: int }
let p = Point { x: 10, y: 20 };
assert_eq(p.x, 10, "struct literal works");

// 新しいtype構文
let v = type Vec<int> {100, 200};
assert_eq(v.get(0), 100, "type literal works");
```
**When**: `moca run` で実行
**Then**: アサーションが全て成功し、正常終了
