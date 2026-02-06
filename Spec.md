# Spec.md — MicroOp変換機構の導入

## 1. Goal
- 既存のスタックベースVM（Op bytecode）からレジスタベースのMicroOpへの変換層を導入し、インタプリタの実行性能を改善する
- MicroOp PCを唯一の真実とし、将来のJIT移行の橋渡しとなる基盤を作る

## 2. Non-Goals
- JITコンパイラの変更（既存のOp→native変換はそのまま維持）
- SSA形式への変換や本格的な最適化パス
- 新しい言語機能の追加
- GC/StackMapの完全な再設計（Safepointは入れるが、正確なRef追跡は後回しでOK）

## 3. Target Users
- moca VMの内部改善。エンドユーザーから見た動作変更はない
- 将来JITをMicroOpベースに移行する際の基盤となる

## 4. Core User Flow
（内部アーキテクチャ変更のため、ユーザーフローの変更なし）

1. moca ソースをコンパイル → 既存の Op bytecode が生成される（変更なし）
2. VM が関数を初めて実行する際に、Op[] → MicroOp[] への変換を lazy に実行
3. 変換結果をキャッシュし、以降は MicroOp インタプリタで実行
4. 変換に失敗した場合は既存のスタックベースインタプリタにフォールバック

## 5. Inputs & Outputs
- **入力**: 既存の `Vec<Op>` bytecode（Function 単位）
- **出力**: `Vec<MicroOp>` + メタデータ（temps数、PC マッピング）
- **副作用**: 実行結果は既存と完全に同一

## 6. Tech Stack
- 言語: Rust (edition 2024)
- 既存クレート内に新モジュールとして追加（外部依存なし）
- テスト: cargo test（既存 snapshot tests + performance tests）

## 7. Rules & Constraints

### 7.1 MicroOp PC が唯一の真実
- 変換時に `old_pc (Op index) → new_pc (MicroOp index)` のマッピング表を作成
- 全ての branch target は MicroOp index に解決してから格納
- Raw fallback に含まれる Op が branch を持つ場合でも、target は MicroOp index

### 7.2 制御フロー命令は必ず MicroOp 側
- `Jmp`, `BrIf`, `BrIfFalse`, `Call`, `Ret` は必ずネイティブ MicroOp として変換
- Raw fallback に制御フロー系を残さない

### 7.3 未対応 Op は Raw でラップ
- MicroOp に変換できない Op は `Raw { op }` でラップ
- Raw op はスタックベースのセマンティクスをそのまま維持
- レジスタ⇔スタック間の橋渡しには `StackPush` / `StackPop` を使用

### 7.4 Lazy 変換
- 関数の MicroOp 変換は初回呼び出し時に実行
- 変換結果は VM インスタンス内にキャッシュ

### 7.5 パフォーマンス
- 各 performance_test で既存比 ±5% 以内（速度低下なし）
- 可能であれば改善を目指す

## 8. Architecture Design

### 8.1 VReg（仮想レジスタ）
```
VReg(usize)  // フレームのレジスタファイルへのインデックス
```

レジスタファイルのレイアウト（1フレームあたり）:
```
[0 .. locals_count)            → 宣言済みlocals（引数を含む）
[locals_count .. locals_count + temps_count) → 一時レジスタ
```

実行時は既存の `self.stack` を流用:
- `VReg(n)` = `stack[frame.stack_base + n]`
- フレーム開始時に `locals_count + temps_count` 分のスロットを確保
- Raw op 用のオペランドスタックはその上に積む

### 8.2 MicroOp 命令セット

#### 制御フロー（常にネイティブ）
| 命令 | フィールド | 説明 |
|------|-----------|------|
| `Jmp` | `{ target }` | 無条件ジャンプ（target は MicroOp PC） |
| `BrIf` | `{ cond: VReg, target }` | cond が truthy なら分岐 |
| `BrIfFalse` | `{ cond: VReg, target }` | cond が falsy なら分岐 |
| `Call` | `{ func_id, args: Vec<VReg>, ret: Option<VReg> }` | 関数呼び出し |
| `Ret` | `{ src: Option<VReg> }` | 値を返す（None = unit） |

#### Move / 定数
| 命令 | フィールド | 説明 |
|------|-----------|------|
| `Mov` | `{ dst, src }` | vreg 間コピー |
| `ConstI64` | `{ dst, imm: i64 }` | 即値ロード |
| `ConstI32` | `{ dst, imm: i32 }` | 即値ロード（I64 に拡張して格納） |
| `ConstF64` | `{ dst, imm: f64 }` | 即値ロード |

#### 整数 ALU（i64）
| 命令 | フィールド | 説明 |
|------|-----------|------|
| `AddI64` | `{ dst, a, b }` | dst = a + b |
| `SubI64` | `{ dst, a, b }` | dst = a - b |
| `MulI64` | `{ dst, a, b }` | dst = a * b |
| `DivI64` | `{ dst, a, b }` | dst = a / b |
| `RemI64` | `{ dst, a, b }` | dst = a % b |
| `NegI64` | `{ dst, src }` | dst = -src |

#### 浮動小数点 ALU（f64）
| 命令 | フィールド | 説明 |
|------|-----------|------|
| `AddF64` | `{ dst, a, b }` | dst = a + b |
| `SubF64` | `{ dst, a, b }` | dst = a - b |
| `MulF64` | `{ dst, a, b }` | dst = a * b |
| `DivF64` | `{ dst, a, b }` | dst = a / b |
| `NegF64` | `{ dst, src }` | dst = -src |

#### 比較（分離型）
| 命令 | フィールド | 説明 |
|------|-----------|------|
| `CmpI64` | `{ dst, a, b, cond: CmpCond }` | dst = (a cond b) ? 1 : 0 |
| `CmpI64Imm` | `{ dst, a, imm: i64, cond: CmpCond }` | dst = (a cond imm) ? 1 : 0 |
| `CmpF64` | `{ dst, a, b, cond: CmpCond }` | dst = (a cond b) ? 1 : 0 |

```
enum CmpCond { Eq, Ne, LtS, LeS, GtS, GeS }
```

#### スタック橋渡し（Raw op 用）
| 命令 | フィールド | 説明 |
|------|-----------|------|
| `StackPush` | `{ src: VReg }` | vreg の値をオペランドスタックに push |
| `StackPop` | `{ dst: VReg }` | オペランドスタックから pop して vreg に格納 |

#### フォールバック
| 命令 | フィールド | 説明 |
|------|-----------|------|
| `Raw` | `{ op: Op }` | 既存のスタックベース実行にフォールバック |

### 8.3 変換パイプライン

```
Op[] bytecode
  │
  ▼
Phase 1: PC マッピング構築
  - Op[] を走査し、各 Op の MicroOp 先頭位置を記録
  - 1 Op → 1+ MicroOps（Raw なら 1:1、レジスタ変換すると bridge 含め N）
  │
  ▼
Phase 2: スタックシミュレーション + VReg 割り当て
  - 仮想スタック（Vec<VReg>）を維持
  - push 操作 → 新しい temp VReg を割り当て、仮想スタックに積む
  - pop 操作 → 仮想スタックから VReg を取得
  - LocalGet(n) → VReg(n) を仮想スタックに積む
  - LocalSet(n) → 仮想スタックから pop、Mov { dst: VReg(n), src } を生成
  │
  ▼
Phase 3: MicroOp 生成
  - 対応済み Op → レジスタベース MicroOp を emit
  - 未対応 Op → StackPush（消費する VReg）+ Raw { op } + StackPop（生成する VReg）
  - 制御フロー → ネイティブ MicroOp（target は Phase 1 のマッピングで解決）
  │
  ▼
ConvertedFunction { micro_ops: Vec<MicroOp>, temps_count: usize }
```

### 8.4 MicroOp インタプリタの実行モデル

```
loop {
    let frame = frames.last_mut();
    let converted = get_converted(frame.func_index);
    if frame.pc >= converted.micro_ops.len() { break; }

    let mop = &converted.micro_ops[frame.pc];
    frame.pc += 1;

    match mop {
        MicroOp::AddI64 { dst, a, b } => {
            let va = stack[frame.stack_base + a.0].as_i64();
            let vb = stack[frame.stack_base + b.0].as_i64();
            stack[frame.stack_base + dst.0] = Value::I64(va + vb);
        }
        MicroOp::Raw { op } => {
            // 既存の execute_op を呼び出す
            self.execute_op(op.clone(), chunk)?;
        }
        MicroOp::Call { func_id, args, ret } => {
            // 引数を新フレームの先頭にコピー
            let callee = &chunk.functions[*func_id];
            let callee_converted = get_or_convert(*func_id);
            let new_stack_base = stack.len();
            // locals + temps 分のスロットを確保
            stack.resize(new_stack_base + callee.locals_count + callee_converted.temps_count, Value::Null);
            // 引数をコピー
            for (i, arg) in args.iter().enumerate() {
                stack[new_stack_base + i] = stack[frame.stack_base + arg.0];
            }
            frames.push(MicroOpFrame { func_index: *func_id, pc: 0, stack_base: new_stack_base, ret_vreg: *ret });
        }
        // ... 他の命令
    }
}
```

### 8.5 Call / Ret の詳細

**Call 実行時**:
1. callee の MicroOp を取得（lazy 変換）
2. `stack` に callee のレジスタファイル分（`locals_count + temps_count`）を確保
3. 引数を caller の VReg → callee の locals[0..argc] にコピー
4. 新しいフレームを push（ret_vreg を記録）

**Ret 実行時**:
1. 戻り値を callee の VReg から取得
2. フレームを pop、stack を truncate
3. caller の ret_vreg に戻り値を格納

### 8.6 performance_test が必要とする Op

| テスト | 必要な Op | 備考 |
|--------|----------|------|
| fibonacci | I64Const, LocalGet/Set, I64LeS, BrIfFalse, Ret, I64Sub, I64Add, Call | 再帰。全てレジスタ変換可能 |
| sum_loop | I64Const, LocalGet/Set, I64LeS, BrIfFalse, Jmp, I64Add | 単純ループ |
| nested_loop | 上記 + I64Mul, I64LtS | 二重ループ |
| mutual_recursion | 上記 + I64Eq, I64RemS, Call | 相互再帰 |
| mandelbrot | 上記 + F64Const, F64Add/Sub/Mul/Div, F64Gt, I64LtS | 浮動小数点 |

→ 設計した MicroOp セットで全 performance_test をカバー可能。
→ print (Syscall) 等はホットパス外なので Raw で十分。

## 9. Acceptance Criteria（最大10個）

1. `cargo test` が全て通る（既存の snapshot tests 含む）
2. 各 performance_test で既存比 ±5% 以内の性能を維持する
3. MicroOp 変換は lazy（関数の初回実行時に変換、結果をキャッシュ）
4. 制御フロー命令（Jmp, BrIf, BrIfFalse, Call, Ret）は常にネイティブ MicroOp
5. Branch target は全て MicroOp PC に解決されている
6. 未対応 Op は Raw { op } でラップされ、実行セマンティクスが変わらない
7. レジスタベース MicroOp が i64/f64 の算術・比較・定数をカバーする
8. 変換に失敗した関数は既存のスタックインタプリタにフォールバック可能
9. performance_test の全出力が既存と完全一致する

## 10. Verification Strategy

- **進捗検証**: 各実装フェーズ完了時に `cargo test` を実行。Phase 1 完了時点で全テスト通過を確認
- **達成検証**: 全 Acceptance Criteria をチェックリストで確認。performance_test の実行時間を baseline と比較
- **漏れ検出**: 既存 snapshot tests（basic, errors, jit, modules, ffi, performance）が全てパスすることで出力の正しさを担保。`cargo clippy` でコード品質を確認

### ベースライン性能（release build）

| ベンチマーク | 時間 |
|---|---|
| fibonacci(35) | 0.197s |
| sum_loop | 0.061s |
| nested_loop | 0.060s |
| mutual_recursion | 0.306s |
| mandelbrot | 0.072s |

## 11. Test Plan

### Scenario 1: 基本的な MicroOp 変換と実行
- **Given**: fibonacci.mc がコンパイルされている
- **When**: MicroOp インタプリタで fib(35) を実行する
- **Then**: 出力が `9227465` で、既存インタプリタと一致する

### Scenario 2: 全 performance_test の正しさと性能
- **Given**: 5 つの performance_test が全てコンパイルされている
- **When**: MicroOp インタプリタで各テストを実行する
- **Then**: 全出力が既存と一致し、実行時間が baseline の ±5% 以内

### Scenario 3: Raw fallback の正しさ
- **Given**: ヒープ操作や Syscall を含むプログラムがある
- **When**: MicroOp 変換で Raw ラップされた Op を含む関数を実行する
- **Then**: 既存インタプリタと同じ出力が得られる

## 12. Implementation Phases

### Phase 1: インフラ + Raw フォールバック
- MicroOp enum、VReg 型、ConvertedFunction 構造体の定義
- Op[] → MicroOp[] 変換（制御フローのみネイティブ、残りは Raw）
- PC マッピング構築とブランチターゲット解決
- MicroOp インタプリタループの実装
- VM への lazy 変換の統合とキャッシュ
- **期待**: 全テスト通過、性能変化なし〜微減

### Phase 2: レジスタベース変換
- スタックシミュレーションによる VReg 割り当てアルゴリズム
- 算術（I64/F64）、定数、比較、Mov のレジスタベース変換
- StackPush/StackPop による Raw op との橋渡し
- **期待**: performance_test で性能改善（特に再帰系）

### Phase 3: 検証とチューニング
- 全テスト通過の確認
- performance_test のベースライン比較
- 必要に応じてチューニング
