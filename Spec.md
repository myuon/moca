# Spec.md - Index/IndexAssign の Desugar 実装

## 1. Goal
- `vec[i]`や`vec[i] = value`などのindex構文をdesugarフェーズで`get`/`set`メソッド呼び出しに変換し、Vec/Mapなどの型で統一的にインデックスアクセスを実現する

## 2. Non-Goals
- `Array<T>`のindex/index assignは対象外（従来のcodegen実装を維持）
- `get`/`set`メソッドの内部実装の最適化
- 範囲外アクセス時のエラーハンドリング（メソッド側の責務）

## 3. Target Users
- mocaコンパイラの開発者
- moca言語でVec/Mapを使うユーザー

## 4. Core User Flow
1. ユーザーが`vec[i]`や`vec[i] = value`を含むコードを書く
2. パーサーが`Expr::Index`/`Statement::IndexAssign`としてASTを生成
3. typecheckerが型情報を付与
4. desugarフェーズで型に応じてメソッド呼び出しに変換
   - `Vec<T>`, `Map<K,V>`: `x.get(i)` / `x.set(i, v)`に変換
   - `Array<T>`: 変換しない
5. codegenが変換後のASTをコンパイル

## 5. Inputs & Outputs
### Inputs
- `Expr::Index { object, index, object_type }` （型情報付き）
- `Statement::IndexAssign { object, index, value, object_type }` （型情報付き）

### Outputs
- `Expr::MethodCall { object, method: "get", args: [index] }`
- `Statement::Expr(Expr::MethodCall { object, method: "set", args: [index, value] })`

## 6. Tech Stack
- 言語: Rust
- テストフレームワーク: cargo test（スナップショットテスト）
- 既存コンパイラパイプライン: parser → typechecker → desugar → codegen

## 7. Rules & Constraints
- desugarフェーズはtypechecker後に実行する（型情報が必要なため）
- `Vec<T>`と`Map<K,V>`のみdesugar対象、`Array<T>`は除外
- `get`/`set`メソッドが未定義の場合はstdlibで定義する
- 既存のcodegenにある`IndexAssign`の`Vec<T>`/`Map<K,V>`向け処理は削除可能

## 8. Open Questions
- なし

## 9. Acceptance Criteria
1. `vec[i]`が`vec.get(i)`にdesugarされる
2. `vec[i] = value`が`vec.set(i, value)`にdesugarされる
3. `map[key]`が`map.get(key)`にdesugarされる
4. `map[key] = value`が`map.set(key, value)`にdesugarされる
5. `Array<T>`のindex/index assignはdesugarされない（従来動作を維持）
6. desugarフェーズがtypechecker後に実行される
7. `Vec<T>`に`get`/`set`メソッドが存在する
8. `Map<K,V>`に`get`/`set`メソッドが存在する
9. 以下のコードが正しく動作する:
   ```
   var v = new Vec<int> { 1, 2, 3 };
   v[0] = 10;
   print(v[0]);  // 10
   ```
10. 既存テストがすべてパスする

## 10. Verification Strategy
- **進捗検証**: 各タスク完了時に`cargo test`を実行し、既存テストが壊れていないことを確認
- **達成検証**: Acceptance Criteriaの9番目のコードを実行して期待通りの出力を得る
- **漏れ検出**: `cargo test`でスナップショットテストを確認、手動でVec/Map/Arrayの各パターンを試す

## 11. Test Plan

### E2E Scenario 1: Vec index access
- **Given**: `var v = new Vec<int> { 1, 2, 3 };`
- **When**: `print(v[1]);`を実行
- **Then**: `2`が出力される

### E2E Scenario 2: Vec index assign
- **Given**: `var v = new Vec<int> { 1, 2, 3 };`
- **When**: `v[0] = 99; print(v[0]);`を実行
- **Then**: `99`が出力される

### E2E Scenario 3: Array unchanged behavior
- **Given**: `var arr: Array<int, 3> = [1, 2, 3];`
- **When**: `arr[0] = 10; print(arr[0]);`を実行
- **Then**: `10`が出力される（従来のcodegen経由で動作）
