# Spec: Collection Literals

## 1. Goal
- `Vec<T>[e1, e2, ...]` や `Map<K,V>{k1: v1, k2: v2}` の構文糖衣を提供し、コレクションの初期化を簡潔に書けるようにする

## 2. Non-Goals
- ネストしたリテラル（例: `Vec[Vec[1, 2], Vec[3, 4]]`）の特別なサポート（通常の構文として動作すればOK）
- イテレータからの生成（`Vec.from_iter(...)`）
- 配列リテラル `[1, 2, 3]` から Vec への暗黙変換
- `insert` メソッドの新設（既存の `put` を使用）

## 3. Target Users
- Moca言語の利用者（言語開発者自身を含む）

## 4. Core User Flow
1. ユーザーがVecリテラル `Vec<int>[1, 2, 3]` を記述
2. パーサーがこれを認識し、AST上で `CollectionLiteral` として表現
3. Desugarフェーズ（またはパーサー内）で以下に展開:
   ```
   {
     var __tmp = Vec<int>.new();
     __tmp.push(1);
     __tmp.push(2);
     __tmp.push(3);
     __tmp
   }
   ```
4. 型チェック・コード生成は通常通り処理

## 5. Inputs & Outputs

### 入力（構文）
| 構文 | 説明 |
|------|------|
| `Type[e1, e2, ...]` | Vecライクなリテラル。`Type.new()` + `.push(ei)` にdesugar |
| `Type{k1: v1, k2: v2}` | Mapライクなリテラル。`Type.new()` + `.put(ki, vi)` にdesugar |

### 出力（desugar後）
```
// Vec<int>[1, 2, 3]
{
  var __tmp = Vec<int>.new();
  __tmp.push(1);
  __tmp.push(2);
  __tmp.push(3);
  __tmp
}

// Map<string, int>{"a": 1, "b": 2}
{
  var __tmp = Map<string, int>.new();
  __tmp.put("a", 1);
  __tmp.put("b", 2);
  __tmp
}
```

## 6. Tech Stack
- 言語: Rust（既存のMocaコンパイラ）
- 変更対象:
  - `src/compiler/lexer.rs` - 必要に応じてトークン追加
  - `src/compiler/parser.rs` - 新構文のパース
  - `src/compiler/ast.rs` - 新しいExpr variant追加
  - `src/compiler/resolver.rs` または新規desugarフェーズ - 構文糖衣の展開
  - `src/compiler/typechecker.rs` - 型チェック対応（desugar後なら変更不要の可能性）
- テスト: 既存の `cargo test` + 新規 `.mc` テストファイル

## 7. Rules & Constraints

### 構文ルール
1. **Vecリテラル**: `TypeName[e1, e2, ...]` または `TypeName<T>[e1, e2, ...]`
   - `[` の直前に型名がある場合にのみリテラルとして認識
   - 単独の `[1, 2, 3]` は既存の配列リテラルのまま
2. **Mapリテラル**: `TypeName{k1: v1, ...}` または `TypeName<K,V>{k1: v1, ...}`
   - `{` の直前に型名がある場合にリテラルとして認識
   - 既存の構造体リテラル `Point { x: 1, y: 2 }` との区別: キーが識別子でなく式の場合はMapリテラル
3. **空リテラル**: `Vec<int>[]` や `Map<string, int>{}` は許可（単に `.new()` と同等）
4. **対象型**: `push` / `put` メソッドを持つ任意の型で使用可能（Vec/Map専用ではない）

### 技術的制約
1. Desugarは型チェック前に行う（resolver段階が望ましい）
2. 一時変数名は既存の変数と衝突しない名前を使用（例: `__collection_tmp_0`）
3. 型引数が省略された場合、要素から推論する（仮）

### 構文の曖昧性回避
- `Type { field: value }` （構造体リテラル）vs `Type { key: value }` （Mapリテラル）
  - **判別方法**: キーが既知のフィールド名なら構造体リテラル、そうでなければMapリテラル（仮）
  - または: Mapリテラルは `Map` または `map` 型名の場合のみ有効とする（仮）

## 8. Open Questions
1. **構造体リテラルとの曖昧性**: `MyType{a: 1}` が構造体かMapか。現時点では「Map/map型のみMapリテラル」で仮決め
2. **型推論**: `Vec[1, 2, 3]` で `Vec<int>` と推論するか、型引数必須か。現時点では「推論する」で仮決め

## 9. Acceptance Criteria
1. `Vec<int>[1, 2, 3]` をパースしてコンパイル・実行できる
2. `Map<string, int>{"a": 1, "b": 2}` をパースしてコンパイル・実行できる
3. 空リテラル `Vec<int>[]` と `Map<string, int>{}` が動作する
4. Vecリテラルで生成したVecに対して `.len()` で正しい長さが返る
5. Mapリテラルで生成したMapに対して `.get()` で正しい値が返る
6. 型引数を省略した `Vec[1, 2, 3]` が `Vec<int>` として推論される（仮）
7. ユーザー定義型で `new()` と `push()` を持つ型でもVecリテラル構文が使える
8. 既存の配列リテラル `[1, 2, 3]` が引き続き動作する
9. 既存の構造体リテラル `Point { x: 1, y: 2 }` が引き続き動作する
10. `cargo test` が全てパスする

## 10. Verification Strategy

### 進捗検証
- 各フェーズ完了時に該当する `.mc` テストファイルを実行
- パーサー変更後: ASTダンプで新しいノードが生成されることを確認
- Desugar後: 展開されたコードが期待通りか確認（デバッグ出力）

### 達成検証
- 全Acceptance Criteriaをテストコードで網羅
- `cargo test` が全てパス
- 手動で `examples/collection_literals.mc` を実行して動作確認

### 漏れ検出
- 既存テストが全てパスすることで後方互換性を確認
- エッジケース（空リテラル、ネスト、型推論）のテストを追加

## 11. Test Plan

### E2E シナリオ 1: Vec リテラルの基本動作
**Given**: 以下のコードを含む `.mc` ファイル
```
let v = Vec<int>[1, 2, 3];
assert_eq(v.len(), 3, "length should be 3");
assert_eq(v.get(0), 1, "first element should be 1");
assert_eq(v.get(2), 3, "last element should be 3");
```
**When**: `moca run` で実行
**Then**: アサーションが全て成功し、正常終了

### E2E シナリオ 2: Map リテラルの基本動作
**Given**: 以下のコードを含む `.mc` ファイル
```
let m = Map<string, int>{"foo": 10, "bar": 20};
assert_eq(m.len(), 2, "length should be 2");
assert_eq(m.get("foo"), 10, "foo should be 10");
assert_eq(m.get("bar"), 20, "bar should be 20");
```
**When**: `moca run` で実行
**Then**: アサーションが全て成功し、正常終了

### E2E シナリオ 3: 既存構文との共存
**Given**: 以下のコードを含む `.mc` ファイル
```
// 既存の配列リテラル
let arr = [1, 2, 3];
assert_eq(arr[0], 1, "array literal works");

// 既存の構造体リテラル
struct Point { x: int, y: int }
let p = Point { x: 10, y: 20 };
assert_eq(p.x, 10, "struct literal works");

// 新しいVecリテラル
let v = Vec<int>[100, 200];
assert_eq(v.get(0), 100, "vec literal works");
```
**When**: `moca run` で実行
**Then**: アサーションが全て成功し、正常終了
