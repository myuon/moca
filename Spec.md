# Spec.md

## 1. Goal
- `vec_push`の再割り当てロジックをmoca言語（std/prelude.mc）で実装し、codegen.rsの手書きコード（約190行）を削減する

## 2. Non-Goals
- 他のvector操作（vec_pop, vec_get, vec_set等）のstd移行は今回のスコープ外（できれば対応するが無理はしない）
- 型システムの大幅な変更
- パフォーマンスの改善（現状維持が目標）

## 3. Target Users
- mocaコンパイラの開発者（将来的にvector操作を拡張・修正しやすくなる）

## 4. Core User Flow
1. ユーザーがmocaコードで`vec_push(v, value)`を呼び出す
2. コンパイラが`vec_push`を認識し、内部的に`vec_push_any`への呼び出しに変換
3. `vec_push_any`（std/prelude.mc実装）が実行され、必要に応じて再割り当てを行う
4. 既存コードは変更なしで動作する

## 5. Inputs & Outputs

### Inputs
- 既存のvec_push呼び出しを含むmocaコード

### Outputs
- 同じ動作をするバイトコード（ただしstd経由で実行）

## 6. Tech Stack
- 言語: Rust（コンパイラ）、moca（標準ライブラリ）
- テスト: スナップショットテスト（tests/snapshots/*.mc）

## 7. Rules & Constraints

### 振る舞いのルール
- `vec_push(v, value)`の外部インターフェースは変更しない
- 型安全性は`vec_push`builtin側で担保し、`vec_push_any`は内部実装として使用
- 再割り当て時の容量は`max(8, cap * 2)`を維持

### 技術的制約
- intrinsic関数は`__`プレフィックスで命名（内部用であることを示す）
- std/prelude.mcはコンパイル時に自動的に読み込まれる既存の仕組みを利用

### Vectorデータレイアウト（維持）
```
[ptr, len, cap]
- Slot 0: データポインタ（null または MocaSlotsへの参照）
- Slot 1: 現在の長さ (len)
- Slot 2: 容量 (cap)
```

## 8. Open Questions
- なし

## 9. Acceptance Criteria

1. [ ] `__heap_load(ref, idx)`intrinsicが追加され、HeapLoadDynにコンパイルされる
2. [ ] `__heap_store(ref, idx, val)`intrinsicが追加され、HeapStoreDynにコンパイルされる
3. [ ] `__alloc_heap(size)`intrinsicが追加され、AllocHeapDynにコンパイルされる
4. [ ] `vec_push_any(v: vec<any>, t: any)`がstd/prelude.mcに実装されている
5. [ ] `vec_push_any`が再割り当てロジック（容量チェック→新メモリ確保→コピー→値追加）を含む
6. [ ] 既存の`vec_push`builtinが内部的に`vec_push_any`を呼び出す
7. [ ] codegen.rsの`compile_vector_push`関数が削除されている
8. [ ] 既存テスト（simple_vec.mc, vec_push_realloc.mc）が通る
9. [ ] `cargo test`が全て通る

## 10. Verification Strategy

### 進捗検証
- intrinsic追加後: 単体でintrinsicを呼び出すテストコードを実行し、正しくコンパイル・実行されることを確認
- vec_push_any実装後: 直接`vec_push_any`を呼び出すテストで動作確認

### 達成検証
- 既存のスナップショットテスト（`cargo test`）が全て通る
- `compile_vector_push`関数がcodegen.rsから削除されていることをgrepで確認

### 漏れ検出
- vec_push_reallocテストが再割り当てパスを通ることを確認（8要素以上をpush）
- 空のvectorへのpush、1要素のvectorへのpushなど境界ケースのテスト

## 11. Test Plan

### E2E シナリオ 1: 基本的なvec_push
```
Given: 空のvectorを作成
When: vec_pushで値を追加
Then: vec_lenが1になり、vec_getで値を取得できる
```

### E2E シナリオ 2: 再割り当てを伴うvec_push
```
Given: 空のvectorを作成
When: vec_pushで9回以上値を追加（初期容量8を超える）
Then: 全ての値が正しく格納され、順序が維持される
```

### E2E シナリオ 3: 既存コードの互換性
```
Given: 既存のsimple_vec.mc, vec_push_realloc.mcテスト
When: cargo testを実行
Then: 全テストがパスする
```
