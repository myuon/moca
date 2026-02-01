# Spec: Linear Memory Heap

## 1. Goal
- VM の heap を `Vec<u64>` 線形メモリ上で管理し、free list を線形メモリ内に埋め込む方式に移行する

## 2. Non-Goals
- メモリのコンパクション（断片化対策）
- 可変長オブジェクトのリサイズ（一度確保したスロット数の変更）
- マルチスレッド対応
- 外部バッファ（`&mut [u64]`）の借用サポート

## 3. Target Users
- moca VM の内部実装
- 将来的に WASM 線形メモリとの互換性を検討する開発者

## 4. Core User Flow
1. VM 初期化時に `Heap::new()` で線形メモリ（`Vec<u64>`）を確保
2. `alloc_slots(n)` で n スロット分のオブジェクトを確保、オフセットを返す
3. `get(offset)` でオフセットから `HeapObject` を構築して返す
4. GC 時に mark/sweep を実行、解放されたブロックを free list に追加
5. 再割り当て時に free list から適切なブロックを取得

## 5. Inputs & Outputs

### Inputs
- 確保したいスロット数（usize）
- GC ルート（`&[Value]`）

### Outputs
- `GcRef { offset: usize }` - オブジェクトのバイトオフセット
- `HeapObject` - パースされたオブジェクト（一時構築）

## 6. Tech Stack
- 言語: Rust（既存）
- テスト: cargo test（既存）

## 7. Rules & Constraints

### メモリレイアウト

```
Linear Memory (Vec<u64>):
+--------------------------------------------------+
| Object 0 | Free Block | Object 2 | Object 3 | ...
+--------------------------------------------------+

Object Layout (in u64 words):
+----------------+----------------+----------------+
| Header (1 word)| Slot 0         | Slot 1         | ...
+----------------+----------------+----------------+

Header Layout (64 bits):
+--------+--------+------------------+-------------+
| marked | (rsv)  | slot_count (32)  | (reserved)  |
| 1 bit  | 7 bits | 32 bits          | 24 bits     |
+--------+--------+------------------+-------------+

Free Block Layout:
+----------------+----------------+
| Header         | Next Free Ptr  |
| (marked=0,     | (offset or 0   |
|  size in slots)|  if end)       |
+----------------+----------------+
```

### Value エンコーディング（64-bit NaN-boxing 風）

```
I64:  0x00_XX_XXXX_XXXX_XXXX (tag=0x00, 下位56bitが値、ただし実際は全64bit使用)
F64:  IEEE 754 double（NaN以外はそのまま）
Bool: 0x01_00_0000_0000_000X (tag=0x01, X=0 or 1)
Null: 0x02_00_0000_0000_0000 (tag=0x02)
Ref:  0x03_XX_XXXX_XXXX_XXXX (tag=0x03, 下位bits=offset)
```

(仮) 実装の簡易化のため、まずは既存の Value enum を u64 に変換する関数を用意し、将来的に NaN-boxing に移行可能な設計とする。

### GcRef の変更

```rust
// Before
pub struct GcRef { pub index: usize }

// After
pub struct GcRef { pub offset: usize }  // バイトオフセット（u64単位）
```

### API 設計

```rust
impl Heap {
    /// 線形メモリを所有する新しいヒープを作成
    pub fn new() -> Self;

    /// 初期容量を指定してヒープを作成
    pub fn with_capacity(capacity: usize) -> Self;

    /// n スロットのオブジェクトを確保
    pub fn alloc_slots(&mut self, slots: Vec<Value>) -> Result<GcRef, String>;

    /// オフセットから HeapObject を構築して返す
    pub fn get(&self, r: GcRef) -> Option<HeapObject>;

    /// オフセットのオブジェクトのスロットを直接読み取る
    pub fn read_slot(&self, r: GcRef, slot_index: usize) -> Option<Value>;

    /// オフセットのオブジェクトのスロットに書き込む
    pub fn write_slot(&mut self, r: GcRef, slot_index: usize, value: Value) -> Result<(), String>;

    /// GC 実行
    pub fn collect(&mut self, roots: &[Value]);
}
```

### 既存 HeapObject の維持

```rust
/// 読み取り専用のビュー（一時構築）
pub struct HeapObject {
    pub marked: bool,
    pub slots: Vec<Value>,  // コピーして構築
}

impl HeapObject {
    /// 線形メモリからパースして構築
    pub fn from_memory(memory: &[u64], offset: usize) -> Option<Self>;

    // 既存メソッドは維持
    pub fn slots_to_string(&self) -> String;
    pub fn trace(&self) -> Vec<GcRef>;
}
```

### Free List 管理

- 解放されたブロックの先頭ワードに次の free block のオフセットを格納
- 0 は「次がない」を示す（オフセット0はヘッダ領域として予約）
- First-fit アルゴリズムで割り当て
- ブロックが要求より大きい場合は分割

### 制約

- 最小オブジェクトサイズ: 2 words（ヘッダ + 最低1スロット or next ptr）
- オフセット 0 は無効値として予約
- スロット数の上限: 2^32 - 1（ヘッダの32bit制限）

## 8. Open Questions
- なし

## 9. Acceptance Criteria（最大10個）

1. `Heap::new()` で `Vec<u64>` ベースの線形メモリが初期化される
2. `alloc_slots(slots)` で線形メモリ上にオブジェクトが確保され、オフセットが返る
3. `get(ref)` でオフセットから `HeapObject` が正しく構築される
4. `read_slot()` / `write_slot()` で個別スロットの読み書きができる
5. Free list が線形メモリ内に埋め込まれ、解放→再割り当てが動作する
6. GC の mark/sweep が線形メモリ上で正しく動作する
7. 既存の `HeapObject::slots_to_string()` が動作する
8. 既存の全テスト（`cargo test`）がパスする
9. `GcRef` がオフセットベースに変更されている
10. ドキュメント（`docs/vm.md`）が更新されている

## 10. Verification Strategy

### 進捗検証
- 各タスク完了時に `cargo test` を実行し、既存テストが壊れていないことを確認
- 新規追加のユニットテストで線形メモリ操作を検証

### 達成検証
- 全 Acceptance Criteria をチェックリストで確認
- `cargo test` が全てパス
- 手動でサンプルプログラムを実行し、ヒープ操作が正常に動作することを確認

### 漏れ検出
- 既存の snapshot テストがパスすることで、振る舞いの変更がないことを検証
- GC 関連のテストで、メモリリークがないことを確認

## 11. Test Plan

### E2E シナリオ 1: 基本的な alloc/get サイクル
```
Given: 空の Heap が初期化されている
When: 3スロットのオブジェクトを2つ確保する
Then: 両方のオブジェクトが get() で正しく取得できる
And: スロットの値が正しく読み取れる
```

### E2E シナリオ 2: Free list による再利用
```
Given: オブジェクト A, B, C が確保されている
When: B を GC で解放し、同じサイズの D を確保する
Then: D は B が使っていた領域に割り当てられる
And: A, C, D が全て正しく取得できる
```

### E2E シナリオ 3: GC による到達可能性トレース
```
Given: オブジェクト A が B への参照を持ち、B が C への参照を持つ
When: ルートを A のみにして GC を実行する
Then: A, B, C は全て生存している
When: ルートを空にして GC を実行する
Then: A, B, C は全て解放されている
```
