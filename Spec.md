# Spec.md

## 1. Goal
- VM配列プリミティブ（AllocArray, ArrayGet, ArraySet, ArrayLen）を削除し、汎用的な低レベルプリミティブ（HeapLoad, HeapStore）を使ってコンパイラ側で配列操作を実装する

## 2. Non-Goals
- push/pop対応（可変長配列は後回し、VMプリミティブを残す）
- 文字列操作の変更（ArrayLen/ArrayGetの文字列対応は現状維持）
- Vector型の新規実装
- パフォーマンス最適化（機能維持が優先）

## 3. Target Users
- mocaコンパイラ/VM開発者
- 将来的にmocaを拡張する開発者

## 4. Core User Flow
1. moca言語で配列リテラル `[1, 2, 3]` を記述
2. コンパイラが新しいヒープレイアウトで配列を割り当てるコードを生成
3. 配列アクセス `arr[i]` が `HeapLoad` にコンパイルされる
4. 配列代入 `arr[i] = v` が `HeapStore` にコンパイルされる
5. `len(arr)` がヘッダフィールドアクセス（HeapLoad）にコンパイルされる
6. 既存のmocaプログラムが変更なしで動作する

## 5. Inputs & Outputs
### Inputs
- 既存のmocaソースコード（配列操作を含む）

### Outputs
- 新しいバイトコード（HeapLoad/HeapStoreベース）
- 変更されたVM実行結果（既存と同一の出力）

## 6. Tech Stack
- 言語: Rust（既存）
- 対象モジュール:
  - `src/vm/ops.rs` - 新プリミティブ定義
  - `src/vm/vm.rs` - 新プリミティブ実行
  - `src/vm/heap.rs` - 配列レイアウト変更
  - `src/vm/bytecode.rs` - オペコード追加
  - `src/compiler/codegen.rs` - 配列コンパイル変更
- テスト: 既存スナップショットテスト

## 7. Rules & Constraints

### 新しい配列メモリレイアウト（インライン型）
```
┌─────────────────┬────────┬────────┬────────┬────────┬─────┐
│  ObjectHeader   │  len   │ elem0  │ elem1  │ elem2  │ ... │
│  (型情報, GC)   │ (i64)  │(Value) │(Value) │(Value) │     │
└─────────────────┴────────┴────────┴────────┴────────┴─────┘
      slot 0        slot 1   slot 2   slot 3   slot 4
```

### 新しいVMプリミティブ
| プリミティブ | スタック効果 | 説明 |
|-------------|-------------|------|
| `HeapLoad(offset)` | `[ref] → [value]` | ヒープオブジェクトのslot[offset]を読む |
| `HeapStore(offset)` | `[ref, value] → []` | ヒープオブジェクトのslot[offset]に書く |
| `HeapLoadDyn` | `[ref, index] → [value]` | 動的インデックスでslotを読む |
| `HeapStoreDyn` | `[ref, index, value] → []` | 動的インデックスでslotに書く |
| `AllocHeap(n)` | `[v1, v2, ..., vn] → [ref]` | n個のslotを持つヒープオブジェクトを割り当て |

### コンパイル変換ルール
| 元の操作 | 新しいコンパイル結果 |
|---------|---------------------|
| `[e1, e2, e3]` (配列リテラル) | `push len; push e1; push e2; push e3; AllocHeap(4)` |
| `arr[i]` (動的インデックス) | `push arr; push i+2; HeapLoadDyn` |
| `arr[i] = v` (動的代入) | `push arr; push i+2; push v; HeapStoreDyn` |
| `len(arr)` | `push arr; HeapLoad(1)` |

※ `i+2` はヘッダ(slot0) + len(slot1) のオフセット

### 維持するVMプリミティブ（今回削除しない）
- `ArrayPush` - 可変長配列用（後回し）
- `ArrayPop` - 可変長配列用（後回し）
- `ArrayLen`, `ArrayGet` - 文字列用に一時的に残す（文字列対応は別タスク）

### GCとの整合性
- `ObjectHeader`は維持し、GCのマーキング機構は変更しない
- `HeapObject::trace()` を更新して新レイアウトの配列要素を走査
- slot 2以降の要素が`Value::Ref`の場合にトレース

## 8. Open Questions
- 文字列を将来的に同じHeapLoad/HeapStore方式に移行するか
- push/pop対応時にcapacityフィールドを追加するか、別のVector型にするか

## 9. Acceptance Criteria（最大10個）
1. [ ] `HeapLoad(offset)` プリミティブが実装され、ヒープオブジェクトのslotを読める
2. [ ] `HeapStore(offset)` プリミティブが実装され、ヒープオブジェクトのslotに書ける
3. [ ] `HeapLoadDyn` / `HeapStoreDyn` が動的インデックスで動作する
4. [ ] `AllocHeap(n)` が新レイアウトでヒープオブジェクトを割り当てる
5. [ ] 配列リテラルが新方式（AllocHeap）でコンパイルされる
6. [ ] 配列インデックスアクセスが `HeapLoadDyn` にコンパイルされる
7. [ ] 配列インデックス代入が `HeapStoreDyn` にコンパイルされる
8. [ ] `len(arr)` が `HeapLoad(1)` にコンパイルされる
9. [ ] 既存の配列関連テスト（array_operations.mc, array_mutation.mc等）がすべてパスする
10. [ ] GCが新レイアウトの配列要素を正しくトレースする

## 10. Verification Strategy

### 進捗検証
- 各フェーズ完了時に該当するテストを実行
- 新プリミティブ追加後、簡単なバイトコードを手動実行して動作確認
- `cargo test` で既存テストの破壊がないことを確認

### 達成検証
- 全Acceptance Criteriaをチェックリストで確認
- 生成されるバイトコードを目視確認（配列操作がHeapLoad/HeapStoreになっている）
- `tests/snapshots/basic/array_*.mc` のテストがすべてパス

### 漏れ検出
- for-inループのテストが通ることを確認（内部でArrayLen/ArrayGetを使用していた箇所）
- GCテスト（heap_pressure.mc）が通ることを確認
- エラーケース（index out of bounds等）が正しく動作することを確認

## 11. Test Plan

### E2E シナリオ 1: 基本的な配列操作
```
Given: 配列リテラルとインデックスアクセスを含むmocaプログラム
When: コンパイル・実行する
Then: 既存と同じ出力が得られ、バイトコードにHeapLoad/HeapStoreが含まれる
```

### E2E シナリオ 2: for-inループ
```
Given: 配列をfor-inでイテレートするmocaプログラム
When: コンパイル・実行する
Then: 既存と同じ出力が得られる（内部的にHeapLoad/HeapLoadDynを使用）
```

### E2E シナリオ 3: GCとの連携
```
Given: 配列内にオブジェクト参照を持ち、GCが発生するmocaプログラム
When: 実行してGCが発生する
Then: 配列内の参照が正しく保持され、メモリリークやダングリング参照がない
```
