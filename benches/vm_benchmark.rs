//! VM performance benchmarks comparing interpreter vs quickening/JIT modes.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::process::Command;
use std::time::Duration;

/// Run mica with the given source code and CLI args, return execution time
fn run_mica_timed(source: &str, args: &[&str]) -> Duration {
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("mica_bench_{}.mica", std::process::id()));
    std::fs::write(&temp_file, source).unwrap();

    let start = std::time::Instant::now();

    let mut cmd_args = vec!["run"];
    cmd_args.extend(args);
    cmd_args.push(temp_file.to_str().unwrap());

    let output = Command::new(env!("CARGO_BIN_EXE_mica"))
        .args(&cmd_args)
        .output()
        .expect("failed to execute mica");

    let elapsed = start.elapsed();

    assert!(
        output.status.success(),
        "benchmark should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_file(&temp_file).ok();

    elapsed
}

/// Fibonacci benchmark - tests recursive function calls
fn fibonacci_source(n: u32) -> String {
    format!(
        r#"
fun fib(n) {{
    if n <= 1 {{
        return n;
    }}
    return fib(n - 1) + fib(n - 2);
}}

print(fib({}));
"#,
        n
    )
}

/// Sum loop benchmark - tests loop performance and arithmetic
fn sum_loop_source(n: u32) -> String {
    format!(
        r#"
fun sum_to(n) {{
    var total = 0;
    var i = 1;
    while i <= n {{
        total = total + i;
        i = i + 1;
    }}
    return total;
}}

print(sum_to({}));
"#,
        n
    )
}

/// Nested loop benchmark - tests nested loop performance
fn nested_loop_source(n: u32) -> String {
    format!(
        r#"
fun nested(n) {{
    var count = 0;
    var i = 0;
    while i < n {{
        var j = 0;
        while j < n {{
            count = count + 1;
            j = j + 1;
        }}
        i = i + 1;
    }}
    return count;
}}

print(nested({}));
"#,
        n
    )
}

/// Array benchmark - tests array allocation and access
fn array_bench_source(n: u32) -> String {
    format!(
        r#"
fun array_sum(n) {{
    var arr = [];
    var i = 0;
    while i < n {{
        push(arr, i);
        i = i + 1;
    }}

    var sum = 0;
    var j = 0;
    while j < len(arr) {{
        sum = sum + arr[j];
        j = j + 1;
    }}
    return sum;
}}

print(array_sum({}));
"#,
        n
    )
}

/// Hot function benchmark - tests JIT compilation benefit
fn hot_function_source(calls: u32, work: u32) -> String {
    format!(
        r#"
fun do_work(n) {{
    var sum = 0;
    var i = 0;
    while i < n {{
        sum = sum + i;
        i = i + 1;
    }}
    return sum;
}}

var total = 0;
var j = 0;
while j < {} {{
    total = total + do_work({});
    j = j + 1;
}}
print(total);
"#,
        calls, work
    )
}

fn bench_interpreter_vs_quickening(c: &mut Criterion) {
    let mut group = c.benchmark_group("interpreter_vs_quickening");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(5));

    // Sum loop benchmark (target: ~500ms)
    // 300000 iterations took ~55ms, so 2500000 should take ~500ms
    let source = sum_loop_source(2_500_000);
    group.bench_function("sum_loop_interpreter", |b| {
        b.iter(|| run_mica_timed(black_box(&source), &["--jit=off"]))
    });
    group.bench_function("sum_loop_quickening", |b| {
        b.iter(|| run_mica_timed(black_box(&source), &["--jit=on"]))
    });

    // Nested loop benchmark (target: ~500ms)
    // 550x550 took ~55ms, so 1600x1600 should take ~500ms
    let source = nested_loop_source(1600);
    group.bench_function("nested_loop_interpreter", |b| {
        b.iter(|| run_mica_timed(black_box(&source), &["--jit=off"]))
    });
    group.bench_function("nested_loop_quickening", |b| {
        b.iter(|| run_mica_timed(black_box(&source), &["--jit=on"]))
    });

    // Hot function benchmark (target: ~500ms)
    // 15000x100 took ~210ms, so 35000x100 should take ~500ms
    let source = hot_function_source(35_000, 100);
    group.bench_function("hot_function_interpreter", |b| {
        b.iter(|| run_mica_timed(black_box(&source), &["--jit=off"]))
    });
    group.bench_function("hot_function_quickening", |b| {
        b.iter(|| run_mica_timed(black_box(&source), &["--jit=on", "--jit-threshold=100"]))
    });

    group.finish();
}

fn bench_fibonacci(c: &mut Criterion) {
    let mut group = c.benchmark_group("fibonacci");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(5));

    // fib(25) took ~45ms, fib grows exponentially (~1.6x per n increase)
    // fib(30) should be ~500ms, fib(32) should be ~1.3s
    for n in [28, 30, 32] {
        let source = fibonacci_source(n);
        group.bench_with_input(BenchmarkId::new("interpreter", n), &source, |b, s| {
            b.iter(|| run_mica_timed(black_box(s), &["--jit=off"]))
        });
        group.bench_with_input(BenchmarkId::new("quickening", n), &source, |b, s| {
            b.iter(|| run_mica_timed(black_box(s), &["--jit=on"]))
        });
    }

    group.finish();
}

fn bench_array_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("array_operations");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(5));

    // 50000 elements took ~35ms, so we need larger sizes for ~500ms
    for n in [100_000, 300_000, 500_000] {
        let source = array_bench_source(n);
        group.bench_with_input(BenchmarkId::new("interpreter", n), &source, |b, s| {
            b.iter(|| run_mica_timed(black_box(s), &["--jit=off"]))
        });
        group.bench_with_input(BenchmarkId::new("quickening", n), &source, |b, s| {
            b.iter(|| run_mica_timed(black_box(s), &["--jit=on"]))
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_interpreter_vs_quickening,
    bench_fibonacci,
    bench_array_operations
);
criterion_main!(benches);
