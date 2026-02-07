# Spec.md — 関数インライン化（フェーズ1: `@inline` アノテーション）

## 1. Goal
- `@inline` アノテーション付き関数の呼び出しを、コンパイル時にバイトコードレベルで展開し、関数呼び出しのオーバーヘッド（Frame作成・`Op::Call`・`Op::Ret`）を除去する

## 2. Non-Goals
- 自動インライン化（ヒューリスティック判定による暗黙のインライン化）
- JITレベルのインライン化（プロファイルガイド付き）
- `@noinline` 等の抑制属性
- トランジティブ（多段）インライン化（`@inline` 関数が別の `@inline` 関数を呼ぶ場合、内側は展開しない）
- `@inline` 以外の属性の意味論的処理（パース基盤は汎用的に作るが、`@inline` 以外は無視する）

## 3. Target Users
- moca言語のユーザーが、パフォーマンスクリティカルな小さい関数にインライン化を指示するために使用する

## 4. Core User Flow
1. ユーザーが関数定義の前に `@inline` を記述する
2. コンパイラがその関数への呼び出し箇所で、`Op::Call` の代わりに関数本体のバイトコードを直接展開する
3. 実行時には関数呼び出しのオーバーヘッドなく処理が実行される

```moca
@inline
fn add(x: int, y: int) -> int {
  x + y
}

fn main() {
  let r = add(1, 2)
  println(r)  // 3
}
```

## 5. Inputs & Outputs
- **入力**: `@inline` アノテーション付き関数定義 + その呼び出し
- **出力**: 呼び出し箇所に関数本体が展開されたバイトコード（`Op::Call` が消える）

## 6. Tech Stack
- 言語: Rust（既存プロジェクト）
- テスト: `cargo test`（スナップショットテスト）

## 7. Rules & Constraints

### 7.1 汎用属性パース基盤
- Lexer に `At`（`@`）トークンを追加する
- AST に `Attribute { name: String, span: Span }` を追加する
- `FnDef` に `attributes: Vec<Attribute>` フィールドを追加する
- パーサーは `@name` 形式の属性を `fn` の前にゼロ個以上パースする
- impl ブロック内のメソッドにも属性を付与できる

### 7.2 インライン展開の仕組み（Codegen時）
- `ResolvedFunction` に `is_inline: bool` フィールドを追加し、Resolver が属性情報から設定する
- Codegen の `compile()` メソッドで、全 `ResolvedFunction` への参照を保持しておく
- Codegen が `ResolvedExpr::Call { func_index, args }` をコンパイルする際、対象関数が `is_inline` であれば：
  1. 引数の式をコンパイルしてスタックに積む
  2. インライン先関数のローカル変数スロットを caller 側に確保する（caller の `locals_count` を増やす）
  3. 引数をローカルスロットにセットする（`Op::LocalSet(param_slot + offset)`、引数はスタック上で逆順なので末尾から）
  4. 関数本体を `local_offset` 付きでコンパイルする（全ての `LocalGet(i)` / `LocalSet(i)` を `LocalGet(i + offset)` / `LocalSet(i + offset)` に変換）
  5. `Return` 文は `Op::Ret` ではなく、インライン末尾へのジャンプ（`Op::Jmp`）に変換する（バックパッチで位置を確定）

### 7.3 エラー条件
- `@inline` 関数が自分自身を直接呼び出している場合（直接再帰）→ **コンパイルエラー**
- 未知の属性名が使われた場合 → 警告（エラーではない。将来の拡張に備える）

### 7.4 対応する呼び出し形式
- 通常の関数呼び出し: `func(args)` → 対応
- メソッド呼び出し: `obj.method(args)` → 対応
- 関連関数呼び出し: `Type::func(args)` → 対応

### 7.5 制約
- トランジティブ展開しない: `@inline fn a()` が `@inline fn b()` を呼ぶ場合、`b()` は通常の `Op::Call` として残る
- インライン展開は1呼び出しにつき1回のみ（同じ関数の複数呼び出し箇所それぞれで展開される）

## 8. Acceptance Criteria

1. `@inline` 付き関数の呼び出しが、生成バイトコードで `Op::Call` ではなく本体展開されている
2. インライン化された関数の実行結果が、インライン化しない場合と同一である
3. `@inline` 付き再帰関数はコンパイルエラーになる
4. メソッド呼び出し（`obj.method()`）でもインライン展開が動作する
5. `@inline` なし関数は従来通り `Op::Call` で呼び出される（リグレッションなし）
6. 引数が正しくローカル変数にマッピングされ、複数引数でも正しく動作する
7. インライン関数内の `return` 文が正しく動作する（caller からの脱出ではなく、インライン末尾に飛ぶ）
8. `cargo fmt && cargo check && cargo test && cargo clippy` が全てパスする
9. 汎用属性パースが動作し、未知の属性でもパースエラーにならない

## 9. Verification Strategy
- **進捗検証**: 各タスク完了後に `cargo check && cargo test` を実行し、既存テストが壊れていないことを確認
- **達成検証**: 新規スナップショットテスト（`.mc` ファイル）でインライン化の正しい動作を確認
- **漏れ検出**: Acceptance Criteria を全てテストでカバーする

## 10. Test Plan

### Test 1: 基本的なインライン展開
```
Given: @inline fn add(x: int, y: int) -> int { x + y } と add(1, 2) の呼び出し
When: コンパイル・実行する
Then: 結果が 3 である
```

### Test 2: インライン関数内の return 文
```
Given: @inline fn abs(x: int) -> int { if x < 0 { return -x } x } と abs(-5) の呼び出し
When: コンパイル・実行する
Then: 結果が 5 である（return が正しくインライン末尾にジャンプ）
```

### Test 3: 再帰関数のコンパイルエラー
```
Given: @inline fn fact(n: int) -> int { if n <= 1 { return 1 } n * fact(n - 1) } の定義
When: コンパイルする
Then: 直接再帰のためコンパイルエラーが発生する
```
