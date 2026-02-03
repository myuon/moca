# Spec.md - new構文のDesugar実装

## 1. Goal
- `new Vec<T> {...}` / `new Map<K,V> {...}` 構文を、通常のメソッド呼び出しとインデックス代入に展開（desugar）し、専用オペコードを不要にする

## 2. Non-Goals
- 新しい構文の追加（既存のnew構文をそのまま使う）
- パフォーマンス最適化（単純な展開のみ）
- 他の構文糖衣のdesugar（今回はnew構文のみ）

## 3. Target Users
- mocaコンパイラ開発者
- moca言語ユーザー（変更は透過的、振る舞いは同じ）

## 4. Core User Flow
1. ユーザーが `new Vec<int> {1, 2, 3}` を記述
2. Parser が `Expr::NewLiteral` としてパース
3. TypeChecker が型検査（従来通り）
4. **Desugar** が以下に展開:
   ```
   {
     let __tmp = Vec<int>::uninit(3);
     __tmp[0] = 1;
     __tmp[1] = 2;
     __tmp[2] = 3;
     __tmp
   }
   ```
5. Monomorphisation、Resolver、Codegen は展開後のASTを処理
6. 実行結果は従来と同じ

## 5. Inputs & Outputs

### Inputs
- `Expr::NewLiteral` ノード（型チェック済み）
  - Vec: `new Vec<T> {e1, e2, ...}`
  - Map: `new Map<K,V> {k1: v1, k2: v2, ...}`

### Outputs
- 展開後のAST（ブロック式）

**Vec展開:**
```
{
  let __new_literal_N: Vec<T> = Vec<T>::uninit(要素数);
  __new_literal_N[0] = e1;
  __new_literal_N[1] = e2;
  ...
  __new_literal_N
}
```

**Map展開:**
```
{
  let __new_literal_N: Map<K,V> = Map<K,V>::uninit();
  __new_literal_N.insert(k1, v1);  // または put
  __new_literal_N.insert(k2, v2);
  ...
  __new_literal_N
}
```

## 6. Tech Stack
- 言語: Rust
- 対象: moca コンパイラ (`src/compiler/`)
- テスト: 既存のスナップショットテスト

## 7. Rules & Constraints

### 振る舞いルール
- 一時変数名は `__new_literal_N` 形式（Nはユニークな番号）でユーザー変数と衝突回避
- 空リテラル `new Vec<T> {}` は `uninit(0)` に展開
- 要素の評価順序は左から右（元のセマンティクスを維持）

### 技術的制約
- Desugarフェーズは TypeChecker の後、Monomorphisation の前に配置
- `Expr::NewLiteral` はDesugar後にASTから消える（後続フェーズでは出現しない）
- `VecLiteral` / `MapLiteral` オペコードは削除する

### 新規追加が必要なもの
- `Vec<T>::uninit(capacity: int) -> Vec<T>` メソッド（std/prelude.mc）
- `Map<K,V>::uninit() -> Map<K,V>` メソッド（std/prelude.mc）
- Mapへの要素追加用メソッド（既存の `insert_*` を使うか、汎用 `insert` を追加）

## 8. Open Questions
- (仮) Mapの `insert` メソッド名: 既存の `insert_string` / `insert_int` を使うか、ジェネリックな `insert` を追加するか → 既存メソッドを使う方向で進め、必要なら後で統一

## 9. Acceptance Criteria

1. `new Vec<int> {1, 2, 3}` が `Vec<int>::uninit(3)` + インデックス代入に展開される
2. `new Map<string, int> {"a": 1, "b": 2}` が `Map::uninit()` + insert呼び出しに展開される
3. 空リテラル `new Vec<int> {}` が `Vec<int>::uninit(0)` に展開される
4. 既存のテスト（vec_literal.mc, map_literal.mc等）がすべてパスする
5. `VecLiteral` / `MapLiteral` オペコードがcodegenから削除されている
6. Desugarフェーズが TypeChecker → Desugar → Monomorphisation の順で実行される
7. 型エラーは従来通りユーザーが書いた構文（NewLiteral）に対して報告される
8. ネストした `new` 式（`new Vec<Vec<int>> { new Vec<int> {1} }`）が正しく展開される
9. `Vec<T>::uninit` と `Map<K,V>::uninit` がstd/prelude.mcに追加されている
10. `cargo test` がすべてパスする

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo check` と `cargo test` を実行
- Desugar後のASTをデバッグ出力して展開結果を目視確認（開発時のみ）

### 達成検証
- 全Acceptance Criteriaをチェックリストで確認
- 既存のスナップショットテストがパス
- 新規テストケース追加（ネストしたnew式など）

### 漏れ検出
- `Expr::NewLiteral` がCodegenに到達しないことを確認（到達したらpanic）
- `VecLiteral` / `MapLiteral` オペコードの参照がゼロであることを確認

## 11. Test Plan

### E2E シナリオ 1: Vec リテラルの展開
- **Given**: `let v: Vec<int> = new Vec<int> {1, 2, 3};`
- **When**: コンパイル・実行
- **Then**: `v.len() == 3` かつ `v.get(0) == 1, v.get(1) == 2, v.get(2) == 3`

### E2E シナリオ 2: Map リテラルの展開
- **Given**: `let m: Map<string, int> = new Map<string, int> {"x": 10};`
- **When**: コンパイル・実行
- **Then**: `m.get_string("x") == 10`

### E2E シナリオ 3: ネストしたリテラル
- **Given**: `let nested: Vec<Vec<int>> = new Vec<Vec<int>> { new Vec<int> {1, 2} };`
- **When**: コンパイル・実行
- **Then**: `nested.get(0).get(0) == 1` かつ `nested.get(0).get(1) == 2`
