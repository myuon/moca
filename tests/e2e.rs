use std::process::Command;

fn run_mica(source: &str) -> (String, String, bool) {
    // Use a unique temp file per test to avoid conflicts in parallel runs
    let temp_dir = std::env::temp_dir();
    let unique_id = std::thread::current().id();
    let temp_file = temp_dir.join(format!("mica_test_{:?}.mica", unique_id));
    std::fs::write(&temp_file, source).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mica"))
        .args(["run", temp_file.to_str().unwrap()])
        .output()
        .expect("failed to execute mica");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();

    std::fs::remove_file(&temp_file).ok();

    (stdout, stderr, success)
}

fn assert_success(source: &str) -> String {
    let (stdout, stderr, success) = run_mica(source);
    assert!(success, "program should succeed, stderr:\n{}", stderr);
    stdout
}

fn assert_failure(source: &str) -> String {
    let (_, stderr, success) = run_mica(source);
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
    assert!(stderr.contains("cannot assign to immutable"), "stderr: {}", stderr);
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
