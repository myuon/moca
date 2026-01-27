# Spec.md

## 1. Goal
- `obj.method(args)` 形式のメソッド呼び出しが正しく実行され、戻り値が返されるようにする

## 2. Non-Goals
- トレイト/インターフェース
- メソッドのオーバーロード
- 継承
- 動的ディスパッチ（vtable等）
- 新しい構文の追加（パーサーは実装済み）

## 3. Target Users
- Moca言語のユーザー
- 構造体にメソッドを定義して使いたい開発者

## 4. Core User Flow
1. ユーザーが構造体を定義する（`struct Point { x, y }`）
2. ユーザーが impl ブロックでメソッドを定義する
3. ユーザーが構造体インスタンスを作成する
4. ユーザーが `instance.method(args)` でメソッドを呼び出す
5. メソッドが実行され、戻り値が返される
6. メソッド内で `self` のフィールドを読み書きできる

## 5. Inputs & Outputs
### 入力
- メソッド呼び出し式: `obj.method(args)`
- オブジェクト: 構造体インスタンス
- 引数: 0個以上の式

### 出力
- メソッドの戻り値
- `self` への変更（フィールド更新）が呼び出し元のオブジェクトに反映される

## 6. Tech Stack
- 言語: Rust
- 対象ファイル: `src/compiler/codegen.rs`（主）、必要に応じて `src/compiler/resolver.rs`
- テスト: スナップショットテスト（`tests/snapshots/`）

## 7. Rules & Constraints
### 振る舞いのルール
- **静的ディスパッチ**: コンパイル時にメソッドを解決する（`StructName::method_name` 形式）
- **self の受け渡し**: 値渡し（オブジェクトへの参照がコピーされる）
- **self の可変性**: メソッド内で `self` のフィールドを変更でき、その変更は呼び出し元に反映される

### 技術的制約
- パーサー、型チェッカーは既に実装済みなので変更しない（バグ修正を除く）
- リゾルバーは必要に応じて修正可（メソッド呼び出しの解決に構造体名が必要な場合）
- 既存のテストが壊れないこと

### 実装方針
- メソッド呼び出し `obj.method(args)` は内部的に `StructName::method(obj, args)` として呼び出す
- `self` は第一引数として渡される（リゾルバーで既に対応済み）

## 8. Open Questions
- なし（仕様は確定）

## 9. Acceptance Criteria（最大10個）
1. [x] `obj.method()` で引数なしメソッドが呼び出せる
2. [x] `obj.method(arg1, arg2)` で引数ありメソッドが呼び出せる
3. [x] メソッドの戻り値が正しく返される
4. [x] メソッド内で `self.field` でフィールドを読み取れる
5. [x] メソッド内で `self.field = value` でフィールドを更新でき、呼び出し元に反映される
6. [x] メソッドチェーン `obj.method1().method2()` が動作する（method1が構造体を返す場合）
7. [x] 既存のスナップショットテストがすべてパスする
8. [x] 新規追加したメソッド呼び出しのスナップショットテストがパスする

## 10. Verification Strategy
### 進捗検証
- 各タスク完了時に `just test` でスナップショットテストを実行
- 新しいテストケースを追加するたびに期待出力を確認

### 達成検証
- 全 Acceptance Criteria をチェックリストで確認
- `just test` で全テストがパスすることを確認

### 漏れ検出
- selfの読み取り・更新が正しく動作するテストケースを必ず含める
- 既存の構造体テスト（`struct_operations.mc`）との整合性を確認

## 11. Test Plan
### e2e シナリオ 1: 基本的なメソッド呼び出し
```
Given: Counterという構造体とincrementメソッドが定義されている
When: counter.increment() を呼び出す
Then: counterのcountフィールドが1増加する
```

### e2e シナリオ 2: 戻り値のあるメソッド
```
Given: Pointという構造体とdistanceメソッドが定義されている
When: point.distance() を呼び出す
Then: 計算された距離が戻り値として返される
```

### e2e シナリオ 3: selfのフィールド更新
```
Given: Counterという構造体とreset, getメソッドが定義されている
When: counter.reset() を呼び出した後、counter.get() を呼び出す
Then: resetで設定した値がgetで取得できる
```
