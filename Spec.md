# Spec.md - AArch64 JIT 実行機能の有効化

## 1. Goal
- AArch64 アーキテクチャで JIT コンパイル済み関数を実行できるようにする（x86-64 と同等の機能）

## 2. Non-Goals
- 新しいオペレーションの追加
- パフォーマンス最適化
- GC safepoint の完全実装
- AArch64 実機でのテスト（環境なし）

## 3. Target Users
- AArch64 環境（Apple Silicon Mac、Linux ARM64）で mica を使用する開発者

## 4. Core User Flow
1. ユーザーが `cargo build --features jit` でビルド
2. mica プログラムを実行
3. 関数呼び出しが JIT threshold（1000回）に達する
4. JIT コンパイラが関数をネイティブコードにコンパイル
5. 以降の呼び出しは JIT コードで実行される
6. 結果が正しく返される

## 5. Inputs & Outputs
### Inputs
- JIT コンパイル済みの `CompiledCode`
- 関数引数（`argc` 個の `Value`）
- VM スタックとローカル変数領域

### Outputs
- 関数の戻り値（`Value`）
- エラー時は `Result::Err`

## 6. Tech Stack
- 言語: Rust
- JIT コンパイラ: `src/jit/compiler.rs`（既存）
- アセンブラ: `src/jit/aarch64.rs`（既存）
- マーシャリング: `src/jit/marshal.rs`（x86-64 と共通）
- メモリ管理: `src/jit/memory.rs`（既存）
- テスト: `tests/e2e.rs`（既存）

## 7. Rules & Constraints

### 実装ルール
- x86-64 の `execute_jit_function` と同じインターフェースを維持
- `JitContext`/`JitValue`/`JitReturn` を使用した値のマーシャリング
- 未対応オペレーションはコンパイル時にエラーを返しインタプリタにフォールバック

### ABI 規約（AArch64）
```
引数渡し:
  x0: JitContext へのポインタ（VM state）
  x1: Value stack pointer
  x2: Locals base pointer

戻り値:
  x0: JitReturn（tag + value の 128-bit 構造体）

Callee-saved レジスタ:
  x19: VM context pointer
  x20: Value stack pointer
  x21: Locals base pointer
  x22: Constants pool pointer
```

### 技術的制約
- `#[cfg(all(target_arch = "aarch64", feature = "jit"))]` で条件コンパイル
- unsafe コードは最小限に、既存パターンに従う
- コンパイル時の型安全性を維持

## 8. Open Questions
- なし（x86-64 実装を参考にすることで解決可能）

## 9. Acceptance Criteria（最大10個）

1. `src/vm/vm.rs` の `execute_jit_function`（AArch64版）が実装されている
2. x86-64 版と同じシグネチャ `fn execute_jit_function(&mut self, func_index: usize, argc: usize, func: &Function) -> Result<Value, String>` を持つ
3. `JitContext`/`JitValue`/`JitReturn` を使用して値をマーシャリングしている
4. 引数を正しく JIT 関数に渡せる
5. 戻り値を正しく `Value` に変換できる
6. コンパイルエラーがない（`cargo build --features jit --target aarch64-unknown-linux-gnu`）
7. x86-64 環境で既存テスト（`tests/e2e.rs`）が引き続きパスする
8. unsafe コードに適切なコメントがある
9. `--trace-jit` オプションで JIT 実行のトレースが出力される
10. 未コンパイル関数へのフォールバックが正しく動作する

## 10. Test Plan

### E2E Test 1: 基本的な JIT 実行
```
Given: fibonacci 関数が定義されている
When: fibonacci(10) を JIT threshold 以上呼び出す
Then: 正しい結果（55）が返される
```

### E2E Test 2: 算術演算の JIT 実行
```
Given: 整数の加減乗除を行う関数が定義されている
When: その関数を JIT threshold 以上呼び出す
Then: 計算結果が正しい
```

### E2E Test 3: 制御フローの JIT 実行
```
Given: ループと条件分岐を含む関数が定義されている
When: その関数を JIT threshold 以上呼び出す
Then: 正しい結果が返される
```

---

## 実装参考: x86-64 の execute_jit_function

```rust
// src/vm/vm.rs:217-261 (x86-64 版)
#[cfg(all(target_arch = "x86_64", feature = "jit"))]
fn execute_jit_function(
    &mut self,
    func_index: usize,
    argc: usize,
    func: &Function,
) -> Result<Value, String> {
    let compiled = self.jit_functions.get(&func_index).unwrap();

    // Set up locals area
    let locals_size = func.locals_count * 16;
    let mut locals: Vec<u8> = vec![0u8; locals_size];

    // Copy arguments to locals
    for i in 0..argc {
        let arg = &self.stack[self.stack.len() - argc + i];
        let jit_val = JitValue::from_value(arg);
        let offset = i * 16;
        unsafe {
            std::ptr::copy_nonoverlapping(
                &jit_val as *const JitValue as *const u8,
                locals.as_mut_ptr().add(offset),
                16,
            );
        }
    }

    // Pop arguments from stack
    for _ in 0..argc {
        self.stack.pop();
    }

    // Set up context and call
    let mut ctx = JitContext { vm: self as *mut VM };
    let mut vstack: Vec<u8> = vec![0u8; 4096];

    let entry: extern "C" fn(*mut JitContext, *mut u8, *mut u8) -> JitReturn =
        unsafe { compiled.entry_point() };

    let result = entry(&mut ctx, vstack.as_mut_ptr(), locals.as_mut_ptr());

    Ok(result.to_value())
}
```
