# Spec.md

## 1. Goal
- JITコンパイラにおいて、型メタデータの伝搬・比較+分岐融合・int特殊化演算の3つの最適化を実装し、sum_loopベンチマークの実行時間を大幅に改善する

## 2. Non-Goals
- VM命令セット（Op enum）の変更
- インタープリタ側の最適化
- レジスタアロケータの導入（ローカル変数のレジスタ化）
- value stackの仮想化（レジスタベースIR化）

## 3. Target Users
- moca言語の利用者（数値計算を含むホットループの実行性能が向上）

## 4. Core User Flow
- ユーザーがmocaプログラムを `moca run` で実行
- JITがホットループを検知しコンパイルする際、型メタデータを利用して最適化されたネイティブコードを生成
- 明示的な操作は不要。既存プログラムが自動的に高速化される

## 5. Inputs & Outputs
- 入力: 既存のmocaソースコード（変更不要）
- 出力: 同じ実行結果、より短い実行時間

## 6. Tech Stack
- 言語: Rust
- 対象アーキテクチャ: x86_64, aarch64（両方のJITコンパイラを更新）
- テスト: 既存の `cargo test`（snapshot_performance テスト含む）

## 7. Rules & Constraints

### 型メタデータパイプライン
- `Type`（compiler::types）をそのままパイプライン上で運び、`Function` に格納する時点で `ValueType`（vm側の簡易enum）に変換する
- `vm` モジュールが `compiler` に依存しないこと（循環参照禁止）
- `ValueType` は `Int | Float | Bool | String | Ptr | Unknown` の6バリアント
- typecheckerで型が確定しない場合（`Type::Any`, `Type::Var` 等）は `ValueType::Unknown` にフォールバック

### 比較+分岐融合（JIT peephole）
- バイトコード列上で `Lt|Le|Gt|Ge` の直後に `JmpIfFalse` or `JmpIfTrue` が続くパターンを検出
- 全8パターン（4比較 × 2分岐）を対象
- 中間のbool push/pop を省略し、`cmp + jcc` に融合
- パターンに該当しない場合（比較結果をbool値として使う場合）は従来のコードを生成

### int特殊化演算（JIT型推論）
- `Function.local_types` を参照し、ループ内のAdd/Sub/Mul/Divでオペランドが確実にIntの場合、タグチェックを省略
- JITコンパイル時にスタック上の型を静的に追跡（シンプルな抽象解釈）
- 型が確定しない場合は従来のタグチェック付きコードにフォールバック
- 正しさを優先: 少しでも不確実なら最適化しない

### 全体制約
- 既存のテストが全てパスすること
- `cargo fmt && cargo check && cargo test && cargo clippy` が全て成功すること
- 既存の実行結果が変わらないこと（出力の一致）

## 8. Open Questions
- なし

## 9. Acceptance Criteria（最大10個）

1. `Function` 構造体に `local_types: Vec<ValueType>` フィールドが追加されている
2. `ValueType` enumが `vm` モジュール内に定義されている（Int/Float/Bool/String/Ptr/Unknown）
3. typecheckerの型情報がcodegen経由で `Function.local_types` に正しく伝搬されている
4. x86_64 JITで `Le + JmpIfFalse` 等の8パターンが融合コードを生成する
5. aarch64 JITで同様の比較+分岐融合が実装されている
6. x86_64 JITで `local_types` がIntのオペランドに対し、Add/Sub/Mul/Divのタグチェックが省略される
7. aarch64 JITで同様のint特殊化が実装されている
8. `local_types` が Unknown の場合、従来のタグチェック付きコードにフォールバックする
9. `cargo test` が全てパスする（既存テスト + 新規ユニットテスト）
10. sum_loop パフォーマンステストの実行時間が改善している

## 10. Verification Strategy

- **進捗検証**: 各タスク完了時に `cargo test` を実行し、既存テストが壊れていないことを確認
- **達成検証**: 全Acceptance Criteriaをチェックリストで確認。sum_loopの実行時間を最適化前後で比較
- **漏れ検出**: `cargo clippy` でコード品質確認。`--trace-jit` フラグでJITコンパイルが実際に最適化パスを通っていることを確認

## 11. Test Plan

### Test 1: 型メタデータの伝搬
- **Given**: int型のローカル変数を含む関数をコンパイル
- **When**: `Function` を生成
- **Then**: `local_types` の対応スロットが `ValueType::Int` になっている

### Test 2: 比較+分岐融合の正しさ
- **Given**: `while i <= N { ... }` パターンを含むプログラム
- **When**: JITコンパイルし実行
- **Then**: 実行結果が従来と一致する（sum_loop: 50000005000000）

### Test 3: int特殊化の正しさとフォールバック
- **Given**: int演算のみのループ と int/float混在のループ
- **When**: JITコンパイルし実行
- **Then**: 両方とも正しい結果を返す。int-onlyループはタグチェックなしコード、混在ループは従来コードが生成される
