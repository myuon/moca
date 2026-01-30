# Spec.md - HeapObject Slots統一

## 1. Goal
- `HeapObject::String`を削除し、全てのヒープオブジェクトを`HeapObject::Slots`に統一する
- 配列/構造体/Vectorのslot[0]の冗長な長さ/フィールド数を削除し、`slots.len()`で統一する

## 2. Non-Goals
- MocaObjectの変更（key-valueマップは別の用途）
- パフォーマンス最適化
- 新機能の追加

## 3. Target Users
- Moca VMの開発者

## 4. Core User Flow
1. 既存のMocaコードがそのまま動作する
2. 内部表現が統一され、コードがシンプルになる

## 5. Inputs & Outputs

### Before
```
HeapObject::String(MocaSlots)  // ObjectType::String
HeapObject::Slots(MocaSlots)   // ObjectType::Slots

Array:  [len, e0, e1, ...]     // slot[0] = 長さ（冗長）
Struct: [n, f0, f1, ...]       // slot[0] = フィールド数（冗長）
Vector: [3, ptr, len, cap]     // slot[0] = 3（冗長）
String: [c0, c1, ...]          // OK
```

### After
```
HeapObject::Slots(MocaSlots)   // ObjectType::Slots のみ

Array:  [e0, e1, ...]          // len = slots.len()
Struct: [f0, f1, ...]          // field_count = slots.len()
Vector: [ptr, len, cap]        // field_count = slots.len() = 3
String: [c0, c1, ...]          // len = slots.len()
```

## 6. Tech Stack
- 言語: Rust
- テスト: cargo test + snapshot tests
- 対象ファイル:
  - `src/vm/heap.rs` - HeapObject::String削除、ObjectType::String削除
  - `src/vm/vm.rs` - 全操作の修正
  - `src/compiler/codegen.rs` - AllocHeap、インデックスアクセスの修正
  - `src/ffi/stack.rs` - 文字列判定の修正
  - `std/prelude.mc` - Vectorレイアウト修正

## 7. Rules & Constraints

### 振る舞いの変更
- `typeof("hello")` は `"slots"` を返す（破壊的変更）
- 文字列と配列の区別は不可になる（同じSlotsとして扱う）

### 技術的制約
- 既存のsnapshotテストは結果が変わる可能性あり（typeof関連）
- GCは変更不要（Slotsのtraceがそのまま使える）

## 8. Open Questions
- なし

## 9. Acceptance Criteria

1. [ ] `HeapObject::String`バリアントが削除されている
2. [ ] `ObjectType::String`が削除されている
3. [ ] 配列が`[e0, e1, ...]`形式で格納される（slot[0]の長さなし）
4. [ ] 構造体が`[f0, f1, ...]`形式で格納される（slot[0]のフィールド数なし）
5. [ ] Vectorが`[ptr, len, cap]`形式で格納される（slot[0]の3なし）
6. [ ] `ArrayLen`が`slots.len()`を返す
7. [ ] インデックスアクセスに`+1`オフセットがない
8. [ ] 構造体フィールドアクセスに`+1`オフセットがない
9. [ ] `cargo test`が全てpass
10. [ ] `cargo clippy`が警告なし

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に`cargo check`でコンパイル確認
- 段階的に`cargo test`で動作確認

### 達成検証
- 全Acceptance Criteriaをチェックリストで確認
- `grep "HeapObject::String\|ObjectType::String" src/` で残存確認

### 漏れ検出
- snapshot testsで既存動作の確認
- 文字列・配列・構造体・Vectorを使うテストケースの確認

## 11. Test Plan

### E2E シナリオ 1: 配列操作
```
Given: 配列 [1, 2, 3] を作成
When: len() と インデックスアクセスを実行
Then: len=3, arr[0]=1, arr[2]=3 が正しく取得できる
```

### E2E シナリオ 2: 構造体操作
```
Given: 構造体 Point { x: 1, y: 2 } を作成
When: フィールドアクセス p.x, p.y を実行
Then: x=1, y=2 が正しく取得できる
```

### E2E シナリオ 3: Vector操作
```
Given: 空のVectorを作成
When: vec_pushで値を追加
Then: vec_lenとvec_getが正しく動作する
```
