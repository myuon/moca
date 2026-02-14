# Spec: Index/IndexAssign の Desugar

## 概要

`vec[i]`や`vec[i] = value`などのindex構文をdesugarフェーズで`get`/`set`メソッド呼び出しに変換し、Vec/Mapなどの型で統一的にインデックスアクセスを実現する。

## 対象

| 型 | index (`x[i]`) | index assign (`x[i] = v`) |
|---|---|---|
| `Vec<T>` | `x.get(i)` に変換 | `x.set(i, v)` に変換 |
| `Map<K,V>` | `x.get(key)` に変換 | `x.set(key, v)` に変換 |
| `Array<T>` | **変換しない**（従来のcodegen） | **変換しない**（従来のcodegen） |
| `Vector<T>` (legacy) | `x.get(i)` に変換 | `x.set(i, v)` に変換 |

## 使用例

```mc
// Vec<T>
let v: Vec<int> = new Vec<int> {1, 2, 3};
print(v[0]);      // -> v.get(0) にdesugar -> 1
v[0] = 10;        // -> v.set(0, 10) にdesugar
print(v[0]);      // -> v.get(0) にdesugar -> 10

// Map<K,V>
let m: Map<string, int> = new Map<string, int> {"a": 1, "b": 2};
print(m["a"]);    // -> m.get("a") にdesugar -> 1
m["a"] = 10;      // -> m.set("a", 10) にdesugar
print(m["a"]);    // -> m.get("a") にdesugar -> 10

// Array<T> - desugar対象外
let arr: Array<int, 3> = [1, 2, 3];
print(arr[0]);    // 直接codegenで処理 -> 1
arr[0] = 10;      // 直接codegenで処理
print(arr[0]);    // -> 10
```

## 実装詳細

### コンパイラパイプライン

```
parser → typechecker → desugar → monomorphise → resolver → codegen
                         ↑
                   型情報を使用
```

desugarフェーズはtypechecker後に実行され、`index_object_types`（Spanをキーとした型情報のHashMap）を受け取る。

### AST変換

**Index式 (`Expr::Index`)**:
```
// Before desugar
Expr::Index { object: vec, index: i, ... }

// After desugar (Vec/Map)
Expr::MethodCall { object: vec, method: "get", args: [i], ... }
```

**IndexAssign文 (`Statement::IndexAssign`)**:
```
// Before desugar
Statement::IndexAssign { object: vec, index: i, value: v, ... }

// After desugar (Vec/Map)
Statement::Expr {
    expr: Expr::MethodCall { object: vec, method: "set", args: [i, v], ... }
}
```

### 型チェック

typecheckerは以下の型チェックを行う:

| 型 | indexの型 | valueの型 |
|---|---|---|
| `Vec<T>` | `int` | `T` |
| `Map<K,V>` | `K` | `V` |
| `Array<T>` | `int` | `T` |

### 標準ライブラリメソッド

`std/prelude.mc`で定義:

**Vec<T>**:
- `fun get(self, index: int) -> T`
- `fun set(self, index: int, value: T)`

**Map<K,V>**:
- `fun get(self, key: any) -> any` - `type_of`でキー型を判定して`get_int`/`get_string`にディスパッチ
- `fun set(self, key: any, val: any)` - `put`のエイリアス

## 関連ドキュメント

- [Collection Literals (new構文)](spec-collection-literals.md) - `new Vec<T> {...}` 構文の仕様
