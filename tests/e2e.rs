use std::process::Command;

fn run_moca(source: &str) -> (String, String, bool) {
    // Use a unique temp file per test to avoid conflicts in parallel runs
    let temp_dir = std::env::temp_dir();
    let unique_id = std::thread::current().id();
    let temp_file = temp_dir.join(format!("moca_test_{:?}.mc", unique_id));
    std::fs::write(&temp_file, source).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_moca"))
        .args(["run", temp_file.to_str().unwrap()])
        .output()
        .expect("failed to execute moca");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();

    std::fs::remove_file(&temp_file).ok();

    (stdout, stderr, success)
}

fn assert_success(source: &str) -> String {
    let (stdout, stderr, success) = run_moca(source);
    assert!(success, "program should succeed, stderr:\n{}", stderr);
    stdout
}

fn assert_failure(source: &str) -> String {
    let (_, stderr, success) = run_moca(source);
    assert!(!success, "program should fail");
    stderr
}

#[test]
fn test_arithmetic() {
    let source = r#"
let x = 10 + 20 * 2;
print(x);
let y = x % 7;
print(y);
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "50\n1\n");
}

#[test]
fn test_control_flow() {
    let source = r#"
var i = 0;
while i < 5 {
    if i % 2 == 0 {
        print(i);
    }
    i = i + 1;
}
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "0\n2\n4\n");
}

#[test]
fn test_fizzbuzz() {
    let source = r#"
fun fizzbuzz(n) {
    var i = 1;
    while i <= n {
        if i % 15 == 0 {
            print(-3);
        } else if i % 3 == 0 {
            print(-1);
        } else if i % 5 == 0 {
            print(-2);
        } else {
            print(i);
        }
        i = i + 1;
    }
}

fizzbuzz(15);
"#;
    let stdout = assert_success(source);
    let expected = "1\n2\n-1\n4\n-2\n-1\n7\n8\n-1\n-2\n11\n-1\n13\n14\n-3\n";
    assert_eq!(stdout, expected);
}

#[test]
fn test_fibonacci() {
    let source = r#"
fun fib(n) {
    if n <= 1 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

var i = 0;
while i < 10 {
    print(fib(i));
    i = i + 1;
}
"#;
    let stdout = assert_success(source);
    let expected = "0\n1\n1\n2\n3\n5\n8\n13\n21\n34\n";
    assert_eq!(stdout, expected);
}

#[test]
fn test_factorial() {
    let source = r#"
fun fact(n) {
    if n <= 1 {
        return 1;
    }
    return n * fact(n - 1);
}

print(fact(5));
print(fact(10));
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "120\n3628800\n");
}

#[test]
fn test_boolean_operations() {
    let source = r#"
print(true && true);
print(true && false);
print(false || true);
print(false || false);
print(!true);
print(!false);
"#;
    let stdout = assert_success(source);
    // true && true = true
    // true && false = false
    // false || true = true
    // false || false = false
    // !true = false
    // !false = true
    assert_eq!(stdout, "true\nfalse\ntrue\nfalse\nfalse\ntrue\n");
}

#[test]
fn test_comparison() {
    let source = r#"
print(1 < 2);
print(2 < 1);
print(1 <= 1);
print(2 > 1);
print(1 >= 1);
print(1 == 1);
print(1 != 2);
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "true\nfalse\ntrue\ntrue\ntrue\ntrue\ntrue\n");
}

#[test]
fn test_unary_minus() {
    let source = r#"
let x = -5;
print(x);
print(-x);
print(--x);
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "-5\n5\n-5\n");
}

#[test]
fn test_division_by_zero() {
    let source = "print(1 / 0);";
    let stderr = assert_failure(source);
    assert!(stderr.contains("division by zero"), "stderr: {}", stderr);
}

#[test]
fn test_undefined_variable() {
    let source = "print(x);";
    let stderr = assert_failure(source);
    assert!(stderr.contains("undefined variable"), "stderr: {}", stderr);
}

#[test]
fn test_undefined_function() {
    let source = "foo();";
    let stderr = assert_failure(source);
    assert!(stderr.contains("undefined function"), "stderr: {}", stderr);
}

#[test]
fn test_immutable_assignment() {
    let source = r#"
let x = 1;
x = 2;
"#;
    let stderr = assert_failure(source);
    assert!(
        stderr.contains("cannot assign to immutable"),
        "stderr: {}",
        stderr
    );
}

// ===== v1 Feature Tests =====

#[test]
fn test_string_operations() {
    let source = r#"
let s = "hello";
print(s);
let s2 = "world";
print(s + " " + s2);
print(len(s));
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "hello\nhello world\n5\n");
}

#[test]
fn test_float_operations() {
    let source = r#"
let x = 3.14;
let y = 2.0;
print(x + y);
print(x * y);
print(x > y);
"#;
    let stdout = assert_success(source);
    assert!(stdout.contains("5.14"));
    assert!(stdout.contains("6.28"));
    assert!(stdout.contains("true"));
}

#[test]
fn test_nil_value() {
    let source = r#"
let x = nil;
print(x);
print(x == nil);
print(x != nil);
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "nil\ntrue\nfalse\n");
}

#[test]
fn test_array_literal_and_access() {
    let source = r#"
let arr = [1, 2, 3, 4, 5];
print(arr[0]);
print(arr[2]);
print(arr[4]);
print(len(arr));
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "1\n3\n5\n5\n");
}

#[test]
fn test_array_mutation() {
    let source = r#"
var arr = [10, 20, 30];
arr[1] = 25;
print(arr[1]);
push(arr, 40);
print(len(arr));
print(arr[3]);
let last = pop(arr);
print(last);
print(len(arr));
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "25\n4\n40\n40\n3\n");
}

#[test]
fn test_object_literal_and_access() {
    let source = r#"
let obj = { x: 10, y: 20, name: "point" };
print(obj.x);
print(obj.y);
print(obj.name);
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "10\n20\npoint\n");
}

#[test]
fn test_object_mutation() {
    let source = r#"
var obj = { value: 100 };
print(obj.value);
obj.value = 200;
print(obj.value);
obj.newField = 300;
print(obj.newField);
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "100\n200\n300\n");
}

#[test]
fn test_for_in_loop() {
    let source = r#"
let arr = [1, 2, 3, 4, 5];
var sum = 0;
for x in arr {
    sum = sum + x;
}
print(sum);
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "15\n");
}

#[test]
fn test_for_in_with_print() {
    let source = r#"
let items = [10, 20, 30];
for item in items {
    print(item);
}
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "10\n20\n30\n");
}

#[test]
fn test_type_of_builtin() {
    let source = r#"
print(type_of(42));
print(type_of(3.14));
print(type_of(true));
print(type_of(nil));
print(type_of("hello"));
print(type_of([1, 2, 3]));
print(type_of({ x: 1 }));
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "int\nfloat\nbool\nnil\nstring\narray\nobject\n");
}

#[test]
fn test_to_string_builtin() {
    let source = r#"
print(to_string(42));
print(to_string(3.14));
print(to_string(true));
print(to_string(nil));
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "42\n3.14\ntrue\nnil\n");
}

#[test]
fn test_parse_int_builtin() {
    let source = r#"
let n = parse_int("42");
print(n);
print(n + 8);
let m = parse_int("  123  ");
print(m);
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "42\n50\n123\n");
}

#[test]
fn test_nested_arrays_and_objects() {
    let source = r#"
let data = {
    numbers: [1, 2, 3],
    point: { x: 10, y: 20 }
};
print(data.numbers[1]);
print(data.point.x);
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "2\n10\n");
}

#[test]
fn test_string_escape_sequences() {
    let source = r#"
let s = "line1\nline2";
print(s);
let t = "tab\there";
print(t);
"#;
    let stdout = assert_success(source);
    assert_eq!(stdout, "line1\nline2\ntab\there\n");
}

// ===== v3 Feature Tests: JIT and GC =====

fn run_moca_with_args(source: &str, args: &[&str]) -> (String, String, bool) {
    let temp_dir = std::env::temp_dir();
    let unique_id = std::thread::current().id();
    let temp_file = temp_dir.join(format!("moca_test_{:?}.mc", unique_id));
    std::fs::write(&temp_file, source).unwrap();

    let mut cmd_args = vec!["run"];
    cmd_args.extend(args);
    cmd_args.push(temp_file.to_str().unwrap());

    let output = Command::new(env!("CARGO_BIN_EXE_moca"))
        .args(&cmd_args)
        .output()
        .expect("failed to execute moca");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();

    std::fs::remove_file(&temp_file).ok();

    (stdout, stderr, success)
}

#[test]
fn test_jit_mode_off() {
    // Test that --jit=off works correctly
    let source = r#"
fun sum(n) {
    var total = 0;
    var i = 0;
    while i < n {
        total = total + i;
        i = i + 1;
    }
    return total;
}

print(sum(100));
"#;
    let (stdout, _stderr, success) = run_moca_with_args(source, &["--jit=off"]);
    assert!(success, "JIT=off mode should work");
    assert_eq!(stdout.trim(), "4950");
}

#[test]
fn test_jit_trace() {
    // Test that --trace-jit outputs JIT information
    let source = r#"
fun hot_func() {
    return 42;
}

var i = 0;
while i < 10 {
    hot_func();
    i = i + 1;
}
print("done");
"#;
    let (stdout, stderr, success) =
        run_moca_with_args(source, &["--trace-jit", "--jit-threshold=5"]);
    assert!(success, "trace-jit should work");
    assert_eq!(stdout.trim(), "done");
    // Should contain JIT trace information
    assert!(
        stderr.contains("[JIT]"),
        "should have JIT trace output: {}",
        stderr
    );
}

#[test]
fn test_jit_hot_function_detection() {
    // Test that hot functions are detected with low threshold
    let source = r#"
fun counter(n) {
    var sum = 0;
    var i = 0;
    while i < n {
        sum = sum + 1;
        i = i + 1;
    }
    return sum;
}

var total = 0;
var j = 0;
while j < 20 {
    total = total + counter(10);
    j = j + 1;
}
print(total);
"#;
    let (stdout, stderr, success) =
        run_moca_with_args(source, &["--trace-jit", "--jit-threshold=10"]);
    assert!(success, "hot function detection should work");
    assert_eq!(stdout.trim(), "200");
    // Should detect hot function
    assert!(
        stderr.contains("Hot function detected"),
        "should detect hot function: {}",
        stderr
    );
}

#[test]
fn test_jit_correctness_arithmetic() {
    // Test that JIT produces same results as interpreter for arithmetic
    let source = r#"
fun compute(n) {
    var sum = 0;
    var i = 0;
    while i < n {
        sum = sum + i * 2 - 1;
        i = i + 1;
    }
    return sum;
}

var total = 0;
var k = 0;
while k < 50 {
    total = total + compute(100);
    k = k + 1;
}
print(total);
"#;
    // Run with JIT off
    let (stdout_off, _, success_off) = run_moca_with_args(source, &["--jit=off"]);
    assert!(success_off, "JIT off should succeed");

    // Run with JIT on (low threshold to ensure JIT is used)
    let (stdout_on, _, success_on) = run_moca_with_args(source, &["--jit=on", "--jit-threshold=5"]);
    assert!(success_on, "JIT on should succeed");

    // Results must match
    assert_eq!(
        stdout_off.trim(),
        stdout_on.trim(),
        "JIT on/off should produce same result"
    );
}

#[test]
fn test_jit_correctness_control_flow() {
    // Test that JIT produces same results as interpreter for control flow
    let source = r#"
fun fizzbuzz_count(n) {
    var fizz = 0;
    var buzz = 0;
    var fizzbuzz = 0;
    var i = 1;
    while i <= n {
        if i % 15 == 0 {
            fizzbuzz = fizzbuzz + 1;
        } else if i % 3 == 0 {
            fizz = fizz + 1;
        } else if i % 5 == 0 {
            buzz = buzz + 1;
        }
        i = i + 1;
    }
    return fizz * 1000000 + buzz * 1000 + fizzbuzz;
}

var result = 0;
var j = 0;
while j < 20 {
    result = result + fizzbuzz_count(100);
    j = j + 1;
}
print(result);
"#;
    let (stdout_off, _, success_off) = run_moca_with_args(source, &["--jit=off"]);
    assert!(success_off, "JIT off should succeed");

    let (stdout_on, _, success_on) = run_moca_with_args(source, &["--jit=on", "--jit-threshold=5"]);
    assert!(success_on, "JIT on should succeed");

    assert_eq!(
        stdout_off.trim(),
        stdout_on.trim(),
        "JIT on/off should produce same result for control flow"
    );
}

#[test]
fn test_jit_correctness_locals() {
    // Test that JIT produces same results as interpreter for local variables
    let source = r#"
fun use_locals(a, b, c) {
    var x = a + b;
    var y = b + c;
    var z = x * y;
    var result = z - a - b - c;
    return result;
}

var sum = 0;
var i = 0;
while i < 100 {
    sum = sum + use_locals(i, i + 1, i + 2);
    i = i + 1;
}
print(sum);
"#;
    let (stdout_off, _, success_off) = run_moca_with_args(source, &["--jit=off"]);
    assert!(success_off, "JIT off should succeed");

    let (stdout_on, _, success_on) = run_moca_with_args(source, &["--jit=on", "--jit-threshold=5"]);
    assert!(success_on, "JIT on should succeed");

    assert_eq!(
        stdout_off.trim(),
        stdout_on.trim(),
        "JIT on/off should produce same result for locals"
    );
}

#[test]
fn test_jit_correctness_nested_calls() {
    // Test that JIT produces same results for nested function calls
    let source = r#"
fun inner(x) {
    return x * 2 + 1;
}

fun outer(n) {
    var sum = 0;
    var i = 0;
    while i < n {
        sum = sum + inner(i);
        i = i + 1;
    }
    return sum;
}

var total = 0;
var j = 0;
while j < 30 {
    total = total + outer(20);
    j = j + 1;
}
print(total);
"#;
    let (stdout_off, _, success_off) = run_moca_with_args(source, &["--jit=off"]);
    assert!(success_off, "JIT off should succeed");

    let (stdout_on, _, success_on) = run_moca_with_args(source, &["--jit=on", "--jit-threshold=5"]);
    assert!(success_on, "JIT on should succeed");

    assert_eq!(
        stdout_off.trim(),
        stdout_on.trim(),
        "JIT on/off should produce same result for nested calls"
    );
}

#[test]
fn test_gc_stats() {
    // Test that --gc-stats outputs GC statistics
    let source = r#"
fun allocate_arrays() {
    var i = 0;
    while i < 1000 {
        let arr = [i, i + 1, i + 2];
        i = i + 1;
    }
}

allocate_arrays();
print("done");
"#;
    let (stdout, stderr, success) = run_moca_with_args(source, &["--gc-stats"]);
    assert!(success, "gc-stats should work");
    assert_eq!(stdout.trim(), "done");
    // Should contain GC statistics
    assert!(
        stderr.contains("[GC]"),
        "should have GC stats output: {}",
        stderr
    );
}

#[test]
fn test_quickening_sum_loop() {
    // Test that quickening works correctly for a sum loop
    let source = r#"
fun sum_to(n) {
    var total = 0;
    var i = 1;
    while i <= n {
        total = total + i;
        i = i + 1;
    }
    return total;
}

print(sum_to(100));
print(sum_to(1000));
"#;
    let (stdout, _, success) = run_moca_with_args(source, &["--jit=on"]);
    assert!(success, "quickening should work");
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "5050");
    assert_eq!(lines[1], "500500");
}

#[test]
fn test_gc_mode_concurrent() {
    // Test that --gc-mode=concurrent works (even if behavior is same as stw for now)
    let source = r#"
var arr = [];
var i = 0;
while i < 100 {
    push(arr, i);
    i = i + 1;
}
print(len(arr));
"#;
    let (stdout, _, success) = run_moca_with_args(source, &["--gc-mode=concurrent"]);
    assert!(success, "concurrent GC mode should work");
    assert_eq!(stdout.trim(), "100");
}

// ===== Thread Tests =====

#[test]
fn test_thread_spawn_and_join() {
    // Test basic thread spawn and join
    let source = r#"
fun worker() {
    var sum = 0;
    var i = 0;
    while i < 100 {
        sum = sum + i;
        i = i + 1;
    }
    return sum;
}

let handle = spawn(worker);
let result = join(handle);
print(result);
"#;
    let (stdout, stderr, success) = run_moca(source);
    assert!(success, "thread spawn/join should work, stderr: {}", stderr);
    assert_eq!(stdout.trim(), "4950");
}

#[test]
fn test_channel_send_recv() {
    // Test channel communication
    let source = r#"
let ch = channel();
let sender_id = ch[0];
let receiver_id = ch[1];

// Send some values
send(sender_id, 42);
send(sender_id, 100);

// Receive them
let a = recv(receiver_id);
let b = recv(receiver_id);
print(a);
print(b);
"#;
    let (stdout, stderr, success) = run_moca(source);
    assert!(success, "channel send/recv should work, stderr: {}", stderr);
    assert_eq!(stdout, "42\n100\n");
}

#[test]
fn test_thread_with_channel() {
    // Test thread communication via channel (simplified version of spec test)
    let source = r#"
fun worker() {
    var sum = 0;
    var i = 0;
    while i < 1000 {
        sum = sum + i;
        i = i + 1;
    }
    return sum;
}

let handle = spawn(worker);
let result = join(handle);
print(result);
"#;
    let (stdout, stderr, success) = run_moca(source);
    assert!(
        success,
        "thread with computation should work, stderr: {}",
        stderr
    );
    assert_eq!(stdout.trim(), "499500");
}

// ===== Struct Tests =====

#[test]
fn test_struct_definition_and_literal() {
    // Test basic struct definition and creation
    let source = r#"
struct Point {
    x: int,
    y: int
}

let p = Point { x: 10, y: 20 };
print(p);
"#;
    let (stdout, stderr, success) = run_moca(source);
    assert!(success, "struct creation should work, stderr: {}", stderr);
    // Struct is compiled as an array [x_value, y_value]
    assert_eq!(stdout.trim(), "[10, 20]");
}

#[test]
fn test_struct_field_access() {
    // Test struct field access
    let source = r#"
struct Point {
    x: int,
    y: int
}

let p = Point { x: 5, y: 15 };
print(p[0]);
print(p[1]);
"#;
    let (stdout, stderr, success) = run_moca(source);
    assert!(
        success,
        "struct field access should work, stderr: {}",
        stderr
    );
    // Since structs are compiled as arrays, access by index
    assert_eq!(stdout, "5\n15\n");
}

#[test]
fn test_struct_in_function() {
    // Test struct passed to and returned from function
    let source = r#"
struct Point {
    x: int,
    y: int
}

fun make_point(a, b) {
    return Point { x: a, y: b };
}

fun sum_point(p) {
    return p[0] + p[1];
}

let p1 = make_point(3, 7);
let result = sum_point(p1);
print(result);
"#;
    let (stdout, stderr, success) = run_moca(source);
    assert!(
        success,
        "struct in function should work, stderr: {}",
        stderr
    );
    assert_eq!(stdout.trim(), "10");
}

#[test]
fn test_struct_with_nullable_field() {
    // Test struct with nullable field
    let source = r#"
struct User {
    name: string,
    age: int?
}

let u1 = User { name: "Alice", age: 30 };
let u2 = User { name: "Bob", age: nil };
print(u1[0]);
print(u1[1]);
print(u2[0]);
print(u2[1]);
"#;
    let (stdout, stderr, success) = run_moca(source);
    assert!(
        success,
        "struct with nullable field should work, stderr: {}",
        stderr
    );
    assert_eq!(stdout, "Alice\n30\nBob\nnil\n");
}

#[test]
fn test_multiple_structs() {
    // Test multiple struct definitions
    let source = r#"
struct Point {
    x: int,
    y: int
}

struct Rectangle {
    width: int,
    height: int
}

let p = Point { x: 1, y: 2 };
let r = Rectangle { width: 10, height: 20 };
print(p[0]);
print(r[0]);
print(r[1]);
"#;
    let (stdout, stderr, success) = run_moca(source);
    assert!(success, "multiple structs should work, stderr: {}", stderr);
    assert_eq!(stdout, "1\n10\n20\n");
}

#[test]
fn test_struct_field_access_dot_syntax() {
    // Test struct field access with .field syntax
    let source = r#"
struct Point {
    x: int,
    y: int
}

let p = Point { x: 10, y: 20 };
print(p.x);
print(p.y);
"#;
    let (stdout, stderr, success) = run_moca(source);
    assert!(
        success,
        "struct field access with dot syntax should work, stderr: {}",
        stderr
    );
    assert_eq!(stdout, "10\n20\n");
}

#[test]
fn test_struct_field_mutation() {
    // Test struct field mutation with .field syntax
    let source = r#"
struct Counter {
    value: int
}

var c = Counter { value: 0 };
print(c.value);
c.value = 42;
print(c.value);
"#;
    let (stdout, stderr, success) = run_moca(source);
    assert!(
        success,
        "struct field mutation should work, stderr: {}",
        stderr
    );
    assert_eq!(stdout, "0\n42\n");
}

// ============================================================================
// Dump Options Tests
// ============================================================================

/// Run moca with file path first, then additional args (for dump options)
fn run_moca_with_trailing_args(source: &str, args: &[&str]) -> (String, String, bool) {
    let temp_dir = std::env::temp_dir();
    let unique_id = std::thread::current().id();
    let temp_file = temp_dir.join(format!("moca_test_{:?}.mc", unique_id));
    std::fs::write(&temp_file, source).unwrap();

    let mut cmd_args = vec!["run", temp_file.to_str().unwrap()];
    cmd_args.extend(args);

    let output = Command::new(env!("CARGO_BIN_EXE_moca"))
        .args(&cmd_args)
        .output()
        .expect("failed to execute moca");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();

    std::fs::remove_file(&temp_file).ok();

    (stdout, stderr, success)
}

#[test]
fn test_dump_ast() {
    // Test --dump-ast option outputs AST to stderr
    let source = r#"
let x = 1 + 2;
print(x);
"#;
    let (stdout, stderr, success) = run_moca_with_trailing_args(source, &["--dump-ast"]);
    assert!(
        success,
        "program should succeed with --dump-ast, stderr: {}",
        stderr
    );
    // Check AST is in stderr
    assert!(
        stderr.contains("== AST =="),
        "stderr should contain AST header"
    );
    assert!(
        stderr.contains("Program"),
        "stderr should contain 'Program'"
    );
    assert!(
        stderr.contains("Binary: +"),
        "stderr should contain binary op"
    );
    // Check program still executes
    assert_eq!(stdout, "3\n", "program should produce correct output");
}

#[test]
fn test_dump_ast_and_bytecode() {
    // Test multiple dump options simultaneously
    let source = r#"
let x = 42;
print(x);
"#;
    let (stdout, stderr, success) =
        run_moca_with_trailing_args(source, &["--dump-ast", "--dump-bytecode"]);
    assert!(success, "program should succeed with multiple dump options");
    // Check both dumps are present in stderr
    assert!(
        stderr.contains("== AST =="),
        "stderr should contain AST header"
    );
    assert!(
        stderr.contains("== Bytecode ==") || stderr.contains("== Main =="),
        "stderr should contain bytecode header"
    );
    assert!(
        stderr.contains("PushInt 42"),
        "stderr should contain PushInt instruction"
    );
    // Check program still executes
    assert_eq!(stdout, "42\n");
}

#[test]
fn test_dump_bytecode_to_file() {
    // Test --dump-bytecode=path outputs to file
    let source = r#"
let x = 10;
print(x);
"#;
    let temp_dir = std::env::temp_dir();
    let unique_id = std::thread::current().id();
    let dump_file = temp_dir.join(format!("moca_dump_{:?}.txt", unique_id));

    let (stdout, stderr, success) = run_moca_with_trailing_args(
        source,
        &[&format!("--dump-bytecode={}", dump_file.to_str().unwrap())],
    );

    assert!(
        success,
        "program should succeed with file dump, stderr: {}",
        stderr
    );
    assert_eq!(stdout, "10\n", "program should produce correct output");

    // Check dump file was created and contains bytecode
    let dump_content = std::fs::read_to_string(&dump_file).expect("dump file should exist");
    assert!(
        dump_content.contains("== Main =="),
        "dump file should contain Main header"
    );
    assert!(
        dump_content.contains("PushInt 10"),
        "dump file should contain PushInt instruction"
    );

    // Cleanup
    std::fs::remove_file(&dump_file).ok();
}
