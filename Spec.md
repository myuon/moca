# Spec.md - HashMap Standard Library Implementation

## 1. Goal
- moca言語のstdライブラリにHashMap（キー・バリュー型のデータ構造）を追加し、ユーザーがO(1)でデータの格納・取得ができるようにする

## 2. Non-Goals
- イテレーション機能（全要素の走査）は今回のスコープ外
- intキーのサポートは今回のスコープ外（stringキーのみ）
- HashDoS対策などのセキュリティ機能
- カスタムハッシュ関数の指定機能

## 3. Target Users
- moca言語でキー・バリュー形式のデータを扱いたい開発者
- 辞書型のデータ構造を使いたいユーザー

## 4. Core User Flow
1. `hashmap_new()` でHashMapを作成
2. `hashmap_put(map, "key", value)` でキー・バリューを格納
3. `hashmap_get(map, "key")` で値を取得
4. `hashmap_contains(map, "key")` でキーの存在確認
5. `hashmap_remove(map, "key")` でエントリを削除
6. `hashmap_len(map)` で要素数を取得

## 5. Inputs & Outputs

### Inputs
- キー: `string` 型
- バリュー: `any` 型（任意の値）

### Outputs
- `hashmap_new()` → HashMap構造体
- `hashmap_get()` → 格納された値（存在しない場合は0/nil）
- `hashmap_contains()` → `bool`
- `hashmap_len()` → `int`

## 6. Tech Stack
- **実装言語**: Moca (std/prelude.mc)
- **ヒープ操作**: VM intrinsics (`__alloc_heap`, `__heap_load`, `__heap_store`)
- **ハッシュ関数**: DJB2アルゴリズム（Moca言語で実装）
- **テスト**: スナップショットテスト (tests/snapshots/basic/)

## 7. Rules & Constraints

### データ構造
```
HashMap: { buckets: int, size: int, capacity: int }
  - buckets: Entry配列へのポインタ
  - size: 現在の要素数
  - capacity: バケット数

Entry: { key: string, value: any, next: int }
  - key: キー文字列
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

### ハッシュ関数 (DJB2)
```
hash = 5381
for each char c in string:
    hash = ((hash << 5) + hash) + c  // hash * 33 + c
return hash
```

### 制約
- キーは `string` 型のみ
- 存在しないキーへのgetは0を返す（エラーではない）
- キーが既に存在する場合、putは値を上書き

## 8. Open Questions
なし（全て仮決めで確定）

## 9. Acceptance Criteria

1. `hashmap_new()` で空のHashMapが作成できる
2. `hashmap_put(map, key, value)` でキー・バリューを格納できる
3. `hashmap_get(map, key)` で格納した値を取得できる
4. `hashmap_contains(map, key)` でキーの存在確認ができる
5. `hashmap_remove(map, key)` でエントリを削除できる
6. `hashmap_len(map)` で要素数を取得できる
7. 同一キーへのputは値を上書きする
8. ハッシュ衝突時も正しく値を格納・取得できる
9. 17個以上の要素を追加してもリサイズにより正常動作する
10. `cargo test` が全てパスする

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

## 11. Test Plan

### E2E シナリオ 1: 基本操作
```
Given: 空のHashMap
When: put("name", "Alice"), put("age", 30) を実行
Then: get("name") == "Alice", get("age") == 30, len == 2
```

### E2E シナリオ 2: 衝突と上書き
```
Given: HashMapに ("key1", 100) を格納済み
When: 同一バケットに衝突する別キーを追加、その後 put("key1", 200) で上書き
Then: 両方のキーが正しく取得でき、key1の値は200
```

### E2E シナリオ 3: リサイズ
```
Given: 空のHashMap（初期容量16）
When: 20個のエントリを追加
Then: 全てのエントリが正しく取得でき、len == 20
```
