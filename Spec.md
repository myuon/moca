# Spec.md — print関数のprelude化とPrintDebug opcode廃止

## 1. Goal
- `print`/`print_debug` ビルトイン（コンパイラ組み込み + `Op::PrintDebug`）を廃止し、`print(s: string)` をprelude関数にする
- 呼び出し側は `print($"{x}")` や `print(debug(arr))` のように文字列を渡す形に統一する

## 2. Non-Goals
- `debug` ビルトインの自作実装への置き換え（将来issueとして積む）
- ObjectKind の廃止（Phase 4）
- `any` 型の言語仕様からの削除
- string interpolation の仕組み変更

## 3. Target Users
- moca 言語のユーザー
- moca コンパイラ / VM の開発者

## 4. Core User Flow
- `print("hello")` → prelude の `print(s: string)` を呼ぶ（改行付き出力）
- `print($"{x}")` → string interpolation で `x.to_string()` → `print` に渡す
- `print(debug(arr))` → `debug` ビルトインが `any` → `string` 変換 → `print` に渡す
- `eprint("error")` → stderr に改行付き出力

## 5. Inputs & Outputs
- **入力**: 既存の `print(x)` / `print_debug(x)` を使う全テストファイル（155ファイル）、prelude の `print_str` / `eprint_str`
- **出力**: `print(s: string)` / `eprint(s: string)` prelude関数、`debug(v: any) -> string` ビルトイン、全テスト書き換え

## 6. Tech Stack
- Rust（コンパイラ / VM）
- moca（prelude / テスト）

## 7. Rules & Constraints
- 既存 snapshot テストの**出力は変更しない**（.stdout は同一）
- `cargo fmt && cargo check && cargo test && cargo clippy` 全パス
- codegen の文字列リテラル最適化は削除する（普通に `print` 関数を呼ぶだけ）
- `debug` ビルトインは将来自作に置き換えるので、issueを作成する

## 8. Acceptance Criteria
1. `print` / `print_debug` がコンパイラのビルトインリストから削除されている
2. `Op::PrintDebug` が `Op` enum から削除されている
3. prelude に `print(s: string)` が定義されている（改行付き出力）
4. prelude に `eprint(s: string)` が定義されている（stderr、改行付き）
5. `debug(v: any) -> string` ビルトインが追加されている（Op::Debug で value_to_string を使用）
6. 全テストファイルが `print($"{x}")` / `print(debug(x))` / `print("literal")` 形式に移行済み
7. 全テスト（`cargo test`）がパスする
8. `cargo clippy` に警告がない
9. `debug` を自作に置き換えるissueが作成されている

## 9. Verification Strategy
- **進捗検証**: 各タスク完了後に `cargo check && cargo test` を実行
- **達成検証**: `grep "Op::PrintDebug" src/` でゼロ、全 Acceptance Criteria チェック
- **漏れ検出**: `grep 'print\b' src/compiler/resolver.rs` でビルトインに `print` が残っていないことを確認

## 10. Test Plan

### Scenario 1: int/float/bool の print
- **Given**: `let x = 42;`
- **When**: `print($"{x}")` を実行
- **Then**: "42\n" が出力される

### Scenario 2: 配列・nil の debug + print
- **Given**: `let arr: array<int> = []; let n = nil;`
- **When**: `print(debug(arr))` / `print(debug(n))` を実行
- **Then**: "[]\n" / "nil\n" が出力される

### Scenario 3: 文字列の print
- **Given**: `let s = "hello";`
- **When**: `print(s)` を実行
- **Then**: "hello\n" が出力される

---

## TODO

- [ ] 1. prelude: `print_str` → `print(s: string)` にリネーム（改行付き: `write(1, s, len(s)); write(1, "\n", 1);`）
- [ ] 2. prelude: `eprint_str` → `eprint(s: string)` にリネーム（同様に改行付き）
- [ ] 3. コンパイラ: `debug(v: any) -> string` ビルトインを追加（resolver, typechecker, codegen に `Op::Debug`）
- [ ] 4. VM: `Op::Debug` ハンドラ追加（pop any → value_to_string → push Ref(string)）
- [ ] 5. bytecode/verifier/microop/JIT/dump に `Op::Debug` を追加
- [ ] 6. codegen: `print` / `print_debug` ビルトインのコード生成を削除（文字列リテラル最適化も削除）
- [ ] 7. resolver/typechecker: `print` / `print_debug` をビルトインリストから削除
- [ ] 8. Op::PrintDebug および関連コード（MicroOp::PrintDebug, JIT print_debug_helper 等）を全削除
- [ ] 9. テストファイル全書き換え: `print(x)` → 型に応じて `print($"{x}")` / `print(debug(x))` / `print(x)`
- [ ] 10. 全テスト通過確認 (`cargo fmt && cargo check && cargo test && cargo clippy`)
- [ ] 11. `debug` を自作実装に置き換えるissueを作成
