# Spec.md

## 1. Goal
- moca標準ライブラリに `sort_int` / `sort_float` 関数を追加し、`vec<int>` / `vec<float>` をin-placeでquicksortできるようにする

## 2. Non-Goals
- 安定ソートの実装
- `vec<string>` やカスタム比較関数を受け取る汎用ソート
- 非破壊（新しいvecを返す）版のソート
- メソッド形式（`v.sort()`）での提供（型特化implが未サポートのため）

## 3. Target Users
- mocaプログラムの開発者が、整数・浮動小数点数のベクタをソートしたいとき

## 4. Core User Flow
1. `vec<int>` または `vec<float>` を用意する
2. `sort_int(v)` または `sort_float(v)` を呼ぶ
3. 元のvecが昇順にソートされた状態になる

## 5. Inputs & Outputs
- **入力**: `vec<int>` または `vec<float>`（要素数0以上）
- **出力**: なし（in-placeで元のvecを書き換え）
- **事後条件**: vec内の要素が昇順に並んでいる

## 6. Tech Stack
- **実装言語**: moca（`std/prelude.mc` に追加）
- **テスト**: `std/prelude_test.mc` に単体テスト追加
- **パフォーマンステスト**: `tests/snapshots/performance/quicksort.mc` + `tests/snapshot_tests.rs` にRust参照実装
- **検証ツール**: `cargo test`、`cargo test snapshot_performance`（JIT feature）

## 7. Rules & Constraints
- quicksortアルゴリズムを使用する
- pivot選択はmedian-of-three（先頭・中央・末尾の中央値）
- 不安定ソート
- in-place（追加のvec割り当てなし）
- 空vecや長さ1のvecは何もせず正常終了
- 関数シグネチャ: `fun sort_int(v: vec<int>)` / `fun sort_float(v: vec<float>)`
- 戻り値の型注釈は `-> type` 形式を使用（moca規約）
- パフォーマンステストでは少なくとも1つの関数がJITコンパイル可能であること

## 8. Acceptance Criteria
1. `sort_int` が `vec<int>` を昇順にin-placeソートする
2. `sort_float` が `vec<float>` を昇順にin-placeソートする
3. 空のvecを渡してもエラーにならない
4. 長さ1のvecを渡してもエラーにならない
5. 100個のランダム整数を複数パターン入力し、出力が単調増加になっている
6. 100個のランダム浮動小数点数を入力し、出力が単調増加になっている
7. `cargo fmt && cargo check && cargo test && cargo clippy` が全てパスする
8. 長さ1000のパフォーマンステストが導入され、Rust参照実装と出力が一致する
9. パフォーマンステストでJITコンパイルが発生する（jit_compile_count > 0）

## 9. Verification Strategy

- **進捗検証**: 各タスク完了後に `cargo test` を実行し、既存テストが壊れていないことを確認
- **達成検証**: 全Acceptance Criteriaをチェックリストで確認。`cargo fmt && cargo check && cargo test && cargo clippy` が全パス
- **漏れ検出**: 100個のランダム入力を複数シードで実行し、ソート結果が常に単調増加であることを確認。パフォーマンステストでRust実装と出力一致を検証

## 10. Test Plan

### Test 1: 基本ソート（int）
- **Given**: ランダムな100個の整数を含む `vec<int>` を複数パターン（異なるシード）用意
- **When**: `sort_int(v)` を呼ぶ
- **Then**: 各vecの全要素が `v.get(i) <= v.get(i+1)` を満たす

### Test 2: 基本ソート（float）
- **Given**: ランダムな100個の浮動小数点数を含む `vec<float>` を用意
- **When**: `sort_float(v)` を呼ぶ
- **Then**: 各vecの全要素が `v.get(i) <= v.get(i+1)` を満たす

### Test 3: パフォーマンステスト
- **Given**: 長さ1000のランダム `vec<int>` をmocaとRustの両方で生成（同一シード）
- **When**: 両方でquicksortを実行し、ソート結果を出力
- **Then**: mocaとRustの出力が完全一致し、JITコンパイルが発生している
