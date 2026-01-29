# Spec.md

## 1. Goal
- VMの`VectorPush`/`VectorPop`オペコードを廃止し、コンパイラが低レベル操作（`SlotGet`/`SlotSet`/`AllocHeap`等）に展開するようにする
- これにより、Vector操作のロジックがランタイム（vm.rs）からコンパイラ（codegen.rs）に移動する

## 2. Non-Goals
- 型安全性の追加（Vector要素の型チェック）は今回やらない
- `vec_new`の変更（現状の`AllocHeap(3)`で十分）
- パフォーマンス最適化（バイトコードサイズ増加は許容）
- 新しいVector操作の追加

## 3. Target Users
- Mocaコンパイラ/VM開発者
- 将来的にstdライブラリでVector操作を実装する際の基盤

## 4. Core User Flow
1. ユーザーがMocaコードで`push(vec, value)`を呼び出す
2. コンパイラが低レベル操作列に展開してバイトコードを生成
3. VMが展開された操作を実行（専用オペコードなし）
4. 既存のVector操作と同じ振る舞いを維持

## 5. Inputs & Outputs

### Inputs
- Mocaソースコード内の`push(vec, value)`, `pop(vec)`, `vec_len(vec)`呼び出し

### Outputs
- 展開されたバイトコード（`SlotGet`, `SlotSet`, `AllocHeap`, `Jump`等の組み合わせ）

### 変換例: `push(vec, value)`
```
// Before: Op::VectorPush

// After (疑似コード):
// 1. len = vec.slots[1]
// 2. cap = vec.slots[2]
// 3. if len >= cap then grow()
// 4. data = vec.slots[0]
// 5. data.slots[len] = value
// 6. vec.slots[1] = len + 1
```

## 6. Tech Stack
- 言語: Rust（既存）
- 変更対象ファイル:
  - `src/compiler/codegen.rs` - コード生成ロジック追加
  - `src/vm/ops.rs` - `VectorPush`/`VectorPop`オペコード削除
  - `src/vm/vm.rs` - 対応するランタイム実装削除
  - `src/vm/bytecode.rs` - バイトコード定義削除
- テスト: 既存スナップショットテスト（`tests/snapshots/`）

## 7. Rules & Constraints

### 振る舞いルール
- `push`: 容量不足時は`max(8, cap * 2)`で拡張（既存動作を維持）
- `pop`: 空ベクターからのpopはランタイムエラー（既存動作を維持）
- `vec_len`: ベクターの現在の長さを返す

### 技術的制約
- Vectorの内部構造は維持: `[ptr, len, cap]`の3スロットヘッダ
- 既存の`AllocHeap`, `SlotGet`, `SlotSet`, `Jump`, `JumpIfFalse`等を組み合わせて実装
- 条件分岐（容量チェック）にはラベルとジャンプを使用

### 破ってはいけない前提
- 既存テストが全てパスすること
- ベクター操作のセマンティクスを変更しないこと

## 8. Open Questions
- なし（全て確定済み）

## 9. Acceptance Criteria

1. `Op::VectorPush`がops.rsから削除されている
2. `Op::VectorPop`がops.rsから削除されている
3. vm.rsの`VectorPush`/`VectorPop`実行ロジックが削除されている
4. bytecode.rsの`OP_VECTOR_PUSH`(76)/`OP_VECTOR_POP`(77)が削除されている
5. codegen.rsで`push(vec, value)`が低レベル操作に展開されている
6. codegen.rsで`pop(vec)`が低レベル操作に展開されている
7. codegen.rsで`vec_len(vec)`が`SlotGet`に展開されている
8. `push`の容量拡張ロジック（grow）がコンパイラ側で展開されている
9. `tests/snapshots/basic/array_operations.mc`のテストがパスする
10. `cargo test`が全てパスする

## 10. Verification Strategy

### 進捗検証
- 各操作（push/pop/vec_len）の実装完了時に`cargo test`を実行
- 生成されるバイトコードを`--debug`オプションで確認

### 達成検証
- 全Acceptance Criteriaをチェックリストで確認
- `cargo test`が全てパス

### 漏れ検出
- ops.rs/vm.rs/bytecode.rsに`Vector`関連コードが残っていないことを`grep`で確認
- 既存のarray_operations.mcテストでエッジケース（空pop、容量拡張）をカバー

## 11. Test Plan

### Scenario 1: 基本的なpush/pop操作
```
Given: 空のベクターを作成
When: 値を3つpushし、1つpopする
Then: popした値が最後にpushした値と一致し、vec_lenが2を返す
```

### Scenario 2: 容量拡張
```
Given: 空のベクターを作成
When: 9つ以上の値をpushする（初期容量8を超える）
Then: 全ての値が正しく格納され、vec_lenが正しい値を返す
```

### Scenario 3: 空ベクターからのpop
```
Given: 空のベクターを作成
When: popを実行する
Then: ランタイムエラーが発生する
```
