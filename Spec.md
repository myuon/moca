# Spec.md - any型の実装

## 1. Goal
- moca言語に`:any`型を追加し、型チェッカーがどんな型でも通すようにする

## 2. Non-Goals
- ランタイムでの動的型チェック
- any型からの安全なキャスト機構
- any型の値に対する型ガード
- TypeScriptのunknown型のような厳格なany

## 3. Target Users
- moca言語のユーザー
- 利用シーン: プロトタイピング、外部FFI連携、型推論が難しいコードの一時的な回避

## 4. Core User Flow
1. ユーザーが変数宣言で`:any`を使用する（例: `let x: any = 42;`）
2. 型チェッカーがany型を認識し、型エラーを発生させない
3. any型の値は他の型との演算・代入で型チェックを通過する
4. コンパイル・実行が正常に完了する

## 5. Inputs & Outputs
- **入力**: `:any`型アノテーションを含むmocaソースコード
- **出力**: 型チェックが通過し、正常にコンパイル・実行されるプログラム

## 6. Tech Stack
- 言語: Rust
- 対象モジュール:
  - `src/compiler/types.rs` - Type enum に Any バリアント追加
  - `src/compiler/typechecker.rs` - unify関数でanyの特別処理
  - `src/compiler/parser.rs` - `any`キーワードのパース
- テスト: `tests/snapshots/` 配下にテストケース追加

## 7. Rules & Constraints

### 型の振る舞いルール
- `any`は全ての型と単一化(unify)できる
- `any`型の値を別の変数に代入すると、その変数も`any`型になる
- `any`型の値に対する演算（`+`, `-`, `==`など）は型チェックを通過し、結果も`any`型
- 関数の引数・戻り値に`:any`を使用可能
- `any`型は`nil`を含む（`any?`は不要だが、書いてもエラーにしない）

### 技術的制約
- 既存のテストケースが全て通過すること
- 型推論（Algorithm W）の動作を壊さないこと
- パフォーマンスへの影響を最小限にすること

## 8. Open Questions
なし

## 9. Acceptance Criteria

1. `let x: any = 42;` が型チェックを通過する
2. `let x: any = "hello";` が型チェックを通過する
3. `let x: any = nil;` が型チェックを通過する
4. `let x: any = [1, 2, 3];` が型チェックを通過する
5. `let y: int = 1; let x: any = y;` が型チェックを通過する
6. `let x: any = 1; let y = x + 1;` が型チェックを通過し、yはany型になる
7. `fun f(x: any) -> any { return x; }` が型チェックを通過する
8. `let x: any = 1; let y: int = x;` が型チェックを通過する（anyは任意の型に代入可能）
9. 既存の全テストケースが通過する
10. `cargo build`が警告なしで通過する

## 10. Verification Strategy

### 進捗検証
- 各実装ステップ完了時に `cargo test` を実行
- 新しいテストケースを追加するたびに期待通りの結果か確認

### 達成検証
- 全Acceptance Criteriaをテストケースとして実装し、全て通過することを確認
- `cargo test` が全て通過
- `cargo build` が警告なしで通過

### 漏れ検出
- 既存のスナップショットテストが全て通過することで、既存機能への影響がないことを確認
- エッジケース（any同士の演算、anyとnullableの組み合わせ等）のテストを追加

## 11. Test Plan

### E2E シナリオ 1: 基本的なany型の使用
```
Given: any型を使った変数宣言を含むmocaファイル
When: コンパイル・実行する
Then: 型エラーなしで実行が完了する
```

```mc
let x: any = 42;
let y: any = "hello";
let z: any = nil;
print(x);
print(y);
print(z);
```

### E2E シナリオ 2: any型と他の型の相互作用
```
Given: any型と他の型を混在させたmocaファイル
When: コンパイル・実行する
Then: 型エラーなしで実行が完了する
```

```mc
let x: any = 10;
let y = x + 5;      // yはany型
let z: int = 20;
let w: any = z;     // intからanyへの代入
print(y);
print(w);
```

### E2E シナリオ 3: 関数でのany型使用
```
Given: any型を引数・戻り値に使った関数を含むmocaファイル
When: コンパイル・実行する
Then: 型エラーなしで実行が完了する
```

```mc
fun identity(x: any) -> any {
    return x;
}

let a = identity(42);
let b = identity("hello");
let c = identity(nil);
print(a);
print(b);
print(c);
```
