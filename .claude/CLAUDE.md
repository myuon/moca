# プロジェクトルール

## コードチェック

コードの変更後やPR作成前には、以下のコマンドを順番に実行すること:

```bash
cargo fmt      # コードのフォーマット
cargo check    # コンパイルエラーの確認
cargo test     # テストの実行
cargo clippy   # lintチェック
```

コミット前に必ず`cargo fmt`を実行し、全てのチェックがパスすることを確認してからコミット・プッシュする。

## moca lint

`.mc` ファイルを変更・作成した場合は `moca lint <file>` を実行し、警告があれば修正すること。

## ドキュメント

- プロジェクトのドキュメントは `docs/` ディレクトリにあるので、適宜参照すること
- `/spec` スキルを使った後に生成される `spec.md` は、既存ドキュメントと内容を統合してから削除すること

## Skills

### spec → impl の自動実行

`/spec` スキルで仕様が確定したら、自動的に `/impl` スキルを実行すること。

### impl スキルの設定

`/impl` スキル実行時は以下のルールに従う:

- **確認不要**: 人間への確認は取らず、自律的に実装を進める
- **セルフレビュー**: レビューは自分自身で行う（外部レビューは不要）
- **チェックコマンド**: 各タスク完了後、以下のコマンドを実行する
  ```bash
  cargo fmt      # コードのフォーマット
  cargo check    # コンパイルエラーの確認
  cargo test     # テストの実行
  cargo clippy   # lintチェック
  ```

## Performance & Benchmarking

パフォーマンステストは以下の2箇所で構成されている:

- **mocaテストファイル**: `tests/snapshots/performance/*.mc`
- **Rustリファレンス実装・テストハーネス**: `tests/snapshot_tests.rs` 内の `snapshot_performance` 関数（`#[cfg(feature = "jit")]`）

### 新しいパフォーマンステストの追加手順

1. `tests/snapshots/performance/` に `.mc` ファイルを作成する
2. `tests/snapshot_tests.rs` に対応するRustリファレンス実装関数を `#[cfg(feature = "jit")]` 付きで追加する
3. `snapshot_performance` テスト関数内で `run_performance_test` を呼び出して登録する
4. **JIT制約**: テスト内の少なくとも1つの関数がJITコンパイル可能である必要がある。JITが対応しているのは整数/浮動小数点の算術・比較、制御フロー（`Jmp`, `BrIfFalse`, `Ret`）、関数呼び出しのみ。文字列操作、ヒープ操作（vec/map）、ビルトイン関数（`to_string`等）はJIT非対応のため、これらを使う場合はJIT対応のヘルパー関数を別途用意する
5. mocaとRustの出力が完全一致することを確認する

### 実装時の注意事項

1. **出力の一致**: 比較対象の実装間で出力が一致していること
2. **最適化による除去の防止**: コンパイラ最適化で計測対象の処理が除去されないようにする（`black_box`を使用するか、結果を出力する）
3. **ローカルでの事前実行**: コミット前に必ずローカルでベンチマークを実行して確認する（`cargo test snapshot_performance`）

## Language-Specific Notes

### moca

- 戻り値の型は `-> type` 形式を使用する（`: type` ではない）
  - ✅ `fn foo() -> int`
  - ❌ `fn foo(): int`

## Investigation Guidelines

ユーザーがシステムの挙動について質問した場合（例: 「なぜXが動かないのか？」）:

- まず、ユーザーが質問している**具体的な機構**を調査する
- 関連するが別の機構に脱線しない
- 調査スコープが不明確な場合は、先に確認する
