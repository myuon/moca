# Spec.md — Vector [] 構文サポート

## 1. Goal
- Vectorに対して `vec[i]` / `vec[i] = value` の配列構文でアクセスできるようにする
- 型情報を使ってVectorと固定配列のコード生成を切り替える

## 2. Non-Goals
- 新しい型システムの設計（既存のtypecheckerを活用）
- `vec_get`/`vec_set`の削除（後方互換性のため残す）

## 3. Target Users
- Moca言語のユーザー（配列と同じ構文でVectorを扱いたい）

## 4. Core User Flow
1. ユーザーが `vec[i]` または `vec[i] = value` を含むコードを書く
2. typecheckerが型を推論し、ASTノードの`inferred_type`フィールドに型を設定
3. codegenがASTの型情報を参照し、Vector/配列で異なるコードを生成
4. VMが適切なオペコードを実行

## 5. Inputs & Outputs

**入力:**
```moca
var vec = vec_new();
vec_push(vec, 10);
print(vec[0]);      // Vector読み取り
vec[0] = 20;        // Vector書き込み
```

**出力（生成コード）:**
- Vector読み取り: `HeapLoad(0)` → `HeapLoadDyn`（ptrを取得してからアクセス）
- Vector書き込み: `HeapLoad(0)` → index → value → `HeapStoreDyn`
- 固定配列: 従来通り直接 `HeapLoadDyn`/`HeapStoreDyn`

## 6. Tech Stack
- 言語: Rust（既存プロジェクト）
- 変更対象:
  - `src/compiler/ast.rs` — Exprに`inferred_type`フィールド追加
  - `src/compiler/typechecker.rs` — 型推論結果をASTに設定
  - `src/compiler/codegen.rs` — ASTの型情報を参照してコード生成分岐

## 7. Rules & Constraints

### 型情報の管理
- `Expr`に`inferred_type: Option<Type>`フィールドを追加
- typecheckerが型推論時にASTノードの`inferred_type`を設定
- codegenが`inferred_type`を参照してコード生成を切り替え

### コード生成ルール
- `expr[index]` の `expr` の型が `Vector<T>` → Vector用コード生成
- `expr[index]` の `expr` の型が `array<T>` → 配列用コード生成
- 型が不明（未解決の型変数など）→ コンパイルエラー

### Vector用コード生成（読み取り: `vec[i]`）
```
1. vecをスタックにpush
2. HeapLoad(0)  // data ptrを取得
3. indexをスタックにpush
4. HeapLoadDyn  // ptr[index]を読み取り
```

### Vector用コード生成（書き込み: `vec[i] = value`）
```
1. vecをスタックにpush
2. HeapLoad(0)  // data ptrを取得
3. indexをスタックにpush
4. valueをスタックにpush
5. HeapStoreDyn // ptr[index] = value
```

### エラーハンドリング
- 型推論失敗時: コンパイルエラー
- 境界外アクセス: 実行時エラー（現状維持）

## 8. Open Questions
なし

## 9. Acceptance Criteria

1. [ ] `vec[i]` でVectorの要素を読み取れる
2. [ ] `vec[i] = value` でVectorの要素を書き込める
3. [ ] 固定配列 `arr[i]` は従来通り動作する
4. [ ] 型推論できない場合はコンパイルエラーになる
5. [ ] `vec_get`/`vec_set` は引き続き動作する
6. [ ] 境界外アクセスは実行時エラーになる
7. [ ] `cargo test` が全て通る

## 10. Verification Strategy

- **進捗検証**: 各フェーズ完了時に `cargo build` が通ることを確認
- **達成検証**: 以下のコードが期待通り動作することを確認
  ```moca
  var vec = vec_new();
  vec_push(vec, 10);
  vec_push(vec, 20);
  print(vec[0]);      // => 10
  vec[1] = 25;
  print(vec[1]);      // => 25
  ```
- **漏れ検出**: `cargo test` で既存テスト + 新規スナップショットテストが通ること

## 11. Test Plan

### Test 1: Vector読み取り
- **Given**: `vec_new()` でVectorを作成し、値をpush
- **When**: `vec[0]` でアクセス
- **Then**: pushした値が返る

### Test 2: Vector書き込み
- **Given**: 値が入ったVector
- **When**: `vec[0] = 99` で書き込み
- **Then**: `vec[0]` が99を返す

### Test 3: 固定配列との共存
- **Given**: Vector `vec` と配列 `arr` の両方を定義
- **When**: 両方に対して `[i]` でアクセス
- **Then**: それぞれ正しい値が返る
