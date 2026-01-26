# Spec.md

## 1. Goal
- プロジェクト名を「mica」から「moca」にリネームし、すべてのコード・ドキュメント・設定ファイルで一貫した命名にする

## 2. Non-Goals
- GitHub リポジトリ名（ディレクトリ `/home/user/mica`）の変更
- npm / crates.io などへの公開名の変更
- 外部ドキュメント（GitHub Wiki等）の更新
- 旧API名(`mica_*`)のエイリアスや deprecation warning の提供

## 3. Target Users
- このリポジトリの開発者・メンテナー
- Moca言語（旧Mica）のユーザー

## 4. Core User Flow
1. 開発者がリネーム後のリポジトリを `git pull` する
2. `cargo build` でビルドが成功する
3. `cargo test` でテストが通過する
4. `./target/debug/moca` コマンドで実行できる
5. `.mc` ファイルを作成してMoca言語プログラムを書ける

## 5. Inputs & Outputs
- **入力**: 現在の「mica」を含むすべてのファイル
- **出力**: 「moca」にリネームされたファイル群（拡張子は `.mc`）

## 6. Tech Stack
- 言語: Rust
- ビルド: Cargo
- テスト: `cargo test`
- C FFI生成: cbindgen

## 7. Rules & Constraints
- 変換ルール:
  - `mica` → `moca`
  - `Mica` → `Moca`
  - `MICA` → `MOCA`
  - `.mica` (拡張子) → `.mc`
- 1コミットで一括変更
- 既存の機能・テストを壊さない

## 8. Open Questions
なし

## 9. Acceptance Criteria（最大10個）
1. `Cargo.toml` の `name` が `"moca"` になっている
2. `cargo build` が成功する
3. `cargo test` が全て通過する
4. `.mica` 拡張子のファイルが存在しない
5. `.mc` 拡張子のファイルがサンプル・テストに存在する
6. `include/moca.h` が生成される（`mica.h` は存在しない）
7. C API関数名がすべて `moca_*` プリフィックスになっている
8. ドキュメント（spec/*.md）内の「mica」「Mica」が「moca」「Moca」に変更されている
9. `grep -ri "mica" --include="*.rs" --include="*.toml" --include="*.md"` で意図しない残存がない
10. 実行バイナリ名が `moca` になっている

## 10. Verification Strategy
- **進捗検証**: 各カテゴリの変更完了後に `cargo check` を実行
- **達成検証**: `cargo test` 全通過 + 上記 Acceptance Criteria のチェック
- **漏れ検出**: `grep -ri "mica"` で残存箇所を検索し、意図的な残存（パス名等）以外がないことを確認

## 11. Test Plan

### e2e シナリオ 1: ビルド確認
- **Given**: リネーム完了後のリポジトリ
- **When**: `cargo build` を実行
- **Then**: エラーなくビルドが完了し、`target/debug/moca` が生成される

### e2e シナリオ 2: テスト実行
- **Given**: リネーム完了後のリポジトリ
- **When**: `cargo test` を実行
- **Then**: 全テストが通過する

### e2e シナリオ 3: サンプル実行
- **Given**: リネーム完了後のリポジトリ
- **When**: `./target/debug/moca run examples/fizzbuzz.mc` を実行
- **Then**: FizzBuzz の出力が正しく表示される
