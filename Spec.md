# Spec.md - VM Inline Assembly Feature

## 1. Goal
- mocaコード内にVM命令列を直接記述し、デバッグや特殊なチューニングを可能にする

## 2. Non-Goals
- JITコンパイラへの直接介入（既存JITはそのまま）
- 新しいVM命令の追加（既存118命令を使用）
- 型システムの拡張（asmブロックの型チェックは最小限）
- マクロシステムの実装

## 3. Target Users
- moca言語/VM開発者: デバッグ、テスト、内部動作確認用
- 上級mocaユーザー: パフォーマンスチューニング、低レベル最適化用

## 4. Core User Flow
1. ユーザーがmocaソースコード内に `asm { ... }` ブロックを記述
2. ブロック内で `__emit("OpName", args...)` を使ってVM命令を指定
3. 必要に応じて `__safepoint()` でGCセーフポイントを挿入
4. コンパイラがasmブロックを対応するVM命令列に変換
5. VMが通常通り実行（ランタイムチェック付き）

## 5. Inputs & Outputs

### 入力
- moca変数をasmブロックの引数として渡す: `asm(x, y) { ... }`
- `__emit` の引数としてリテラル値を渡す: `__emit("PushInt", 42)`

### 出力
- 戻り値型を指定して最後のスタックトップを返す: `let r = asm(x) -> i64 { ... }`
- 戻り値なしの場合はスタックを消費しない

## 6. Tech Stack
- 言語: Rust（既存）
- パーサー: 既存パーサー拡張（`asm`ブロック構文追加）
- コンパイラ: 既存コンパイラ拡張（`__emit`等を命令に変換）
- テスト: snapshot_tests（`tests/snapshots/asm/`に追加）

## 7. Rules & Constraints

### 構文ルール
```moca
// 基本形
asm {
    __emit("PushInt", 42);
    __emit("Print");
}

// 入力あり
asm(var1, var2) {
    __emit("Add");
}

// 入力・出力あり
let result = asm(x) -> i64 {
    __emit("PushInt", 5);
    __emit("Add");
};
```

### 組み込み関数
| 関数 | 用途 |
|------|------|
| `__emit("Op", args...)` | VM命令を発行 |
| `__safepoint()` | GCセーフポイントを挿入 |
| `__gc_hint(size)` | 次の割り当てサイズをヒント |

### GC制御
- asmブロック内ではデフォルトでGCは発動しない（`__safepoint()`呼び出し箇所のみGC許可）
- `__safepoint()`がない場合、ブロック全体がアトミックに実行される

### 変数バインディング
- `asm(a, b)` で変数a, bをスタックにpush（左から右の順）
- `-> type` 指定時、ブロック終了時のスタックトップを戻り値として返す
- 戻り値型: `i64`, `f64`, `bool`, `string`, `array`, `object`, `null`

### ランタイムチェック（安全性）
- スタックアンダーフロー検出
- 型エラー検出（期待する型と異なる場合）
- 不正な命令名はコンパイルエラー
- 不正な引数はコンパイルエラー

### 制約
- asmブロック内では通常のmoca文は書けない（`__emit`等の組み込み関数のみ）
- ジャンプ命令（`Jmp`, `JmpIfTrue`, `JmpIfFalse`）のターゲットはasmブロック内に限定
- `Call`/`Ret`命令は使用禁止（関数境界を壊す可能性）

## 8. Open Questions
- なし（実装中に発生した場合は追記）

## 9. Acceptance Criteria

1. [ ] `asm { __emit("PushInt", 42); __emit("Print"); }` で42が出力される
2. [ ] `let r = asm(x) -> i64 { __emit("PushInt", 1); __emit("Add"); };` でx+1が返る
3. [ ] 複数入力 `asm(a, b)` でa, bがスタックに正しくpushされる
4. [ ] `__safepoint()` を挿入した箇所でGCが発動可能になる
5. [ ] `__safepoint()` がない場合、ブロック内でGCは発動しない
6. [ ] 不正な命令名（例: `__emit("InvalidOp")`）でコンパイルエラーになる
7. [ ] スタックアンダーフロー時にランタイムエラーになる
8. [ ] `-> i64` 指定時にスタックトップがi64でなければランタイムエラーになる
9. [ ] `Jmp`命令でasmブロック外へのジャンプはコンパイルエラーになる
10. [ ] `Call`/`Ret`命令はコンパイルエラーになる

## 10. Verification Strategy

### 進捗検証
- 各フェーズ完了時にsnapshotテストを実行し、期待する出力を確認
- パーサー完了時: asmブロックのAST出力を確認
- コンパイラ完了時: 生成されるVM命令列を確認

### 達成検証
- 全Acceptance Criteriaをテストケースとして実装し、`cargo test`で全パス
- `tests/snapshots/asm/`に各シナリオのテストを追加

### 漏れ検出
- 既存テスト（`cargo test`）が全パスすることを確認（既存機能の破壊がないこと）
- エラーケース（不正命令、スタックアンダーフロー等）のテストを網羅

## 11. Test Plan

### E2E シナリオ 1: 基本的な算術演算
```
Given: mocaソースに `let r = asm(10) -> i64 { __emit("PushInt", 5); __emit("Add"); }; print(r);` がある
When: コンパイル・実行する
Then: `15` が出力される
```

### E2E シナリオ 2: GCセーフポイント
```
Given: asmブロック内で大量のオブジェクト割り当てを行い、`__safepoint()`を挿入
When: GC閾値を低く設定して実行する
Then: セーフポイントでGCが発動し、メモリが解放される
```

### E2E シナリオ 3: エラーハンドリング
```
Given: mocaソースに `asm { __emit("Add"); }` がある（スタックが空）
When: 実行する
Then: スタックアンダーフローのランタイムエラーが発生する
```
