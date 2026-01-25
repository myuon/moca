# CLI Debug Dump Options

コンパイラパイプラインの中間表現を出力するCLIオプション。

## オプション

| オプション | 説明 |
|-----------|------|
| `--dump-ast` | AST（抽象構文木）を出力 |
| `--dump-resolved` | 名前解決済みプログラムを出力 |
| `--dump-bytecode` | バイトコード逆アセンブリを出力 |

## 使用方法

```bash
# stderrに出力
mica run example.mica --dump-ast

# ファイルに出力
mica run example.mica --dump-ast=output.txt

# 複数同時指定
mica run example.mica --dump-ast --dump-bytecode
```

## 出力形式

### --dump-ast

```
Program
├── FnDef: add(a: int, b: int) -> int
│   └── Return
│       └── Binary: +
│           ├── Ident: a
│           └── Ident: b
└── Expr
    └── Call: add(2)
        ├── Int: 1
        └── Int: 2
```

### --dump-resolved

```
ResolvedProgram
Functions:
└── [0] add(a -> slot:0, b -> slot:1) [locals: 2]
    └── Return
        └── Binary(+)
            ├── Local(slot:0)
            └── Local(slot:1)
Main:
└── Expr
    └── Call func:0 args:2
        ├── Int(1)
        └── Int(2)
```

### --dump-bytecode

```
== Function[0]: add (arity: 2, locals: 2) ==
0000: LoadLocal 0
0001: LoadLocal 1
0002: Add
0003: Ret

== Main ==
0000: PushInt 1
0001: PushInt 2
0002: Call 0, 2 ; add
0003: Pop
0004: PushNil
0005: Ret
```

## 振る舞い

- ダンプ出力先はデフォルトでstderr
- ダンプ後もプログラムは通常実行される
- 複数指定時の出力順序: AST → Resolved → Bytecode（パイプライン順）
- パース失敗時はダンプなし
- 型チェック失敗時は`--dump-ast`のみ出力可能
