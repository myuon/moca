# Spec.md - Vector型の実装

## 1. Goal
- moca言語にVector型（動的配列）を追加し、`ArrayPush`/`ArrayPop`/`ArrayLen` VMプリミティブを削除してコンパイラ側でVector操作を実装する

## 2. Non-Goals
- 固定長配列の削除（既存の `[1,2,3]` 構文は維持）
- ジェネリクスや型パラメータの導入
- イテレータの実装
- スライス操作

## 3. Target Users
- moca言語のユーザー（動的にサイズが変わる配列が必要な場面）

## 4. Core User Flow
1. `Vec::new()` または `Vec::with_capacity(n)` でVectorを生成
2. `vec.push(value)` で要素を追加
3. `vec.pop()` で要素を取り出し
4. `vec[i]` でインデックスアクセス
5. `vec[i] = value` でインデックス代入
6. `len(vec)` で長さ取得
7. `vec.capacity()` で容量取得

## 5. Inputs & Outputs

### 入力
- Vector生成: `Vec::new()`, `Vec::with_capacity(n)`
- 要素追加: `vec.push(value)`
- 要素取り出し: `vec.pop()`
- インデックス: `vec[i]`, `vec[i] = value`

### 出力
- Vectorオブジェクト（ヒープ上）
- 各操作の戻り値

## 6. Tech Stack
- 言語: Rust
- 変更対象ファイル:
  - `src/vm/ops.rs` - ArrayPush/ArrayPop/ArrayLen削除、StrLen追加
  - `src/vm/heap.rs` - MocaVector型追加
  - `src/vm/vm.rs` - Vector用VM操作削除
  - `src/vm/bytecode.rs` - シリアライズ更新
  - `src/compiler/codegen.rs` - Vector操作のコード生成
  - `src/compiler/parser.rs` - Vec::new()構文パース
  - `src/compiler/resolver.rs` - Vector組み込み関数解決
  - `src/compiler/typechecker.rs` - Vector型チェック
- テスト: cargo test + snapshot tests

## 7. Rules & Constraints

### メモリレイアウト
```
Vector本体（3スロット）:
┌─────────────────┬─────────────┬─────────────┬─────────────┐
│  ObjectHeader   │     ptr     │     len     │     cap     │
│  (MocaVector)   │   (GcRef)   │    (i64)    │    (i64)    │
└─────────────────┴─────────────┴─────────────┴─────────────┘
      slot 0          slot 1        slot 2        slot 3

データ領域（Slots型、ptrが指す先）:
┌─────────────────┬─────────────┬─────────────┬─────────────┬─────┐
│  ObjectHeader   │   slot 0    │   slot 1    │   slot 2    │ ... │
│   (MocaSlots)   │   (Value)   │   (Value)   │   (Value)   │     │
└─────────────────┴─────────────┴─────────────┴─────────────┴─────┘
```

### 成長戦略
- 容量不足時: 現在の容量の2倍に拡張（最小4）
- 初期容量: 0（最初のpushで4に拡張）

### Vector操作のコンパイル
| moca構文 | コンパイル結果 |
|---------|---------------|
| `Vec::new()` | `PushNull; PushInt 0; PushInt 0; AllocHeap 3` (ptr=null, len=0, cap=0) |
| `Vec::with_capacity(n)` | `AllocHeap(n); PushInt 0; PushInt n; AllocHeap 3` |
| `vec.push(val)` | 組み込み関数呼び出し（容量チェック・拡張含む） |
| `vec.pop()` | 組み込み関数呼び出し（境界チェック含む） |
| `vec[i]` | `HeapLoad 0`(ptr取得) → `HeapLoadDyn`(データアクセス) |
| `vec[i] = v` | `HeapLoad 0`(ptr取得) → `HeapStoreDyn`(データ書き込み) |
| `len(vec)` | `HeapLoad 1` (Vectorのlenフィールド) |
| `vec.capacity()` | `HeapLoad 2` (capフィールド) |

### 固定長配列のlen()
| moca構文 | コンパイル結果 |
|---------|---------------|
| `len(arr)` | `HeapLoad 0` (Slotsのslot[0]に格納されたlen) |

※ 固定長配列は `[len, elem0, elem1, ...]` のレイアウト（前回実装済み）

### 文字列のlen()
| moca構文 | コンパイル結果 |
|---------|---------------|
| `len(str)` | `StrLen` (新規プリミティブ) |

※ 文字列は別のメモリレイアウトのため、専用プリミティブが必要

### GC対応
- MocaVector型を追加（ObjectType::Vector）
- trace()でptrフィールド（slot 0）をトレース
- データ領域は既存のSlots型を使用（既にGC対応済み）

### 境界チェック
- インデックスアクセス: 0 <= index < len
- pop: len > 0 を確認
- 違反時はランタイムエラー

### 組み込み関数として実装するもの
- `vec_push(vec, value)` - push操作（容量拡張ロジック含む）
- `vec_pop(vec)` - pop操作（境界チェック含む）

これらは複雑なロジックを含むためVMプリミティブではなく組み込み関数として実装。

## 8. Open Questions
なし

## 9. Acceptance Criteria

1. `Vec::new()` で空のVectorが生成できる
2. `Vec::with_capacity(n)` で指定容量のVectorが生成できる
3. `vec.push(value)` で要素を追加できる
4. `vec.pop()` で末尾要素を取り出せる
5. `vec[i]` でインデックスアクセスできる
6. `vec[i] = value` でインデックス代入できる
7. `len(vec)` でVectorの長さを取得できる
8. `vec.capacity()` でVectorの容量を取得できる
9. `ArrayPush` / `ArrayPop` / `ArrayLen` がops.rsから削除されている
10. 全既存テスト（219 unit + 10 snapshot）がパスする

## 10. Verification Strategy

### 進捗検証
- 各フェーズ完了時に `cargo check` でコンパイル確認
- 各フェーズ完了時に `cargo test` でテスト通過確認

### 達成検証
- 全Acceptance Criteriaをチェックリストで確認
- Vector操作のバイトコード出力を目視確認

### 漏れ検出
- 既存のpush/pop使用テストがVector構文に書き換えられているか確認
- `grep -r "ArrayPush\|ArrayPop\|ArrayLen"` で残存コードがないか確認

## 11. Test Plan

### E2E シナリオ 1: Vector基本操作
```
Given: 空のプログラム
When: 以下を実行
  let v = Vec::new()
  v.push(1)
  v.push(2)
  v.push(3)
  print(len(v))
  print(v[1])
  print(v.pop())
  print(len(v))
Then: 出力が "3", "2", "3", "2" となる
```

### E2E シナリオ 2: Vector容量拡張
```
Given: 空のプログラム
When: 以下を実行
  let v = Vec::with_capacity(2)
  print(v.capacity())
  v.push(1)
  v.push(2)
  v.push(3)  // 容量拡張発生
  print(v.capacity())
  print(len(v))
Then: 出力が "2", "4", "3" となる（容量が2倍に拡張）
```

### E2E シナリオ 3: Vectorインデックス代入
```
Given: 空のプログラム
When: 以下を実行
  let v = Vec::new()
  v.push(10)
  v.push(20)
  v.push(30)
  v[1] = 99
  print(v[0])
  print(v[1])
  print(v[2])
Then: 出力が "10", "99", "30" となる
```
