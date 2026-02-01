# Inline Assembly (asm block)

VM命令列を直接mocaコードに記述できる機能です。デバッグや特殊なチューニング用途に使用します。

## 構文

### 基本形

```moca
asm {
    __emit("PushInt", 42);
    __emit("Print");
};
```

### 入力変数あり

変数をスタックにpushしてからasm命令を実行します。

```moca
let x = 10;
asm(x) {
    __emit("PushInt", 5);
    __emit("Add");
    __emit("Print");  // 15を出力
}
```

### 入力・出力あり

戻り値型を指定すると、ブロック終了時のスタックトップを返します。

```moca
let x = 10;
let result = asm(x) -> i64 {
    __emit("PushInt", 5);
    __emit("Add");
};
print(result);  // 15
```

### 複数入力

左から右の順でスタックにpushされます。

```moca
let a = 3;
let b = 4;
let sum = asm(a, b) -> i64 {
    // stack: [a=3, b=4] (top=b)
    __emit("Add");
};
print(sum);  // 7
```

## 組み込み関数

| 関数 | 用途 |
|------|------|
| `__emit("Op", args...)` | VM命令を発行 |
| `__safepoint()` | GCセーフポイントを挿入 |
| `__gc_hint(size)` | 次の割り当てサイズをヒント |

### __emit

VM命令を発行します。第1引数は命令名（文字列）、以降は命令の引数です。

```moca
__emit("PushInt", 42);      // 整数をpush
__emit("PushFloat", 3.14);  // 浮動小数点をpush
__emit("PushString", "hello"); // 文字列をpush
__emit("Add");              // 引数なしの命令
__emit("GetL", 0);          // ローカル変数slot 0を取得
```

### __safepoint

GCセーフポイントを挿入します。デフォルトではasmブロック内でGCは発動しませんが、`__safepoint()`を呼び出した箇所でGCが許可されます。

```moca
asm(x) {
    __emit("PushInt", 1);
    __emit("Add");
    __safepoint();  // ここでGC許可
    __emit("PushInt", 2);
    __emit("Add");
}
```

### __gc_hint

次の割り当てサイズをヒントとして提供します。

```moca
asm {
    __gc_hint(1024);
    __emit("AllocArray", 100);
}
```

## 使用可能な命令

### 定数・スタック操作

| 命令 | 引数 | 説明 |
|------|------|------|
| `PushInt` | i64 | 整数をpush |
| `PushFloat` | f64 | 浮動小数点をpush |
| `PushTrue` | - | trueをpush |
| `PushFalse` | - | falseをpush |
| `PushNull` | - | nullをpush |
| `PushString` | string | 文字列をpush |
| `Pop` | - | スタックトップを破棄 |
| `Dup` | - | スタックトップを複製 |

### ローカル変数

| 命令 | 引数 | 説明 |
|------|------|------|
| `GetL` | slot | ローカル変数をpush |
| `SetL` | slot | スタックトップをローカル変数に保存 |

### 算術演算

| 命令 | 説明 |
|------|------|
| `Add` | 加算 |
| `Sub` | 減算 |
| `Mul` | 乗算 |
| `Div` | 除算 |
| `Mod` | 剰余 |
| `Neg` | 符号反転 |

### 比較演算

| 命令 | 説明 |
|------|------|
| `Eq` | 等価 |
| `Ne` | 非等価 |
| `Lt` | 小なり |
| `Le` | 小なりイコール |
| `Gt` | 大なり |
| `Ge` | 大なりイコール |

### 論理演算

| 命令 | 説明 |
|------|------|
| `Not` | 論理否定 |

### 制御フロー

| 命令 | 引数 | 説明 |
|------|------|------|
| `Jmp` | target | 無条件ジャンプ |
| `JmpIfFalse` | target | 条件付きジャンプ（false時） |
| `JmpIfTrue` | target | 条件付きジャンプ（true時） |

### ヒープ・オブジェクト

| 命令 | 引数 | 説明 |
|------|------|------|
| `New` | n | n個のキー・値ペアからオブジェクト作成 |
| `GetF` | field | フィールドを取得 |
| `SetF` | field | フィールドに値を設定 |

### 配列操作

| 命令 | 引数 | 説明 |
|------|------|------|
| `AllocArray` | n | n要素の配列を割り当て |
| `ArrayLen` | - | 配列の長さを取得 |
| `ArrayGet` | - | 配列要素にアクセス |
| `ArraySet` | - | 配列要素に値を設定 |
| `ArrayPush` | - | 配列に要素を追加 |
| `ArrayPop` | - | 配列から要素を削除 |

### 型操作

| 命令 | 説明 |
|------|------|
| `TypeOf` | 型名を文字列にして取得 |
| `ToString` | 値を文字列に変換 |
| `ParseInt` | 文字列をintに変換 |

### 例外処理

| 命令 | 引数 | 説明 |
|------|------|------|
| `Throw` | - | 例外をスロー |
| `TryBegin` | target | try開始（catchへのジャンプターゲット） |
| `TryEnd` | - | try終了 |

### ビルトイン

| 命令 | 説明 |
|------|------|
| `Print` | 標準出力に出力 |
| `GcHint` | 次の割り当てサイズのヒント |

### スレッド操作

| 命令 | 引数 | 説明 |
|------|------|------|
| `ThreadSpawn` | func_index | スレッドを生成 |
| `ChannelCreate` | - | チャネルを作成 |
| `ChannelSend` | - | チャネルに送信 |
| `ChannelRecv` | - | チャネルから受信 |
| `ThreadJoin` | - | スレッドの終了を待機 |

## 禁止命令

以下の命令はasmブロック内では使用できません：

| 命令 | 理由 |
|------|------|
| `Call` | 関数境界を壊す可能性がある |
| `Ret` | 関数境界を壊す可能性がある |

## 安全性

asmブロックは通常のmocaコードと同様にランタイムチェックが行われます：

- スタックアンダーフロー検出
- 型エラー検出
- 不正な命令名はコンパイルエラー

## 戻り値型

`-> type` で指定可能な型：

| 型 | 説明 |
|------|------|
| `i64` | 64bit整数 |
| `f64` | 64bit浮動小数点 |
| `bool` | ブール値 |
| `string` | 文字列 |
| `array` | 配列 |
| `nil` | null値 |

## 使用例

### カスタム算術演算

```moca
fun fast_add3(a: i64, b: i64, c: i64) -> i64 {
    return asm(a, b, c) -> i64 {
        __emit("Add");
        __emit("Add");
    };
}

print(fast_add3(1, 2, 3));  // 6
```

### スタック操作

```moca
let x = 10;
asm(x) {
    __emit("Dup");      // [10, 10]
    __emit("Add");      // [20]
    __emit("Print");    // 20を出力
}
```

### 配列の直接操作

```moca
let arr = [1, 2, 3];
asm(arr) {
    __emit("PushInt", 0);    // index
    __emit("ArrayGet");      // arr[0]
    __emit("Print");         // 1を出力
}
```
