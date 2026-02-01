# Spec.md - vec<T>/map<K,V> ジェネリクス化

## 1. Goal
- `vec<T>` と `map<K,V>` を完全なジェネリクス構造体として実装し、`VectorAny`/`HashMapAny`（any型ベース）を廃止する
- 型安全なコレクション操作を実現し、コンパイル時に型エラーを検出できるようにする

## 2. Non-Goals
- イテレータ / for-in ループのジェネリクス対応
- 追加のコレクション型（Set, Deque等）
- Hash trait / Eq trait などの trait システム
- mapのキー型拡張（int/string以外）

## 3. Target Users
- mocaプログラマー：型安全なコレクションを使用したい開発者
- moca言語開発者：型システムの一貫性を保ちたいメンテナー

## 4. Core User Flow

### vec<T> の使用
```mc
// 1. 型引数付きでベクトルを作成
var v: vec<int> = vec::new();  // または型推論

// 2. 要素を追加
v.push(10);
v.push(20);

// 3. 要素にアクセス
print(v[0]);      // インデックスアクセス
print(v.get(0));  // メソッドアクセス
v.set(0, 100);

// 4. 長さ取得・pop
print(v.len());
let x = v.pop();
```

### map<K,V> の使用
```mc
// 1. 型引数付きでマップを作成
let m: map<string, int> = map::new();

// 2. キー・値を追加
m.put("score", 100);
m.put("level", 5);

// 3. 値にアクセス
print(m.get("score"));

// 4. 操作
print(m.contains("score"));
m.remove("level");
print(m.len());
```

## 5. Inputs & Outputs

### Inputs
- ジェネリック型パラメータ付きの `vec<T>`, `map<K,V>` 宣言
- 各メソッド呼び出し時の引数

### Outputs
- 型チェック：不正な型の要素挿入・取得をコンパイル時にエラー
- monomorphisation：使用される具体型ごとに特殊化されたコード生成
- 実行時：既存と同等の動作（ヒープ操作ベース）

## 6. Tech Stack
- 言語：Rust（既存のmocaコンパイラ）
- 主要変更ファイル：
  - `src/compiler/types.rs` - 型定義
  - `src/compiler/typechecker.rs` - 型チェック
  - `src/compiler/monomorphise.rs` - 特殊化
  - `src/compiler/resolver.rs` - 名前解決
  - `src/compiler/codegen.rs` - コード生成
  - `std/prelude.mc` - 標準ライブラリ実装
- テストフレームワーク：既存のスナップショットテスト

## 7. Rules & Constraints

### 型システムルール
- `vec<T>` の `T` は任意の型（プリミティブ、構造体、ネストしたジェネリクス）
- `map<K,V>` の `K` は `int` または `string`（ハッシュ関数が実装済みの型）
- `map<K,V>` の `V` は任意の型
- 型推論：`vec::new()` の戻り値型は使用箇所から推論可能

### 実装ルール
- Monomorphisation方式：`vec<int>` と `vec<string>` は別々の特殊化コードを生成
- 内部レイアウト：既存の ptr/len/cap（vec）、buckets/size/capacity（map）を維持
- ヒープ操作：`__alloc_heap`, `__heap_load`, `__heap_store` を継続使用

### 互換性
- 構文は維持：`vec::new()`, `map::new()`, `.push()`, `.get()` 等
- `vec<any>`, `map<any,any>` は引き続きサポート（後方互換）

## 8. Open Questions
- なし（仕様確定済み）

## 9. Acceptance Criteria

1. `vec<int>` を宣言し、`int` 型の値を push/get できる
2. `vec<string>` を宣言し、`string` 型の値を push/get できる
3. `vec<int>` に `string` を push しようとするとコンパイルエラーになる
4. `map<string, int>` を宣言し、string キーで int 値を put/get できる
5. `map<int, string>` を宣言し、int キーで string 値を put/get できる
6. `map<string, int>` に int 値以外を put しようとするとコンパイルエラーになる
7. ネストした型 `vec<vec<int>>` が動作する
8. 構造体を要素に持つ `vec<Point>` が動作する（Point は任意のユーザー定義構造体）
9. `VectorAny`, `HashMapAny` が prelude.mc から削除されている
10. 既存の17個のvec/map関連テストが全て通過する

## 10. Verification Strategy

### 進捗検証
- 各フェーズ完了時に `cargo check` と `cargo test` を実行
- 型エラーテストを先に書き、コンパイルエラーが正しく出ることを確認

### 達成検証
- 全 Acceptance Criteria をチェックリストで確認
- `cargo test` で全テスト通過
- `cargo clippy` で警告なし

### 漏れ検出
- 既存の17個のvec/map関連テストを具体型に書き換え
- 新規テストケース追加：型エラー検出、ネスト型、構造体要素

## 11. Test Plan

### E2E シナリオ 1: vec<T> 基本操作
```
Given: 空の vec<int> を作成
When: push(10), push(20), get(0), pop() を実行
Then: get(0) は 10 を返し、pop() は 20 を返し、len() は 1
```

### E2E シナリオ 2: map<K,V> 基本操作
```
Given: 空の map<string, int> を作成
When: put("a", 1), put("b", 2), get("a"), remove("b") を実行
Then: get("a") は 1 を返し、contains("b") は false、len() は 1
```

### E2E シナリオ 3: 型エラー検出
```
Given: vec<int> を宣言
When: v.push("hello") をコンパイル
Then: 型エラー「expected int, got string」が発生
```
