# Spec.md - Associated Function の導入

## 1. Goal
- `vec::new()`や`map::new()`のようにassociated function（型に紐づく静的関数）を定義・呼び出しできるようにする

## 2. Non-Goals
- インスタンスメソッド（`self`を取るメソッド）の変更
- 新しい型の追加
- JITコンパイラの対応（VMのみ対応）
- `vec_new()`等の既存関数の互換性維持（削除する）

## 3. Target Users
- mocaプログラマー
- より直感的なAPI（`vec::new()`）を使いたい開発者

## 4. Core User Flow
1. ユーザーが`impl vec { fun new() -> vec<any> { ... } }`のようにassociated functionを定義
2. ユーザーが`vec::new()`の形式で呼び出し
3. 型推論により戻り値の型が決定される（`let v: vec<int> = vec::new()`なら`vec<int>`に推論）

## 5. Inputs & Outputs

### Input（ソースコード）
```moca
impl vec {
    fun new() -> vec<any> {
        // 実装
    }
}

let v = vec::new();
```

### Output
- 正常にパース・型チェック・コンパイル・実行される
- `vec::new()`が対応する関数を呼び出す

## 6. Tech Stack
- 言語: Rust
- テスト: cargo test + スナップショットテスト
- フォーマット: cargo fmt
- Lint: cargo clippy

## 7. Rules & Constraints

### 構文ルール
- associated functionは`impl TypeName { fun name(...) -> ReturnType { ... } }`で定義
- 呼び出しは`TypeName::function_name(args)`形式
- `self`パラメータを持たない関数のみがassociated function

### 対象の型
- `vec`（ビルトイン）
- `map`（ビルトイン）
- ユーザー定義struct

### 型推論ルール
- 戻り値型に`any`を含む場合、使用箇所から型推論される
- 明示的に型注釈がある場合はそれに従う
- 例: `let v: vec<int> = vec::new()` → `vec<int>`として推論

### 削除対象
- `vec_new()`関数（prelude.mcから削除）
- `map_new()`関数（prelude.mcから削除）
- typechecker.rsの`vec_new`/`map_new`ビルトイン処理

## 8. Open Questions
なし

## 9. Acceptance Criteria

1. `impl vec { fun new() -> vec<any> { ... } }`の形式でassociated functionを定義できる
2. `impl map { fun new() -> map<any, any> { ... } }`の形式でassociated functionを定義できる
3. ユーザー定義structに対して`impl Point { fun origin() -> Point { ... } }`の形式でassociated functionを定義できる
4. `vec::new()`の形式でassociated functionを呼び出せる
5. `map::new()`の形式でassociated functionを呼び出せる
6. `Point::origin()`の形式でユーザー定義structのassociated functionを呼び出せる
7. 引数付きassociated function（例: `vec::with_capacity(10)`）が定義・呼び出しできる
8. 戻り値の型推論が機能する（`let v: vec<int> = vec::new()`で`v`が`vec<int>`になる）
9. `vec_new()`および`map_new()`が削除されている
10. 全ての既存テストがパスする

## 10. Verification Strategy

### 進捗検証
- 各フェーズ完了時に`cargo check`と`cargo test`を実行
- パース→型チェック→コード生成→実行の順に段階的に動作確認

### 達成検証
- 上記Acceptance Criteriaを全てチェックリストで確認
- 新規スナップショットテストで`vec::new()`と`map::new()`の動作を確認

### 漏れ検出
- `cargo clippy`でwarningがないことを確認
- 既存の全テストがパスすることを確認
- `vec_new`/`map_new`がコードベースに残っていないことをgrepで確認

## 11. Test Plan

### E2E シナリオ 1: ビルトイン型のassociated function
```
Given: prelude.mcに`impl vec { fun new() -> vec<any> { ... } }`が定義されている
When: `let v = vec::new(); v.push(1); print(v.len());`を実行する
Then: `1`が出力される
```

### E2E シナリオ 2: ユーザー定義structのassociated function
```
Given: `struct Point { x: int, y: int }` と `impl Point { fun origin() -> Point { return Point { x: 0, y: 0 }; } }`が定義されている
When: `let p = Point::origin(); print(p.x);`を実行する
Then: `0`が出力される
```

### E2E シナリオ 3: 型推論の確認
```
Given: `impl vec { fun new() -> vec<any> { ... } }`が定義されている
When: `let v: vec<int> = vec::new(); v.push(1); let x: int = v.get(0); print(x);`を実行する
Then: コンパイルエラーなく`1`が出力される
```
