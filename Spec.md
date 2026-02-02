# Spec.md - 構造体初期化構文の統一

## 1. Goal

構造体リテラルの初期化構文を `type` キーワードを使った形式に統一し、コレクションリテラルと一貫した構文にする。

- Before: `Point { x: 1, y: 2 }`
- After: `type Point { x: 1, y: 2 }`

## 2. Non-Goals

- 非推奨期間を設けた段階的な移行（即時廃止とする）
- `type` キーワード以外の新しい構文の追加
- コレクションリテラル（`type Vec<int> {1, 2, 3}` など）の変更
- 構造体定義（`struct Point { x: int, y: int }`）の変更

## 3. Target Users

- Moca言語のユーザー（言語利用者）
- Moca言語の開発者（コンパイラ開発者）

## 4. Core User Flow

1. ユーザーが構造体を定義する: `struct Point { x: int, y: int }`
2. ユーザーが構造体インスタンスを作成する: `type Point { x: 1, y: 2 }`
3. コンパイラが正しくパースし、バイトコードを生成する
4. 旧構文 `Point { x: 1, y: 2 }` を使うとパースエラーになる

## 5. Inputs & Outputs

### Inputs
- Mocaソースコード（`.mc` ファイル）

### Outputs
- 新構文: 正常にコンパイル・実行
- 旧構文: パースエラー（明確なエラーメッセージ）

## 6. Tech Stack

- 言語: Rust
- テストフレームワーク: `cargo test`
- Lint: `cargo clippy`
- Formatter: `cargo fmt`

## 7. Rules & Constraints

### 構文ルール

1. 構造体リテラルは必ず `type` キーワードで始まる
2. 形式: `type <StructName> { field1: value1, field2: value2, ... }`
3. ジェネリック構造体: `type <StructName><TypeArgs> { field1: value1, ... }`
4. 空の構造体: `type <StructName> {}`
5. トレーリングカンマは許可する

### 技術的制約

1. 既存の `TypeLiteral` パース処理を拡張して構造体リテラルも処理する
2. `StructLiteral` AST ノードは維持する（内部表現は変えない）
3. 旧構文 `Name { field: value }` はパースエラーとする
4. エラーメッセージは新構文への移行を促す内容にする

### 互換性

1. 既存のテストファイル（`.mc`）は全て新構文に書き換える
2. ドキュメントは新構文に更新する

## 8. Open Questions

なし

## 9. Acceptance Criteria

1. [ ] `type Point { x: 1, y: 2 }` がパースできる
2. [ ] `type Container<int> { value: 42 }` がパースできる
3. [ ] `type Empty {}` がパースできる
4. [ ] `type Point { x: 1, }` （トレーリングカンマ）がパースできる
5. [ ] 旧構文 `Point { x: 1, y: 2 }` がパースエラーになる
6. [ ] 構造体リテラルの型チェックが正しく動作する
7. [ ] 構造体リテラルのコード生成が正しく動作する
8. [ ] 全ての既存テスト（`cargo test`）がパスする
9. [ ] `cargo clippy` が警告なしでパスする
10. [ ] ドキュメント（`docs/structs.md`, `docs/language.md`）が更新されている

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo test` を実行し、既存機能が壊れていないことを確認
- パーサー変更後、簡単なテストコードで新構文がパースできることを確認

### 達成検証
- 全 Acceptance Criteria をチェックリストで確認
- `cargo fmt && cargo check && cargo test && cargo clippy` が全てパス

### 漏れ検出
- `grep -r "{ .* : " tests/` で旧構文の残りがないか確認
- 構造体リテラルを使う全てのテストファイルをリストアップし、漏れなく更新

## 11. Test Plan

### E2E シナリオ 1: 基本的な構造体リテラル

```
Given: struct Point { x: int, y: int } が定義されている
When: let p = type Point { x: 10, y: 20 }; を実行する
Then: p.x == 10 かつ p.y == 20 である
```

### E2E シナリオ 2: ジェネリック構造体リテラル

```
Given: struct Container<T> { value: T } が定義されている
When: let c = type Container<int> { value: 42 }; を実行する
Then: c.value == 42 である
```

### E2E シナリオ 3: 旧構文のエラー

```
Given: struct Point { x: int, y: int } が定義されている
When: let p = Point { x: 1, y: 2 }; をパースする
Then: パースエラーが発生する
```
