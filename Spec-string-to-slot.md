# Spec.md - String to Slot Migration

## 1. Goal
- VM上のStringデータを `MocaSlots` 形式に統一し、`MocaString` を廃止する
- コードの複雑性を低減し、ヒープオブジェクトの種類を減らす

## 2. Non-Goals
- 新しい文字列操作（部分文字列取得、文字置換など）の追加
- UTF-8/Unicodeサポートの拡張
- 文字列の最適化（インターニングなど）
- 既存APIの振る舞い変更

## 3. Target Users
- Moca言語のユーザー（文字列を使用するプログラム）
- Moca VMの開発者

## 4. Core User Flow
1. ユーザーが文字列リテラル `"hello"` を記述
2. コンパイラが `PushString` 命令を生成
3. VMが文字列を `MocaSlots` として割り当て（各文字をASCII値で保存）
4. 文字列操作（連結、長さ取得、インデックスアクセス）が動作

## 5. Inputs & Outputs

### 入力
- 文字列リテラル（ASCII文字列）

### 出力（内部表現）
```
"hello" → MocaSlots {
    header: ObjectHeader { obj_type: Slots, marked: false },
    slots: [104, 101, 108, 108, 111]  // h, e, l, l, o のASCII値
}
```

### 長さ取得
- `slots.len()` から取得（O(1)）

### インデックスアクセス
- `slots[index]` で直接アクセス（オフセット不要）

## 6. Tech Stack
- 言語: Rust
- テスト: cargo test + snapshot tests
- 対象ファイル:
  - `src/vm/heap.rs` - MocaString削除、割り当て関数変更
  - `src/vm/vm.rs` - PushString、文字列操作の実装変更
  - `src/vm/ops.rs` - 必要に応じて変更

## 7. Rules & Constraints

### 振る舞いのルール
- 文字列は不変（immutable）として扱う
- 各文字はASCII値（0-127）として `Value::Int` で保存
- 空文字列は空のSlots `[]` として表現

### 技術的制約
- 既存のmark-and-sweep GCはそのまま動作すること
- `ObjectType::String` は削除し、文字列は `ObjectType::Slots` を使用
- 文字列かどうかの判別が必要な場合は、別の方法で対応（要検討）

### 破ってはいけない前提
- 既存のsnapshotテストが同じ結果を返すこと

## 8. Open Questions
- 文字列とSlots（配列）を区別する必要がある場面はあるか？
  - (仮) 現時点では区別不要と想定。問題が出たら対応

## 9. Acceptance Criteria

1. [ ] `MocaString` 構造体が削除されている
2. [ ] `ObjectType::String` が削除されている
3. [ ] `PushString` が文字列を `MocaSlots` として割り当てる
4. [ ] 文字列連結（Add操作）が正しく動作する
5. [ ] 文字列長取得（ArrayLen操作）が正しく動作する
6. [ ] 文字インデックスアクセス（HeapLoadDyn）が正しく動作する
7. [ ] 既存のstring関連snapshotテストがすべてpassする
8. [ ] `cargo test` が全てpassする
9. [ ] GCが文字列（Slots形式）を正しくトレースする
10. [ ] コンパイルエラー・警告がない

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo test` を実行
- コンパイルエラーがないことを確認

### 達成検証
- 全Acceptance Criteriaをチェックリストで確認
- `cargo test` が全てpass

### 漏れ検出
- `tests/snapshots/basic/string_*.mc` のテストケースを確認
- 文字列を使用する他のテストも確認

## 11. Test Plan

### E2E シナリオ 1: 文字列リテラルの生成と表示
```
Given: 文字列リテラル "hello" を含むプログラム
When: プログラムを実行
Then: "hello" が正しく出力される
```

### E2E シナリオ 2: 文字列連結
```
Given: "hello" + " world" を含むプログラム
When: プログラムを実行
Then: "hello world" が正しく出力される
```

### E2E シナリオ 3: 文字列操作の組み合わせ
```
Given: 文字列長取得、インデックスアクセスを含むプログラム
When: プログラムを実行
Then: 既存のsnapshotテストと同じ結果が出力される
```
