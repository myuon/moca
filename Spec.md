# Spec.md - 組み込み型関数のメソッド化

## 1. Goal
- 組み込み型（Vector, HashMap, string, array）の関数を、メソッド呼び出し構文 `v.push(x)` で使えるようにする

## 2. Non-Goals
- 言語仕様の変更（新しい構文やキーワードの追加）
- ジェネリクスやオーバーロードの導入
- 既存の型システムの変更
- mutability の明示的な指定の導入

## 3. Target Users
- moca言語のユーザー
- より自然なオブジェクト指向的な記法でコードを書きたい開発者

## 4. Core User Flow
1. ユーザーが `let v = vec_new();` でベクターを作成
2. `v.push(10);` でメソッド呼び出し構文で要素を追加
3. `v.len()` で長さを取得
4. `let m = map_new_any();` でマップを作成
5. `m.put_string("key", "value");` で要素を追加
6. `m.get_string("key")` で要素を取得

## 5. Inputs & Outputs

### 変換前（現在）
```moca
let v = vec_new();
vec_push(v, 10);
print(vec_len(v));

let m = map_new_any();
map_put_string(m, "key", "value");
print(map_get_string(m, "key"));
```

### 変換後（目標）
```moca
let v = vec_new();
v.push(10);
print(v.len());

let m = map_new_any();
m.put_string("key", "value");
print(m.get_string("key"));
```

## 6. Tech Stack
- 言語: Rust
- テスト: cargo test
- stdlib: moca言語 (`std/prelude.mc`)

## 7. Rules & Constraints

### 実装方式
- **VectorAny, HashMapAny 等の既存構造体に impl ブロックでメソッドを追加**
- コンパイラの組み込み関数（`vec_push` 等）を削除し、メソッド呼び出しに統一
- 既存の `vec_push_any`, `map_put_string` 等の内部関数は impl 内メソッドとして再実装

### 命名規則
- サフィックスは維持: `put_string`, `put_int`, `get_string`, `get_int`
- プレフィックス（`vec_`, `map_`）は削除

### 対象メソッド一覧

#### Vector (`VectorAny`)
| 現在の関数 | メソッド名 |
|-----------|----------|
| `vec_new()` | `vec_new()` (コンストラクタは関数のまま) |
| `vec_with_capacity(n)` | `vec_with_capacity(n)` (コンストラクタは関数のまま) |
| `vec_push(v, x)` | `v.push(x)` |
| `vec_pop(v)` | `v.pop()` |
| `vec_get(v, i)` | `v.get(i)` |
| `vec_set(v, i, x)` | `v.set(i, x)` |
| `vec_len(v)` | `v.len()` |
| `vec_capacity(v)` | `v.capacity()` |

#### HashMap (`HashMapAny`)
| 現在の関数 | メソッド名 |
|-----------|----------|
| `map_new_any()` | `map_new_any()` (コンストラクタは関数のまま) |
| `map_put_string(m, k, v)` | `m.put_string(k, v)` |
| `map_get_string(m, k)` | `m.get_string(k)` |
| `map_has_string(m, k)` | `m.has_string(k)` |
| `map_contains_string(m, k)` | `m.contains_string(k)` |
| `map_remove_string(m, k)` | `m.remove_string(k)` |
| `map_put_int(m, k, v)` | `m.put_int(k, v)` |
| `map_get_int(m, k)` | `m.get_int(k)` |
| `map_has_int(m, k)` | `m.has_int(k)` |
| `map_contains_int(m, k)` | `m.contains_int(k)` |
| `map_remove_int(m, k)` | `m.remove_int(k)` |
| `map_len(m)` | `m.len()` |
| `map_size(m)` | `m.size()` |
| `map_keys(m)` | `m.keys()` |
| `map_values(m)` | `m.values()` |

#### String / Array
| 現在の関数 | メソッド名 |
|-----------|----------|
| `len(s)` (string) | `s.len()` |
| `len(a)` (array) | `a.len()` |

### 削除対象（コンパイラ組み込み）
- `vec_push`, `vec_pop`, `vec_get`, `vec_set`, `vec_len`, `vec_capacity`
- `push`, `pop` (汎用)
- `map_*` 系の組み込み関数（もしあれば）

### 互換性
- 既存の関数形式は **削除**（後方互換性なし）
- 既存テストはすべてメソッド形式に書き換え

## 8. Open Questions
- `len()` を string/array でメソッドとして呼び出すには、コンパイラでの特別扱いが必要（struct ではないため）。実装時に要検討。

## 9. Acceptance Criteria（最大10個）

1. `v.push(x)` でベクターに要素を追加できる
2. `v.pop()` でベクターから要素を取り出せる
3. `v.get(i)` でベクターの要素を取得できる
4. `v.set(i, x)` でベクターの要素を設定できる
5. `v.len()` でベクターの長さを取得できる
6. `m.put_string(k, v)` でマップに要素を追加できる
7. `m.get_string(k)` でマップから要素を取得できる
8. `m.contains_string(k)` でマップのキー存在確認ができる
9. 既存の `vec_push(v, x)` 形式はコンパイルエラーになる
10. `cargo test` が全てパスする

## 10. Verification Strategy

### 進捗検証
- 各メソッド実装後に対応するスナップショットテストを実行
- `cargo check` でコンパイルエラーがないことを確認

### 達成検証
- 全 Acceptance Criteria をチェックリストで確認
- `cargo test` が全てパス
- `cargo clippy` で警告がないことを確認

### 漏れ検出
- 既存のテストファイルを全てメソッド形式に変換し、テストがパスすることで網羅性を担保
- `vec_push`, `map_put_string` 等の旧関数名で grep し、残存がないことを確認

## 11. Test Plan

### E2E シナリオ 1: Vector 基本操作
**Given**: 空の moca プログラム
**When**: 以下のコードを実行
```moca
let v = vec_new();
v.push(10);
v.push(20);
print(v.len());
print(v.get(0));
v.set(0, 30);
print(v.pop());
```
**Then**: 出力が `2`, `10`, `20` となる

### E2E シナリオ 2: HashMap 基本操作
**Given**: 空の moca プログラム
**When**: 以下のコードを実行
```moca
let m = map_new_any();
m.put_string("name", "Alice");
print(m.get_string("name"));
print(m.contains_string("name"));
print(m.len());
```
**Then**: 出力が `Alice`, `true`, `1` となる

### E2E シナリオ 3: 旧構文がエラーになる
**Given**: 空の moca プログラム
**When**: `vec_push(v, 10);` を含むコードをコンパイル
**Then**: コンパイルエラーが発生する
