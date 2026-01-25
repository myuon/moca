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
let mut i = 0;
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
fn fizzbuzz(n) {
    let mut i = 1;
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
fn fib(n) {
    if n <= 1 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

let mut i = 0;
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
fn fact(n) {
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
