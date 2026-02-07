# Spec.md — JIT: HeapLoad/HeapStore/HeapLoadDyn/HeapStoreDyn ネイティブ対応

## 1. Goal
- JITコンパイラが `HeapLoad(n)` / `HeapStore(n)` / `HeapLoadDyn` / `HeapStoreDyn` をネイティブコードに変換できるようにする
- これにより、`@inline` で `Vec::get`/`Vec::set` を展開したループがJITコンパイル可能になる

## 2. Non-Goals
- `HeapAlloc` / `HeapAllocDyn`（ヒープアロケーション）のJIT対応
- GCセーフポイントの追加（Heap読み書きはアロケーションを発生させないため不要）
- 境界チェックの追加（インタプリタと同じくチェックなし）
- JIT内からのVM呼び出しヘルパー方式（アプローチBはフォールバック時のみ）

## 3. Target Users
- moca言語のユーザーが、配列・ベクター操作を含むホットループの高速化の恩恵を受ける

## 4. Core User Flow
1. ユーザーが `Vec::get` / `Vec::set` に `@inline` を付与（stdlibで設定済みにする）
2. ホットループ内で `counts[idx]` 等のベクターアクセスがインライン展開され、`Call` が `HeapLoad`/`HeapLoadDyn`/`HeapStoreDyn` に置き換わる
3. ループJITコンパイラがこれらのOpをネイティブコードに変換
4. ループ全体がネイティブ実行される

## 5. Inputs & Outputs
- **入力**: HeapLoad/HeapStore/HeapLoadDyn/HeapStoreDyn を含むバイトコード
- **出力**: ヒープメモリを直接読み書きするネイティブ機械語（x86_64 / aarch64）

## 6. Tech Stack
- 言語: Rust（既存プロジェクト）
- 対象: `src/jit/compiler.rs` (aarch64), `src/jit/compiler_x86_64.rs` (x86_64)
- テスト: `cargo test`（既存パフォーマンステスト + スナップショットテスト）

## 7. Rules & Constraints

### 7.1 ヒープメモリレイアウト
- ヒープは `Vec<u64>` の線形メモリ
- オブジェクトレイアウト: `[Header(1word) | Tag0 | Val0 | Tag1 | Val1 | ...]`
- 各スロットは2ワード（tag: u64 + payload: u64）= 16バイト
- スロットNのアドレス: `heap_base + (ref_index + 1 + 2*N) * 8`

### 7.2 JitCallContext の heap_base
- `JitCallContext.heap_base` (offset 48) にヒープメモリのベースポインタが格納済み
- ループ内に `Call` がない場合、GC/アロケーションは発生しないので `heap_base` は安定

### 7.3 各Opのセマンティクス

**HeapLoad(n)** — 静的フィールド読み込み
- スタック: `[ref] → [value]`
- ref の payload がヒープインデックス
- `heap_base[ref + 1 + 2*n]` から tag、`heap_base[ref + 1 + 2*n + 1]` から payload を読む
- tag + payload をスタックにプッシュ

**HeapStore(n)** — 静的フィールド書き込み
- スタック: `[ref, value] → []`
- value の tag/payload を `heap_base[ref + 1 + 2*n]` / `heap_base[ref + 1 + 2*n + 1]` に書く

**HeapLoadDyn** — 動的インデックス読み込み
- スタック: `[ref, index] → [value]`
- index の payload を整数としてスロット番号に使用
- `heap_base[ref + 1 + 2*index]` から tag、`+1` から payload

**HeapStoreDyn** — 動的インデックス書き込み
- スタック: `[ref, index, value] → []`
- `heap_base[ref + 1 + 2*index]` に tag、`+1` に payload を書く

### 7.4 ネイティブコード生成パターン（x86_64 の例）

```
; HeapLoad(n): [ref] → [value]
; ref を VSTACK からポップ
sub VSTACK, 16
mov TMP0, [VSTACK + 8]           ; ref payload (heap index)
mov TMP1, [VM_CTX + 48]          ; heap_base
; アドレス計算: heap_base + (ref + 1 + 2*n) * 8
add TMP0, (1 + 2*n)
shl TMP0, 3                       ; * sizeof(u64)
add TMP1, TMP0
; tag + payload を読み込み
mov TMP2, [TMP1]                  ; tag
mov TMP3, [TMP1 + 8]             ; payload
; VSTACK にプッシュ
mov [VSTACK], TMP2
mov [VSTACK + 8], TMP3
add VSTACK, 16
```

### 7.5 フォールバック戦略
- 基本はネイティブ実装（アプローチA）で進める
- 実装上の障壁が発生した場合のみ、VMヘルパー方式（アプローチB）にフォールバックする

### 7.6 stdlib の @inline 化
- `Vec<T>::get` と `Vec<T>::set` に `@inline` を付与する（`std/prelude.mc`）
- これにより、ベクターアクセスがループ内でインライン展開され、HeapLoad系Opに変わる

### 7.7 text_counting.mc の更新
- `to_letter_index` に `@inline` を付与
- パフォーマンステスト（`snapshot_performance`）が to_letter_index の関数JITではなくループJITで動作することを確認

## 8. Acceptance Criteria

1. `HeapLoad(n)` がJITコンパイラでネイティブコードに変換される（x86_64 + aarch64）
2. `HeapStore(n)` がJITコンパイラでネイティブコードに変換される（x86_64 + aarch64）
3. `HeapLoadDyn` がJITコンパイラでネイティブコードに変換される（x86_64 + aarch64）
4. `HeapStoreDyn` がJITコンパイラでネイティブコードに変換される（x86_64 + aarch64）
5. `Vec<T>::get` / `Vec<T>::set` に `@inline` が付与され、ループ内で展開される
6. text_counting.mc のホットループがJITコンパイルされる（`--trace-jit` でコンパイル成功を確認）
7. text_counting.mc の実行結果が変わらない（出力一致）
8. `snapshot_performance` テストがパスする（JITコンパイル発生 + 出力一致）
9. `cargo fmt && cargo check && cargo test && cargo clippy` が全てパスする

## 9. Verification Strategy
- **進捗検証**: 各Op実装後に `cargo check && cargo test` でリグレッションなしを確認
- **達成検証**: `cargo run -- run tests/snapshots/performance/text_counting.mc --trace-jit` でループJITコンパイル成功を確認
- **漏れ検出**: 全4 Opのネイティブ実装 + 全テストパスで確認

## 10. Test Plan

### Test 1: text_counting ループJITコンパイル成功
```
Given: to_letter_index に @inline、Vec::get/Vec::set に @inline を付与した text_counting.mc
When: --trace-jit で実行する
Then: "[JIT] Compiled loop in 'count_chars'" が出力され、実行結果が元と同一
```

### Test 2: snapshot_performance テスト通過
```
Given: 全パフォーマンステスト
When: cargo test snapshot_performance を実行
Then: text_counting を含む全テストが通過（JITコンパイルが1回以上発生）
```

### Test 3: 全テスト通過
```
Given: Vec::get/Vec::set に @inline を追加した状態
When: cargo test を実行
Then: 全テストが通過（@inline 化によるリグレッションなし）
```
