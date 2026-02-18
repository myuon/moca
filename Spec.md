# Spec.md — Phase 3: コンパイラに型別 print 命令を追加

Phase 1 (Map動的ディスパッチ除去) 完了。Phase 2 (to_string型別特殊化) はprimitive impl blocks移行済みでスキップ。

## 1. Goal
- `print()` でコンパイル時に型がわかる場合、型別の print 命令を emit し、ランタイムの ObjectKind チェックを不要にする
- `Op::TypeOf` の prelude/std での使用がゼロであることを維持

## 2. Non-Goals
- ObjectKind の廃止（Phase 4）
- `type_of()` ビルトイン関数自体の削除（ユーザー向け機能として残す）
- `values_equal` の ObjectKind::String チェック除去（desugar で _string_eq に書き換え済みのため、フォールバックとして残す）
- JIT の変更

## 3. Target Users
- moca コンパイラ / VM の開発者

## 4. Core User Flow
- `print(42)` → codegen が `Op::PrintInt` を emit（`value_to_string` 経由せず直接出力）
- `print(3.14)` → `Op::PrintFloat`
- `print(true)` → `Op::PrintBool`
- `print("hello")` → 既存の `print_str` 関数呼び出し（変更なし）
- `print(some_ref)` → 型不明な場合は既存の `Op::PrintDebug`（ObjectKind チェック使用、フォールバック）

## 5. Tech Stack
- Rust（コンパイラ/VM）

## 6. Rules & Constraints
- 既存テストの出力は変わらない
- `cargo fmt && cargo check && cargo test && cargo clippy` 全パス
- 文字列の print は既に `print_str` 関数呼び出しで最適化済みなので変更不要

## 7. Acceptance Criteria
1. `Op::PrintInt` が追加され、int 型の print で emit される
2. `Op::PrintFloat` が追加され、float 型の print で emit される
3. `Op::PrintBool` が追加され、bool 型の print で emit される
4. VM がこれらの新 opcode を正しく処理する
5. 全テスト（`cargo test`）がパスする
6. `cargo clippy` に警告がない

## 8. Verification Strategy
- **進捗検証**: 各 opcode 追加後に `cargo test` 実行
- **達成検証**: `print(42)` 等の呼び出しで新 opcode が使われることを確認

## TODO

- [x] 1. Op enum に PrintInt/PrintFloat/PrintBool を追加
- [x] 2. VM に新 opcode のハンドラを追加
- [x] 3. codegen で print() の引数型に応じて型別 opcode を emit
- [x] 4. bytecode serialization に新 opcode を追加
- [x] 5. verifier に新 opcode を追加
- [x] 6. 全テスト通過確認（cargo fmt/check/test/clippy 全パス）
