# Spec.md - Map Standard Library Implementation

## 1. Goal
- moca言語のstdライブラリにMap（キー・バリュー型のデータ構造）を追加し、ユーザーがO(1)でデータの格納・取得ができるようにする

## 2. Non-Goals
- HashDoS対策などのセキュリティ機能
- カスタムハッシュ関数の指定機能
- 既存機能への移行（新規追加のみ）

## 3. Target Users
- moca言語でキー・バリュー形式のデータを扱いたい開発者
- 辞書型のデータ構造を使いたいユーザー

## 4. Core User Flow
1. `map_new()` でMapを作成
2. `map_put(m, key, value)` でキー・バリューを格納
3. `map_get(m, key)` で値を取得
4. `map_contains(m, key)` でキーの存在確認
5. `map_remove(m, key)` でエントリを削除
6. `map_len(m)` で要素数を取得
7. イテレーション機能で全要素を走査

## 5. Inputs & Outputs

### Inputs
- キー: `int` 型 または `string` 型（map型でキーを明示）
- バリュー: `any` 型（任意の値）

### Outputs
- `map_new()` → Map構造体
- `map_get()` → 格納された値（存在しない場合は0/nil）
- `map_contains()` → `bool`
- `map_len()` → `int`

## 6. Tech Stack
- **実装言語**: Moca (std/prelude.mc)
- **ヒープ操作**: VM intrinsics (`__alloc_heap`, `__heap_load`, `__heap_store`)
- **ハッシュ関数**:
  - int: 値をそのまま使用（modulo bucket数）
  - string: DJB2アルゴリズム（Moca言語で実装）
- **テスト**: スナップショットテスト (tests/snapshots/basic/)

## 7. Rules & Constraints

### データ構造
```
Map: { buckets: int, size: int, capacity: int }
  - buckets: Entry配列へのポインタ
  - size: 現在の要素数
  - capacity: バケット数

Entry: { key: any, value: any, next: int }
  - key: キー（int or string）
  - value: 格納値
  - next: 次のEntryへのポインタ（Chaining用、0なら終端）
```

### 衝突解決
- Chaining（連結リスト方式）を使用
- 同一バケットに複数エントリがある場合、線形に探索

### リサイズ
- 負荷率（load factor）が0.75を超えたら自動拡張
- 初期バケット数: 16
- 拡張時: バケット数を2倍にしてrehash

### ハッシュ関数
```
# int の場合
hash = key

# string の場合 (DJB2)
hash = 5381
for each char c in string:
    hash = ((hash << 5) + hash) + c  // hash * 33 + c
return hash
```

### 制約
- キーは `int` 型または `string` 型
- 存在しないキーへのgetは0を返す（エラーではない）
- キーが既に存在する場合、putは値を上書き

### イテレーション
- `map_keys(m)` - 全キーの配列を返す
- `map_values(m)` - 全バリューの配列を返す
- または `map_foreach(m, callback)` 形式

## 8. Open Questions
なし

## 9. Acceptance Criteria

1. `map_new()` で空のMapが作成できる
2. `map_put(m, key, value)` でキー・バリューを格納できる（int/string両対応）
3. `map_get(m, key)` で格納した値を取得できる
4. `map_contains(m, key)` でキーの存在確認ができる
5. `map_remove(m, key)` でエントリを削除できる
6. `map_len(m)` で要素数を取得できる
7. 同一キーへのputは値を上書きする
8. ハッシュ衝突時も正しく値を格納・取得できる
9. 17個以上の要素を追加してもリサイズにより正常動作する
10. イテレーション機能で全要素を走査できる

## 10. Verification Strategy

### 進捗検証
- 各関数実装後に対応するスナップショットテストを追加・実行
- `cargo test` で既存テストが壊れていないことを確認

### 達成検証
- 全Acceptance Criteriaをスナップショットテストでカバー
- `cargo fmt && cargo check && cargo test && cargo clippy` が全てパス

### 漏れ検出
- 衝突ケースのテスト（同一バケットに複数エントリ）
- リサイズのテスト（17個以上の要素）
- エッジケース（空のmap、存在しないキー）
- intキーとstringキー両方のテスト

## 11. Test Plan

### E2E シナリオ 1: 基本操作（stringキー）
```
Given: 空のMap
When: map_put(m, "name", "Alice"), map_put(m, "age", 30) を実行
Then: map_get(m, "name") == "Alice", map_get(m, "age") == 30, map_len(m) == 2
```

### E2E シナリオ 2: 基本操作（intキー）
```
Given: 空のMap
When: map_put(m, 1, "one"), map_put(m, 2, "two") を実行
Then: map_get(m, 1) == "one", map_get(m, 2) == "two", map_len(m) == 2
```

### E2E シナリオ 3: リサイズとイテレーション
```
Given: 空のMap（初期容量16）
When: 20個のエントリを追加
Then: 全てのエントリが正しく取得でき、map_len(m) == 20、イテレーションで全要素が取得できる
```
