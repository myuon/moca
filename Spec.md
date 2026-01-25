# Spec.md - x86-64 JIT Support

## 1. Goal
- x86-64（amd64）アーキテクチャ向けにJITコンパイル機能を追加し、aarch64と同等のバイトコード操作をネイティブコードに変換し、**実際にJIT実行できる**ようにする

## 2. Non-Goals
- 浮動小数点演算の最適化
- 配列・オブジェクト操作のJIT対応
- スレッド操作のJIT対応
- レジスタ割り当て最適化
- Windows対応（VirtualAlloc等）
- 既存aarch64実装の変更（JIT実行機能の追加は対象外）

## 3. Target Users
- micaをx86-64環境（Linux/macOS）で利用する開発者
- JIT機能を有効にしてビルドするユーザー

## 4. Core User Flow
1. ユーザーが`cargo build --features jit`でx86-64環境でビルド
2. micaスクリプトを実行
3. 関数呼び出しが閾値（1000回）に達するとJITコンパイルが発動
4. x86-64ネイティブコードが生成される
5. **以降の呼び出しではJITコンパイル済みネイティブコードが実行される**
6. `--trace-jit`オプションでコンパイル・実行状況を確認可能

## 5. Inputs & Outputs
### Inputs
- micaバイトコード（Function構造体）
- JITコンパイル閾値設定
- VMコンテキスト（値スタック、ローカル変数、定数プール）

### Outputs
- x86-64ネイティブコード（実行可能メモリ上）
- CompiledCode構造体（関数ポインタ保持）
- スタックマップ（GC用）
- **JIT実行結果（戻り値）**

## 6. Tech Stack
- **言語**: Rust
- **メモリ管理**: libc::mmap（Unix系）
- **条件付きコンパイル**: `#[cfg(all(target_arch = "x86_64", feature = "jit"))]`
- **テスト**: cargo test
- **ABI**: System V AMD64 ABI（Linux/macOS）

## 7. Rules & Constraints
### 振る舞いのルール
- aarch64実装と同等のOp対応範囲を維持
- 128ビット値表現（tag 64bit + payload 64bit）を継承
- System V AMD64 ABIに準拠
- JIT実行とインタプリタ実行で同一の結果を返す

### 技術的制約
- x86-64のレジスタ慣例に従う
  - Callee-saved: RBX, RBP, R12-R15
  - Caller-saved: RAX, RCX, RDX, RSI, RDI, R8-R11
  - 引数: RDI, RSI, RDX, RCX, R8, R9
  - 戻り値: RAX（+ RDX for 128-bit）
- RIP相対アドレッシングを活用
- 64ビット即値はMOV r64, imm64（REX.W + B8+rd）で直接ロード

### 破ってはいけない前提
- 既存のaarch64実装を壊さない
- feature gateで完全に無効化可能
- テストが全て通過する状態を維持

## 8. Open Questions
- なし

## 9. Acceptance Criteria
1. `cargo build --features jit`がx86-64環境で成功する
2. `cargo test --features jit`がx86-64環境で全て通過する
3. `src/jit/x86_64.rs`が存在し、基本命令（MOV, ADD, SUB, IMUL, CMP, JMP, JCC, CALL, RET等）をエンコードできる
4. JitCompilerがx86-64向けにバイトコードをコンパイルできる
5. 以下のOpがx86-64ネイティブコードに変換される:
   - PushInt, PushFloat, PushTrue, PushFalse, PushNil, Pop
   - LoadLocal, StoreLocal
   - Add, Sub, Mul, Div（整数版含む）
   - Lt, Le, Gt, Ge, Eq, Ne（整数版含む）
   - Jmp, JmpIfTrue, JmpIfFalse
   - Ret
6. **JITコンパイル済み関数が実際にネイティブ実行され、正しい結果を返す**
7. x86-64用のユニットテストが存在し、命令エンコーディングを検証できる
8. `#[cfg(all(target_arch = "x86_64", feature = "jit"))]`で適切に条件付きコンパイルされる
9. aarch64環境でのビルド・テストが引き続き成功する
10. clippy警告がゼロの状態を維持する

## 10. Test Plan

### E2E Scenario 1: x86-64環境でのJITビルドとテスト
- **Given**: x86-64 Linux/macOS環境
- **When**: `cargo build --features jit && cargo test --features jit`を実行
- **Then**: ビルドとテストが全て成功する

### E2E Scenario 2: JIT実行による算術演算
- **Given**: 整数の加算・乗算を行うmica関数（JITコンパイル閾値を低く設定）
- **When**: 関数を閾値回数以上呼び出す
- **Then**: JITコンパイルが発動し、以降の呼び出しでネイティブ実行され、インタプリタと同一の結果を返す

### E2E Scenario 3: JIT実行による制御フロー
- **Given**: ループと条件分岐を含むmica関数（例: fizzbuzz、factorial）
- **When**: JITコンパイル後に実行
- **Then**: 正しい制御フローで実行され、期待通りの結果を返す
