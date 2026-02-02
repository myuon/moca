# Spec.md - type構文からnew構文への変更

## 1. Goal
- コレクションリテラル構文を `type` から `new` キーワードに変更し、予約語エスケープ機構（バッククォート）を導入する

## 2. Non-Goals
- VM最適化（`VecLiteral`/`MapLiteral` オプコードの `uninit` + `set` への置き換え）は今回やらない
- 移行期間や両構文の並行サポートはしない
- `new` 以外の用途（クラスインスタンス化など）の追加

## 3. Target Users
- moca言語のユーザー（コレクション初期化時に `new` 構文を使用）
- 既存コードで `new` という識別子を使っていたユーザー（バッククォートエスケープで対応）

## 4. Core User Flow
1. ユーザーが `new Vec<int> {1, 2, 3}` と記述
2. パーサーが `new` キーワードを認識し、コレクションリテラルとしてパース
3. リゾルバーが `uninit` + `set` 呼び出しにデシュガー
4. 既存の `new` という名前の関数/変数は `` `new` `` と記述することで使用可能

## 5. Inputs & Outputs

### 入力（ソースコード）
```mc
// コレクション初期化
let v: Vec<int> = new Vec<int> {1, 2, 3};
let m: Map<string, int> = new Map<string, int> {"a": 1, "b": 2};

// 予約語エスケープ
fun `new`() { return 42; }
let x = `new`();
```

### 出力（実行結果）
- Vec/Mapが正しく初期化される
- エスケープされた識別子が通常の識別子として扱われる

## 6. Tech Stack
- 言語: Rust
- テスト: cargo test（スナップショットテスト）
- フォーマット: cargo fmt
- Lint: cargo clippy

## 7. Rules & Constraints

### 構文ルール
- `new` は予約語となる
- コレクションリテラル構文: `new 型名<型引数> { 要素 }`
- 要素形式は2種類（混在禁止）:
  - `expr, expr, ...` — Vec/array用
  - `key: value, key: value, ...` — Map用
- 空リテラル対応: `new Vec<int> {}`

### 予約語エスケープルール
- バッククォートで囲むことで任意の予約語を識別子として使用可能: `` `keyword` ``
- 対象: すべての予約語（`new`, `let`, `fun`, `if`, etc.）
- エスケープされた識別子は通常の識別子と同等に扱う

### デシュガールール
- `new Vec<T> {e1, e2, ...}` は以下に展開:
  ```
  {
    var __tmp = Vec<T>::uninit(n);
    __tmp.set(0, e1);
    __tmp.set(1, e2);
    ...
    __tmp
  }
  ```
- `new Map<K,V> {k1: v1, k2: v2, ...}` は以下に展開:
  ```
  {
    var __tmp = Map<K,V>::uninit(n);
    __tmp.set(k1, v1);
    __tmp.set(k2, v2);
    ...
    __tmp
  }
  ```

### 破壊的変更
- `type` キーワードによるコレクションリテラル構文は削除
- 既存テストの構文を `new` に更新する必要あり

## 8. Open Questions
- `uninit` メソッドと `set` メソッドがVec/Mapに実装されているか要確認（なければ追加）

## 9. Acceptance Criteria（最大10個）

1. `new Vec<int> {1, 2, 3}` がパースできる
2. `new Map<string, int> {"a": 1}` がパースできる
3. `new Vec<int> {}` （空リテラル）がパースできる
4. `new` が予約語として認識される
5. `` `new` `` でエスケープした識別子が関数名/変数名として使用できる
6. `` `let` ``, `` `if` `` など他の予約語もエスケープ可能
7. 旧構文 `type Vec<int> {...}` がパースエラーになる
8. `new Vec<int> {1, 2, 3}` で作成したVecの `.len()` が `3` を返す
9. `new Map<string, int> {"a": 1}` で作成したMapの `.get_string("a")` が `1` を返す
10. 全既存テストが新構文で通過する

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo check` でコンパイルエラーがないことを確認
- 新構文のパーステストを追加し、`cargo test` で動作確認

### 達成検証
- 全Acceptance Criteriaをチェックリストで確認
- `cargo fmt && cargo check && cargo test && cargo clippy` が全てパス

### 漏れ検出
- 既存のスナップショットテスト（`tests/snapshots/generics/`）を新構文に更新し、全テスト通過を確認
- `type` キーワードでコレクションリテラルを書いたテストがエラーになることを確認

## 11. Test Plan

### E2E シナリオ 1: Vec リテラル基本動作
```
Given: ソースコード `let v: Vec<int> = new Vec<int> {1, 2, 3}; print(v.len()); print(v.get(1));`
When: コンパイル・実行する
Then: 出力が `3` と `2` になる
```

### E2E シナリオ 2: 予約語エスケープ
```
Given: ソースコード `fun `new`() { return 42; } print(`new`());`
When: コンパイル・実行する
Then: 出力が `42` になる
```

### E2E シナリオ 3: 旧構文の拒否
```
Given: ソースコード `let v: Vec<int> = type Vec<int> {1, 2, 3};`
When: コンパイルする
Then: パースエラーが発生する
```
