# Spec.md - CLI Debug Dump Options

## 1. Goal
- `mica run`コマンドに`--dump-ast`、`--dump-resolved`、`--dump-bytecode`オプションを追加し、コンパイラパイプラインの中間表現を人間可読形式で出力できるようにする

## 2. Non-Goals
- Tokens（字句解析結果）のダンプは対象外
- JSON等の機械可読形式での出力
- 新規コマンド（`mica dump`等）の追加
- JITコンパイル結果のダンプ

## 3. Target Users
- mica言語の開発者（コンパイラデバッグ用途）
- mica言語を学習中のユーザー（内部動作の理解用途）

## 4. Core User Flow
1. ユーザーが`mica run example.mica --dump-ast`を実行
2. ASTが標準エラー出力にPretty Print形式で出力される
3. 通常通りプログラムが実行される
4. （オプション）`--dump-ast=output.txt`形式でファイルに出力可能

## 5. Inputs & Outputs

### Inputs
- ソースファイル（.mica）
- ダンプオプション（`--dump-ast`, `--dump-resolved`, `--dump-bytecode`）
- 出力先指定（オプション、`--dump-ast=path`形式）

### Outputs

#### --dump-ast
型情報付きのAST（抽象構文木）
```
Program
├── FnDef: add(a: Int, b: Int) -> Int
│   └── Body
│       └── BinaryOp: Add : Int
│           ├── Var: a : Int
│           └── Var: b : Int
└── Statement: Expr
    └── Call: add(1, 2) : Int
```

#### --dump-resolved
名前解決済みプログラム（スロット番号付き）
```
ResolvedProgram
├── Function[0]: add
│   ├── Params: [a -> slot:0, b -> slot:1]
│   └── Body
│       └── BinaryOp: Add
│           ├── LoadLocal(slot:0) : Int
│           └── LoadLocal(slot:1) : Int
└── Main
    └── Call(func:0, args:[1, 2]) : Int
```

#### --dump-bytecode
バイトコード逆アセンブリ
```
== Function: add ==
0000: LoadLocal 0      ; a
0002: LoadLocal 1      ; b
0004: AddInt
0005: Ret

== Main ==
0000: PushInt 1
0002: PushInt 2
0004: Call 0, 2        ; add(2 args)
0007: Pop
0008: PushNil
0009: Ret
```

## 6. Tech Stack
- 言語: Rust (edition 2024)
- CLIパーサー: clap v4.5
- 出力実装: `std::fmt::Display` trait
- テスト: cargo test（既存e2e.rsに追加）

## 7. Rules & Constraints

### 振る舞いルール
- ダンプオプションは複数同時指定可能（`--dump-ast --dump-bytecode`）
- ダンプは実行前に行い、その後通常実行を継続
- ダンプ出力先はデフォルトでstderr（実行結果のstdoutと分離）
- ファイル指定時は`--dump-ast=path`形式で指定

### 出力順序
複数指定時の出力順序: AST → Resolved → Bytecode（パイプライン順）

### 技術的制約
- 型情報はTypeChecker実行後に取得するため、型エラーがある場合はAST出力時に型情報が不完全になる可能性あり
- ResolvedProgramとBytecodeは型チェック成功後のみ出力可能

### エラー時の挙動
- パース失敗時: エラーメッセージのみ出力、ダンプなし
- 型チェック失敗時: `--dump-ast`は出力可能、`--dump-resolved`と`--dump-bytecode`は出力不可

## 8. Open Questions
なし

## 9. Acceptance Criteria

1. `mica run example.mica --dump-ast`でASTがstderrに出力される
2. `mica run example.mica --dump-resolved`でResolvedProgramがstderrに出力される
3. `mica run example.mica --dump-bytecode`でバイトコードがstderrに出力される
4. ダンプ後もプログラムが正常に実行される
5. `--dump-ast --dump-bytecode`のように複数オプション同時指定が動作する
6. `--dump-ast=output.txt`形式でファイル出力ができる
7. AST出力に型情報（`: Int`等）が含まれる
8. Bytecode出力に行番号とオペコード名が含まれる
9. 既存の`--trace-jit`や`--gc-stats`オプションと併用可能
10. `mica run --help`にダンプオプションの説明が表示される

## 10. Test Plan

### E2E Test 1: 基本的なダンプ出力
```
Given: examples/arith.mica が存在する
When: `mica run examples/arith.mica --dump-ast` を実行
Then:
  - stderrに "Program" を含むAST出力がある
  - プログラムが正常終了する（exit code 0）
```

### E2E Test 2: 複数オプション同時指定
```
Given: examples/arith.mica が存在する
When: `mica run examples/arith.mica --dump-ast --dump-bytecode` を実行
Then:
  - stderrにAST出力がある
  - stderrにBytecode出力がある（"==" セクション区切りを含む）
  - AST出力がBytecode出力より前に出現する
```

### E2E Test 3: ファイル出力
```
Given: examples/arith.mica が存在する
When: `mica run examples/arith.mica --dump-ast=/tmp/ast.txt` を実行
Then:
  - /tmp/ast.txt が作成される
  - ファイル内容に "Program" を含むAST出力がある
  - stderrにはAST出力がない
```
