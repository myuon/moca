//! Performance benchmark tests that verify JIT optimization provides
//! at least 10% improvement over interpreter baseline.
//!
//! These tests run with `cargo test --features jit perf_`.

use moca::compiler::run_file_capturing_output;
use moca::config::{JitMode, RuntimeConfig};
use std::time::{Duration, Instant};

/// Required improvement ratio (JIT time must be <= baseline * this value)
const IMPROVEMENT_THRESHOLD: f64 = 0.9;

/// Number of warmup runs before measurement
const WARMUP_RUNS: usize = 3;

/// Number of measurement runs to average
const MEASUREMENT_RUNS: usize = 3;

/// Run a benchmark scenario and return execution time.
fn run_benchmark(source: &str, jit_enabled: bool) -> Duration {
    // Create temp file
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("moca_perf_{}.mc", std::process::id()));
    std::fs::write(&temp_file, source).expect("failed to write temp file");

    let config = if jit_enabled {
        RuntimeConfig {
            jit_mode: JitMode::On,
            jit_threshold: 1, // Compile immediately
            ..Default::default()
        }
    } else {
        RuntimeConfig {
            jit_mode: JitMode::Off,
            jit_threshold: u32::MAX, // Never compile
            ..Default::default()
        }
    };

    let start = Instant::now();
    let (_output, result) = run_file_capturing_output(&temp_file, &config);
    let elapsed = start.elapsed();

    // Cleanup
    std::fs::remove_file(&temp_file).ok();

    result.expect("benchmark execution failed");
    elapsed
}

/// Assert that JIT version is at least 10% faster than baseline.
/// Uses warmup runs and averages multiple measurements for stability.
fn assert_optimization_effect(name: &str, source: &str) {
    // Warmup runs (discard results)
    for _ in 0..WARMUP_RUNS {
        run_benchmark(source, false);
        run_benchmark(source, true);
    }

    // Measurement runs
    let mut baseline_times = Vec::with_capacity(MEASUREMENT_RUNS);
    let mut optimized_times = Vec::with_capacity(MEASUREMENT_RUNS);

    for _ in 0..MEASUREMENT_RUNS {
        baseline_times.push(run_benchmark(source, false));
        optimized_times.push(run_benchmark(source, true));
    }

    // Calculate averages
    let baseline_avg: f64 =
        baseline_times.iter().map(|d| d.as_secs_f64()).sum::<f64>() / MEASUREMENT_RUNS as f64;
    let optimized_avg: f64 =
        optimized_times.iter().map(|d| d.as_secs_f64()).sum::<f64>() / MEASUREMENT_RUNS as f64;

    let ratio = optimized_avg / baseline_avg;
    let improvement_pct = (1.0 - ratio) * 100.0;

    println!(
        "[{}] baseline avg: {:.4}s ({:?}), optimized avg: {:.4}s ({:?}), improvement: {:.1}%",
        name, baseline_avg, baseline_times, optimized_avg, optimized_times, improvement_pct
    );

    assert!(
        optimized_avg <= baseline_avg * IMPROVEMENT_THRESHOLD,
        "{}: JIT optimization did not meet 10% improvement threshold.\n\
         baseline avg: {:.4}s, optimized avg: {:.4}s, ratio: {:.3} (need <= {})",
        name,
        baseline_avg,
        optimized_avg,
        ratio,
        IMPROVEMENT_THRESHOLD
    );
}

/// Run a JIT correctness test and return the output.
/// Used for scenarios where JIT correctness matters but performance improvement isn't expected.
fn run_jit_correctness_test(source: &str) -> String {
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("moca_test_{}.mc", std::process::id()));
    std::fs::write(&temp_file, source).expect("failed to write temp file");

    let config = RuntimeConfig {
        jit_mode: JitMode::On,
        jit_threshold: 1, // Compile immediately
        ..Default::default()
    };

    let (output, result) = run_file_capturing_output(&temp_file, &config);
    std::fs::remove_file(&temp_file).ok();
    result.expect("JIT execution failed");
    output.stdout
}

// ============================================================================
// Benchmark Scenarios
// ============================================================================

#[test]
#[cfg(feature = "jit")]
fn perf_sum_loop() {
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

print(sum_to(1000000));
"#;
    assert_optimization_effect("sum_loop", source);
}

#[test]
#[cfg(feature = "jit")]
fn perf_nested_loop() {
    let source = r#"
fun nested(n) {
    var count = 0;
    var i = 0;
    while i < n {
        var j = 0;
        while j < n {
            count = count + 1;
            j = j + 1;
        }
        i = i + 1;
    }
    return count;
}

print(nested(500));
"#;
    assert_optimization_effect("nested_loop", source);
}

#[test]
#[cfg(feature = "jit")]
fn perf_hot_function() {
    let source = r#"
fun do_work(n) {
    var sum = 0;
    var i = 0;
    while i < n {
        sum = sum + i;
        i = i + 1;
    }
    return sum;
}

var total = 0;
var j = 0;
while j < 10000 {
    total = total + do_work(100);
    j = j + 1;
}
print(total);
"#;
    assert_optimization_effect("hot_function", source);
}

#[test]
#[cfg(feature = "jit")]
#[cfg_attr(
    target_arch = "aarch64",
    ignore = "aarch64 JIT does not yet have emit_call_self optimization"
)]
fn perf_fibonacci() {
    let source = r#"
fun fib(n) {
    if n <= 1 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

print(fib(30));
"#;
    assert_optimization_effect("fibonacci", source);
}

#[test]
#[cfg(feature = "jit")]
#[ignore = "JIT does not support AllocArray, CallBuiltin (push/len), GetIndex yet"]
fn perf_array_operations() {
    let source = r#"
fun array_sum(n) {
    var arr = [];
    var i = 0;
    while i < n {
        push(arr, i);
        i = i + 1;
    }

    var sum = 0;
    var j = 0;
    while j < len(arr) {
        sum = sum + arr[j];
        j = j + 1;
    }
    return sum;
}

print(array_sum(100000));
"#;
    assert_optimization_effect("array_operations", source);
}

#[test]
#[cfg(feature = "jit")]
fn jit_mutual_recursion() {
    // Test mutual recursion: is_even calls is_odd, is_odd calls is_even
    // This tests that emit_call_external (non-self recursion) works correctly.
    //
    // Note: Mutual recursion through jit_call_helper has overhead that makes
    // it slower than the interpreter. This test verifies correctness only.
    // Performance improvement would require implementing mutual call optimization
    // similar to emit_call_self.
    let source = r#"
fun is_even(n) {
    if n == 0 {
        return 1;
    }
    return is_odd(n - 1);
}

fun is_odd(n) {
    if n == 0 {
        return 0;
    }
    return is_even(n - 1);
}

// Test various inputs
print(is_even(0));   // 1 (0 is even)
print(is_even(1));   // 0 (1 is odd)
print(is_even(10));  // 1 (10 is even)
print(is_even(11));  // 0 (11 is odd)
"#;
    let output = run_jit_correctness_test(source);
    assert_eq!(output.trim(), "1\n0\n1\n0");
}
