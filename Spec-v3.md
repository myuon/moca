# Spec-v3.md — Optimization & JIT

## 1. Goal

- **VM の実行性能を大幅に向上させる**
- Quickening と Inline Cache による Interpreter 高速化
- Baseline JIT による AArch64 ネイティブコード生成
- 並行 GC (Concurrent Mark) による停止時間短縮

## 2. Non-Goals

- x86_64 JIT（将来対応）
- Optimizing JIT（Tier 2）
- 投機的最適化 / Deoptimization
- Moving / Compacting GC
- M:N スケジューラ / ユーザーランドスレッド

## 3. Target Users

- パフォーマンスが重要なアプリケーション開発者
- mica を組み込みスクリプトとして使うホストアプリ開発者
- 長時間実行するサーバーサイドアプリケーション

## 4. Core User Flow

1. 通常通り `mica run app.mica` を実行
2. VM は最初 Interpreter で実行（Tier 0）
3. ホットな関数は自動的に JIT コンパイル（Tier 1）
4. JIT コードが実行され、性能向上
5. GC は並行で動作し、停止時間を最小化

## 5. Inputs & Outputs

### Inputs
- `.mica` ソースファイル
- `--jit=[on|off|auto]` オプション
- `--gc-mode=[stw|concurrent]` オプション

### Outputs
- 実行結果（v1/v2 と同じ）
- `--trace-jit` で JIT コンパイル情報
- `--gc-stats` で GC 統計

## 6. Tech Stack

| カテゴリ | 選定 |
|----------|------|
| JIT バックエンド | 自前コード生成（AArch64） |
| アセンブラ | 自前 or `dynasm-rs` |
| メモリ管理 | `mmap` / `mprotect` for executable memory |

## 7. Rules & Constraints

### 7.1 Tiered Execution

```
┌─────────────────────────────────────────────────────────┐
│                    Function Entry                        │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│  Tier 0: Bytecode Interpreter                           │
│  - 即座に実行開始                                        │
│  - 呼び出しカウンタを増加                                │
│  - ホット判定閾値に達したら JIT キューへ                 │
└─────────────────────────────────────────────────────────┘
                          │
                          │ hot (count >= threshold)
                          ▼
┌─────────────────────────────────────────────────────────┐
│  Tier 1: Baseline JIT (AArch64)                         │
│  - テンプレート方式でネイティブコード生成                │
│  - 関数エントリを JIT コードに差し替え                   │
│  - Safepoint / Stack Map を埋め込み                     │
└─────────────────────────────────────────────────────────┘
```

#### 閾値とトリガー
- デフォルト: 1000 回呼び出しで JIT 対象
- `--jit-threshold=<n>` で変更可能
- `--jit=off` で JIT 無効化（Interpreter のみ）

### 7.2 VM 最適化（Quickening）

#### Quickening とは
- 初回実行時に命令を特殊化
- 型プロファイルに基づいて高速パスを選択

#### 特殊化命令例
```
// 通常命令
ADD                     // 任意の Value 同士の加算

// 特殊化命令（Quickening 後）
ADD_SMI_SMI             // SMI + SMI の高速パス
ADD_FLOAT_FLOAT         // f64 + f64 の高速パス
ADD_STRING_STRING       // 文字列結合の高速パス
```

#### 実装
```rust
// 命令実行時に型をチェックして特殊化
fn execute_add(&mut self) {
    let b = self.pop();
    let a = self.pop();

    if a.is_smi() && b.is_smi() {
        // 次回から ADD_SMI_SMI を実行するよう書き換え
        self.quicken_to(Op::ADD_SMI_SMI);
        self.push(a.as_smi() + b.as_smi());
    } else {
        // 一般パス
        self.push(self.generic_add(a, b));
    }
}
```

### 7.3 Inline Cache

#### 用途
- プロパティアクセス（`obj.field`）
- メソッド呼び出し（`obj.method()`）
- 配列アクセス（型特殊化）

#### 構造
```rust
struct InlineCache {
    // Monomorphic cache
    cached_type_id: u32,
    cached_offset: u16,

    // 状態
    state: CacheState,  // Uninitialized, Monomorphic, Polymorphic, Megamorphic
}

enum CacheState {
    Uninitialized,
    Monomorphic,      // 1つの型のみ
    Polymorphic(4),   // 2-4 の型
    Megamorphic,      // 5以上、キャッシュ無効
}
```

#### プロパティアクセスの最適化
```rust
// READ_FIELD 命令の実装（IC あり）
fn execute_read_field(&mut self, field_name: &str, ic: &mut InlineCache) {
    let obj = self.peek();
    let type_id = obj.type_id();

    if ic.state == Monomorphic && ic.cached_type_id == type_id {
        // キャッシュヒット：オフセット直接アクセス
        let value = obj.read_field_at_offset(ic.cached_offset);
        self.replace_top(value);
    } else {
        // キャッシュミス：通常ルックアップ + キャッシュ更新
        let (value, offset) = obj.lookup_field(field_name);
        ic.update(type_id, offset);
        self.replace_top(value);
    }
}
```

### 7.4 Baseline JIT (AArch64)

#### コード生成戦略
- テンプレート方式（命令ごとに固定パターン）
- レジスタ割り当ては単純（スタックベース維持）
- 最適化は最小限（将来の Tier 2 で対応）

#### AArch64 レジスタ規約
```
x0-x7   : 引数 / 戻り値
x8      : indirect result location
x9-x15  : caller-saved temporaries
x16-x17 : intra-procedure-call scratch (IP0, IP1)
x18     : platform register (reserved)
x19-x28 : callee-saved
x29     : frame pointer (FP)
x30     : link register (LR)
sp      : stack pointer
```

#### mica JIT での使用
```
x19     : VM state pointer
x20     : Value stack pointer
x21     : locals base pointer
x22     : constants pool pointer
x23-x28 : 空き（将来用）
```

#### コード生成例（ADD_SMI_SMI）
```asm
// スタックから2値をポップして加算、結果をプッシュ
ldr x0, [x20, #-8]!     // pop a
ldr x1, [x20, #-8]!     // pop b
// タグチェック（SMI の下位ビットは 001）
and x2, x0, #0x7
and x3, x1, #0x7
orr x2, x2, x3
cmp x2, #0x2            // 両方 SMI か？
b.ne slow_path
// SMI 加算（タグを維持）
add x0, x0, x1
sub x0, x0, #1          // タグ調整
str x0, [x20], #8       // push result
```

#### Safepoint
```asm
// Safepoint: GC がスレッドを停止できるポイント
safepoint:
    ldr x0, [x19, #VM_GC_PENDING_OFFSET]
    cbz x0, continue
    bl gc_safepoint_handler
continue:
```

### 7.5 Stack Map

#### 目的
- GC が JIT スタック上の参照を特定
- 正確な GC のために必須

#### 構造
```rust
struct StackMap {
    // PC → スタックスロットの参照 bitmap
    entries: Vec<StackMapEntry>,
}

struct StackMapEntry {
    native_pc: u32,          // JIT コードのオフセット
    bytecode_pc: u32,        // 対応する Bytecode PC
    stack_slots: BitVec,     // 参照スロットの bitmap
    locals_slots: BitVec,    // 参照 locals の bitmap
}
```

### 7.6 Concurrent Mark GC

#### フェーズ
```
1. Initial Mark (STW, 短時間)
   - ルートから直接参照されるオブジェクトをマーク

2. Concurrent Mark (並行)
   - ヒープを走査してマーク
   - Write Barrier でミューテータの変更を追跡

3. Remark (STW, 短時間)
   - Concurrent Mark 中の変更を処理

4. Concurrent Sweep (並行)
   - 未マークオブジェクトを解放
```

#### Write Barrier
```rust
// オブジェクトフィールドへの書き込み時
fn write_barrier(obj: *mut Object, field: usize, new_value: Value) {
    if gc.is_marking() && new_value.is_ptr() {
        // Snapshot-at-the-beginning barrier
        let old_value = obj.fields[field];
        if old_value.is_ptr() && !old_value.is_marked() {
            gc.mark_gray(old_value);
        }
    }
    obj.fields[field] = new_value;
}
```

#### JIT コード内の Write Barrier
```asm
write_field:
    // Barrier check
    ldr x3, [x19, #VM_GC_MARKING_OFFSET]
    cbz x3, no_barrier

    // Old value check
    ldr x4, [x0, x1, lsl #3]  // old value
    and x5, x4, #0x7
    cbnz x5, no_barrier       // not a pointer

    // Mark gray
    bl gc_mark_gray

no_barrier:
    str x2, [x0, x1, lsl #3]  // write new value
```

### 7.7 OS スレッド対応

#### モデル
- 各スレッドに独立した VM インスタンス
- ヒープは共有（GC は全スレッドを停止）
- スレッド間通信は Channel（v3 で最小実装）

#### API
```
// スレッド生成
let handle = spawn(fn() {
    // 新しいスレッドで実行
    heavy_computation();
});

// 結果待ち
let result = handle.join();

// Channel
let (tx, rx) = channel();
spawn(fn() {
    tx.send(42);
});
let value = rx.recv();
```

## 8. Open Questions

- JIT コンパイルの非同期化（バックグラウンドコンパイル）
- Deoptimization の必要性（投機的最適化なしなら不要？）
- Concurrent Mark の Write Barrier オーバーヘッド測定

## 9. Acceptance Criteria

1. Quickening により頻出命令が特殊化される
2. Inline Cache によりプロパティアクセスが高速化される
3. ホットな関数が自動的に JIT コンパイルされる
4. JIT コードが正しく実行される（Interpreter と同じ結果）
5. JIT コード実行中に GC が正しく動作する
6. Concurrent Mark により STW 時間が短縮される
7. Write Barrier が正しく動作する
8. `--jit=off` で JIT を無効化できる
9. `--trace-jit` で JIT コンパイル情報が出力される
10. マイクロベンチマークで v1 比 3x 以上の性能向上

## 10. Test Plan

### E2E Test 1: JIT 基本動作

**Given:** 以下の内容の `jit_test.mica`
```
fn sum(n) {
    let mut total = 0;
    let mut i = 0;
    while i < n {
        total = total + i;
        i = i + 1;
    }
    return total;
}

// JIT 閾値を超える呼び出し
let mut j = 0;
while j < 2000 {
    sum(100);
    j = j + 1;
}
print(sum(100));
```

**When:** `mica run --trace-jit jit_test.mica` を実行

**Then:**
- stdout に `4950` が出力される
- stderr に `[JIT] Compiling: sum` のようなログが出る

### E2E Test 2: Concurrent GC

**Given:** 以下の内容の `gc_stress.mica`
```
fn allocate_many() {
    let mut i = 0;
    while i < 100000 {
        let arr = [i, i + 1, i + 2];
        i = i + 1;
    }
}

allocate_many();
print("done");
```

**When:** `mica run --gc-mode=concurrent --gc-stats gc_stress.mica` を実行

**Then:**
- stdout に `done` が出力される
- stderr に GC 統計（pause time < 10ms）が出る

### E2E Test 3: マルチスレッド

**Given:** 以下の内容の `thread_test.mica`
```
let (tx, rx) = channel();

let h1 = spawn(fn() {
    let mut sum = 0;
    let mut i = 0;
    while i < 1000 {
        sum = sum + i;
        i = i + 1;
    }
    tx.send(sum);
});

let result = rx.recv();
h1.join();
print(result);
```

**When:** `mica run thread_test.mica` を実行

**Then:** stdout に `499500` が出力される
