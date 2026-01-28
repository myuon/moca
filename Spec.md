# Spec.md - AArch64 JIT Call命令サポート

## 1. Goal
- aarch64 JITでOp::Call命令をサポートし、相互再帰（is_even ↔ is_odd）がJITコンパイルされて正しく動作する

## 2. Non-Goals
- CallBuiltin（組み込み関数呼び出し）
- 可変長引数のサポート
- クロージャ呼び出し
- 自己再帰最適化（emit_call_self）は今回スコープ外
- 末尾呼び出し最適化

## 3. Target Users
- mocaコンパイラの開発者
- aarch64（Apple Silicon Mac等）でJITを使用するユーザー

## 4. Core User Flow
1. ユーザーが相互再帰を含むmocaコードを実行
2. JITが関数をコンパイル（Call命令を含む）
3. JITコードがjit_call_helper経由で他の関数を呼び出し
4. 正しい結果が返される

## 5. Inputs & Outputs
### Inputs
- Op::Call(func_index, argc) バイトコード命令
- 引数の値（VSTACKから取得）

### Outputs
- ターゲット関数の戻り値（VSTACKに格納）
- JitReturn構造体（tag, payload）

## 6. Tech Stack
- 言語: Rust
- アーキテクチャ: AArch64 (ARM64)
- アセンブラ: 既存のAArch64Assembler（src/jit/asm_aarch64.rs）
- テスト: cargo test --features jit

## 7. Rules & Constraints

### 呼び出し規約（AArch64 AAPCS64準拠）
- 引数: x0, x1, x2, x3, ... （最初の8引数）
- 戻り値: x0（tag）, x1（payload）
- Callee-saved: x19-x28, x29(fp), x30(lr)
- Caller-saved: x0-x18

### jit_call_helper引数
- x0: ctx (*mut JitCallContext)
- x1: func_index (u64)
- x2: argc (u64)
- x3: args (*const JitValue)

### レジスタ使用規則（既存のregs定義に従う）
- VM_CTX (x19): VMコンテキストポインタ
- VSTACK (x20): 値スタックポインタ
- LOCALS (x21): ローカル変数ベースポインタ
- TMP0/TMP1 (x9/x10): 一時レジスタ

### 実装制約
- 既存のemit_prologue/emit_epilogueと整合性を保つ
- スタックは16バイトアラインメントを維持
- x86_64のemit_call_externalのロジックをaarch64に変換

## 8. Open Questions
なし

## 9. Acceptance Criteria
1. [x] compiler.rsにemit_call関数が実装されている
2. [x] Op::Call(func_index, argc)がcompile_opで処理される
3. [x] jit_call_helperが正しい引数で呼び出される
4. [x] 戻り値がVSTACKに正しく格納される
5. [x] スタックの深さ(stack_depth)が正しく更新される
6. [x] `cargo test --features jit jit_mutual_recursion`が通る
7. [x] is_even/is_oddの両方がJITコンパイルされる（trace-jitで確認可能）
8. [x] 結果が正しい（is_even(0)=1, is_even(1)=0, is_even(10)=1, is_even(11)=0）

## 10. Verification Strategy

### 進捗検証
- emit_call実装後、単純な関数呼び出しのバイトコードを手動テスト
- `cargo run --features jit -- run --jit on --trace-jit <test.mc>` でJITトレース確認

### 達成検証
- `cargo test --features jit jit_mutual_recursion` が通る
- トレース出力で両関数がJITコンパイルされていることを確認

### 漏れ検出
- x86_64のemit_call_externalと比較し、同等の機能が実装されているか確認
- カバレッジ: 引数0個/1個/複数個のテストケース

## 11. Test Plan

### E2E Test 1: 基本的な相互再帰
```
Given: is_even/is_oddの相互再帰関数が定義されている
When: is_even(10)を実行（JIT有効、threshold=1）
Then:
  - 両関数がJITコンパイルされる
  - 結果が1（10は偶数）
```

### E2E Test 2: 複数回の呼び出し
```
Given: is_even/is_oddの相互再帰関数が定義されている
When: is_even(0), is_even(1), is_even(10), is_even(11)を順に実行
Then:
  - 結果がそれぞれ1, 0, 1, 0
  - パニックやクラッシュが発生しない
```

### E2E Test 3: 既存テストとの互換性
```
Given: JITフィーチャーが有効
When: cargo test --features jit を実行
Then:
  - jit_mutual_recursionがPASS
  - 他のJITテストが壊れていない
```

## 12. TODO List

### Setup
- [x] x86_64のemit_call_external実装を確認・理解

### Core
- [x] emit_call関数を実装（jit_call_helper呼び出し）
- [x] compile_opにOp::Callのハンドリングを追加
- [x] 引数のVSTACK→レジスタ渡しを実装
- [x] 戻り値のVSTACKへの格納を実装

### Test & Polish
- [x] jit_mutual_recursionテストを実行して確認
- [x] trace-jitで両関数のJITコンパイルを確認
