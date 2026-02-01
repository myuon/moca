# Spec.md - Generics Support

## 1. Goal
- moca 言語に Generics（ジェネリクス）を導入し、関数・メソッド・構造体で型パラメータ `<T, U>` を使えるようにする
- コンパイラに Monomorphisation フェーズを追加し、呼び出し時の具体型に応じた特殊化コードを生成する

## 2. Non-Goals
- Trait bound（型制約）: `<T: Comparable>` のような制約は実装しない
- 高階型（Higher-Kinded Types）: `<F<_>>` のような型コンストラクタは対象外
- 既存の `vec<T>`, `map<K,V>` の汎用ジェネリクスへの置き換え: 後日対応

## 3. Target Users
- moca 言語のユーザー（言語利用者）
- moca コンパイラの開発者（将来の拡張のため）

## 4. Core User Flow
1. ユーザーが型パラメータ付きの関数/構造体を定義する
2. 呼び出し時に型引数を明示 (`f<int>()`) または省略（型推論）
3. コンパイラが型チェック時に型パラメータを解決
4. Monomorphisation フェーズで具体型ごとの関数/構造体を生成
5. 実行時は通常の（非ジェネリック）コードとして動作

## 5. Inputs & Outputs

### Inputs（ソースコード例）
```
// ジェネリック関数
fun identity<T>(x: T) -> T {
    return x;
}

// 複数型パラメータ
fun pair<T, U>(a: T, b: U) -> {first: T, second: U} {
    return {first: a, second: b};
}

// ジェネリック構造体
struct Container<T> {
    value: T,
}

// ジェネリック impl ブロック
impl<T> Container<T> {
    fun new(v: T) -> Container<T> {
        return Container { value: v };
    }

    fun get(self) -> T {
        return self.value;
    }

    // メソッド独自の型パラメータ
    fun map<U>(self, f: (T) -> U) -> Container<U> {
        return Container { value: f(self.value) };
    }
}

// 呼び出し
let a = identity<int>(42);
let b = identity("hello");  // 型推論で T = string

let c = Container<int>::new(100);
let d = Container::new("world");  // 型推論で T = string
print(c.get());
print(d.get());
```

### Outputs
- 型チェック成功/エラーメッセージ
- Monomorphisation 後の具体化された関数群
- 実行可能なバイトコード

## 6. Tech Stack
- 言語: Rust（既存プロジェクト継続）
- テストフレームワーク: insta（スナップショットテスト、既存）
- その他: 既存の moca コンパイラ/VM インフラ

## 7. Rules & Constraints

### 構文ルール
- 型パラメータリスト: `<T>`, `<T, U>`, `<T, U, V>` （カンマ区切り）
- 型パラメータ名: 大文字開始の識別子（慣習として `T`, `U`, `V` など）
- 関数定義: `fun name<T>(params) -> RetType { ... }`
- 構造体定義: `struct Name<T> { field: T, ... }`
- impl ブロック: `impl<T> StructName<T> { ... }`
- メソッド追加パラメータ: `fun method<U>(self, ...) -> ... { ... }`
- 呼び出し時の型指定: `func<Type>(args)`, `Type<T>::method(args)`

### 型推論ルール
- 既存の Hindley-Milner 型推論を拡張
- 引数から型パラメータを推論可能な場合は型引数を省略可
- 推論不可能な場合はコンパイルエラー

### Monomorphisation ルール
- 呼び出しサイトで使用される具体型の組み合わせごとに特殊化
- 同じ具体型の組み合わせは1つの実装を共有
- 未使用のジェネリック定義はコード生成しない（デッドコード除去）

### 制約
- 型パラメータに制約（trait bound）は付けられない
- 再帰的な型パラメータ適用は許可（例: `Container<Container<int>>`）
- 型パラメータのデフォルト値は未サポート

## 8. Open Questions
- `vec<T>`, `map<K,V>` の汎用化タイミング（今回は対象外だが将来的に統合予定）

## 9. Acceptance Criteria（最大10個）

1. [ ] `fun identity<T>(x: T) -> T` のようなジェネリック関数を定義できる
2. [ ] `identity<int>(42)` のように型引数を明示して呼び出せる
3. [ ] `identity(42)` のように型引数を省略し、型推論で解決できる
4. [ ] `<T, U>` のような複数型パラメータを使用できる
5. [ ] `struct Container<T> { value: T }` のようなジェネリック構造体を定義できる
6. [ ] `impl<T> Container<T> { ... }` でジェネリック構造体にメソッドを定義できる
7. [ ] `Container<int>::new(42)` のように型引数付きで associated function を呼び出せる
8. [ ] メソッドに追加の型パラメータ `fun map<U>(self, f: (T) -> U)` を定義できる
9. [ ] Monomorphisation により、使用された具体型の組み合わせごとにコードが生成される
10. [ ] 既存のテスト（非ジェネリックコード）が引き続きパスする

## 10. Verification Strategy

### 進捗検証
- 各フェーズ（Parser → TypeChecker → Monomorphisation → Codegen）ごとにスナップショットテストを追加
- 段階的に機能を追加し、各段階でテストがパスすることを確認
- `cargo check && cargo test` を各コミット前に実行

### 達成検証
- 上記 Acceptance Criteria の全項目をテストケースとして実装
- `examples/` に generics を使ったサンプルプログラムを追加し、実行確認
- スナップショットテストで AST・型情報・生成コードを検証

### 漏れ検出
- エッジケースのテスト: ネストした型パラメータ、再帰的構造体、複数パラメータ
- 既存テストのリグレッション確認
- `cargo clippy` でコード品質チェック

## 11. Test Plan

### E2E シナリオ 1: ジェネリック関数の基本
```
Given: 以下のソースコード
  fun identity<T>(x: T) -> T {
      return x;
  }
  let a = identity<int>(42);
  let b = identity("hello");
  print(a);
  print(b);

When: コンパイル・実行する

Then:
  - コンパイル成功
  - 出力: "42" と "hello"
```

### E2E シナリオ 2: ジェネリック構造体とメソッド
```
Given: 以下のソースコード
  struct Box<T> {
      value: T,
  }

  impl<T> Box<T> {
      fun new(v: T) -> Box<T> {
          return Box { value: v };
      }
      fun get(self) -> T {
          return self.value;
      }
  }

  let int_box = Box<int>::new(123);
  let str_box = Box::new("test");
  print(int_box.get());
  print(str_box.get());

When: コンパイル・実行する

Then:
  - コンパイル成功
  - 出力: "123" と "test"
```

### E2E シナリオ 3: 複数型パラメータと型推論
```
Given: 以下のソースコード
  fun make_pair<T, U>(a: T, b: U) -> {first: T, second: U} {
      return {first: a, second: b};
  }

  let p1 = make_pair<int, string>(1, "one");
  let p2 = make_pair(2, "two");  // 型推論
  print(p1.first);
  print(p2.second);

When: コンパイル・実行する

Then:
  - コンパイル成功
  - 出力: "1" と "two"
```
