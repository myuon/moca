# Spec.md — HeapLoad/HeapStore系のネイティブMicroOp化

## 1. Goal
- `HeapLoad(n)` / `HeapLoadDyn` / `HeapStore(n)` / `HeapStoreDyn` をRaw fallbackからネイティブMicroOpに変換し、StackPush/StackPop のオーバーヘッドを排除する

## 2. Non-Goals
- `HeapAlloc*` 系のMicroOp化
- HeapLoad/HeapStoreDynをJITコンパイル対応にする（JITはOp級で動いておりMicroOpは関与しない）
- 複合命令の特殊化（例: HeapLoad(0) + HeapLoadDyn を1命令にまとめるなど）

## 3. Target Users
- moca言語の開発者（内部最適化）

## 4. Core User Flow
- ユーザーから見た挙動変更なし。MicroOp内部表現のみ変更。

## 5. Inputs & Outputs
- 入力: moca ソースコード
- 出力: 実行結果は変更前と完全一致。`--dump-microops` でネイティブMicroOp表現に変わる。

## 6. Tech Stack
- Rust（既存プロジェクト）
- テスト: `cargo test`（既存スナップショットテスト）

## 7. Rules & Constraints

### 7.1 新しいMicroOpバリアント

```rust
// dst = heap[src][offset] (静的オフセット)
MicroOp::HeapLoad { dst: VReg, src: VReg, offset: usize }

// dst = heap[obj][idx] (動的インデックス)
MicroOp::HeapLoadDyn { dst: VReg, obj: VReg, idx: VReg }

// heap[dst_obj][offset] = src
MicroOp::HeapStore { dst_obj: VReg, offset: usize, src: VReg }

// heap[obj][idx] = src
MicroOp::HeapStoreDyn { obj: VReg, idx: VReg, src: VReg }
```

### 7.2 スタックセマンティクス（Op → MicroOp変換）

| Op | スタック効果 | MicroOp変換 |
|----|------------|-------------|
| `HeapLoad(n)` | pop ref, push ref[n] | vstack pop 1 → HeapLoad { dst, src, n } → vstack push dst |
| `HeapLoadDyn` | pop index, pop ref, push ref[index] | vstack pop 2 → HeapLoadDyn { dst, obj, idx } → vstack push dst |
| `HeapStore(n)` | pop value, pop ref → ref[n]=value | vstack pop 2 → HeapStore { dst_obj, n, src } |
| `HeapStoreDyn` | pop value, pop index, pop ref → ref[index]=value | vstack pop 3 → HeapStoreDyn { obj, idx, src } |

### 7.3 変換前後のhot path比較（text_countのlen(text)の例）

**変更前:**
```
StackPush v6        ← vstackフラッシュ（不要な退避）
StackPush v0        ← vstackフラッシュ（不要な退避）
Raw { HeapLoad(1) } ← スタック経由
StackPop v13        ← 結果取得
StackPop v14        ← フラッシュされたv6回収
```

**変更後:**
```
HeapLoad v13, v0, 1  ← 1命令で完了、vstackフラッシュ不要
```

### 7.4 変更対象ファイル

| ファイル | 変更内容 |
|---------|---------|
| `src/vm/microop.rs` | 4バリアント追加 |
| `src/vm/microop_converter.rs` | `Op::HeapLoad(n)` / `HeapLoadDyn` / `HeapStore(n)` / `HeapStoreDyn` をネイティブ変換 |
| `src/vm/vm.rs` | `run_microop()` に4バリアントの実行ハンドラ追加 |
| `src/compiler/dump.rs` | `format_single_microop` に4バリアントの表示追加 |

## 8. Open Questions
- なし

## 9. Acceptance Criteria

1. `cargo test` が全パスする
2. `cargo clippy` が警告なしでパスする
3. `--dump-microops` でHeapLoad/HeapLoadDynがネイティブMicroOpとして表示される（`Raw { HeapLoad(...) }` ではない）
4. `--dump-microops` でHeapStore/HeapStoreDynがネイティブMicroOpとして表示される
5. hot pathでStackPush/StackPopの数が減少している
6. 既存のスナップショットテスト出力に変化なし

## 10. Verification Strategy
- **進捗検証**: 各タスク完了後に `cargo check && cargo test` を実行
- **達成検証**: text_countの `--dump-microops` 出力で `Raw { HeapLoad` が消えていることを目視確認
- **漏れ検出**: `--dump-microops` 出力を変更前後で比較し、StackPush/StackPopの減少を確認

## 11. Test Plan

### Scenario 1: HeapLoad/HeapLoadDynのネイティブ化
- **Given**: HeapLoad(n) を含む関数
- **When**: microop_converterで変換
- **Then**: `MicroOp::HeapLoad { dst, src, offset }` が生成される（Rawではない）

### Scenario 2: HeapStore/HeapStoreDynのネイティブ化
- **Given**: HeapStoreDyn を含む関数
- **When**: microop_converterで変換
- **Then**: `MicroOp::HeapStoreDyn { obj, idx, src }` が生成される（Rawではない）

### Scenario 3: 既存テスト全パス
- **Given**: 既存のテストスイート
- **When**: `cargo test` を実行
- **Then**: 全テストがパスする

## TODO

- [ ] 1. `MicroOp` enumに4バリアント追加 + dump表示対応
- [ ] 2. `microop_converter` で HeapLoad(n) / HeapLoadDyn をネイティブ変換
- [ ] 3. `microop_converter` で HeapStore(n) / HeapStoreDyn をネイティブ変換
- [ ] 4. `vm.rs` の `run_microop()` に4バリアントの実行ハンドラ追加
- [ ] 5. `cargo fmt && cargo check && cargo test && cargo clippy` を実行して全パスを確認
