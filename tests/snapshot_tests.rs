//! Snapshot tests for the moca compiler.
//!
//! All tests run in-process to contribute to coverage measurement.

use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use moca::compiler::{dump_ast, dump_bytecode, lint_file, run_file_capturing_output, run_tests};
use moca::config::{JitMode, RuntimeConfig};

/// Run a .mc file in-process and return (stdout, stderr, exit_code, jit_compile_count)
fn run_moca_file_inprocess(path: &Path, config: &RuntimeConfig) -> (String, String, i32, usize) {
    let (output, result) = run_file_capturing_output(path, config);

    match result {
        Ok(()) => (output.stdout, output.stderr, 0, output.jit_compile_count),
        Err(e) => {
            // Combine captured stderr with error message
            let stderr = if output.stderr.is_empty() {
                e
            } else {
                format!("{}{}", output.stderr, e)
            };
            (output.stdout, stderr, 1, output.jit_compile_count)
        }
    }
}

/// Get RuntimeConfig based on directory name
fn get_config_for_dir(dir_name: &str) -> RuntimeConfig {
    match dir_name {
        "jit" => RuntimeConfig {
            jit_mode: JitMode::On,
            jit_threshold: 1, // Low threshold to trigger JIT quickly in tests
            ..RuntimeConfig::default()
        },
        _ => RuntimeConfig::default(),
    }
}

/// Check if a test is a dump test (FFI tests that dump AST/bytecode)
fn is_dump_test(base_path: &Path) -> Option<&'static str> {
    let name = base_path.file_name()?.to_str()?;
    if name.starts_with("dump_ast") {
        Some("ast")
    } else if name.starts_with("dump_bytecode") {
        Some("bytecode")
    } else {
        None
    }
}

/// Run a dump test and return (stdout, stderr, exit_code)
/// Dump tests both run the program AND dump AST/bytecode
fn run_dump_test(path: &Path, dump_type: &str) -> (String, String, i32) {
    // First get the dump output
    let dump_result = match dump_type {
        "ast" => dump_ast(path),
        "bytecode" => dump_bytecode(path),
        _ => {
            return (
                String::new(),
                format!("unknown dump type: {}", dump_type),
                1,
            );
        }
    };

    let dump_output = match dump_result {
        Ok(output) => output,
        Err(e) => return (String::new(), e, 1),
    };

    // Then run the program to get stdout
    let (output, result) = run_file_capturing_output(path, &RuntimeConfig::default());

    match result {
        Ok(()) => {
            // Format stderr like CLI does: "== AST ==" or "== Bytecode ==" header
            let header = match dump_type {
                "ast" => "== AST ==",
                "bytecode" => "== Bytecode ==",
                _ => "",
            };
            let stderr = format!("{}\n{}", header, dump_output);
            (output.stdout, stderr, 0)
        }
        Err(e) => (output.stdout, e, 1),
    }
}

/// Run a single snapshot test (file-based or directory-based)
fn run_snapshot_test(test_path: &Path, dir_name: &str) {
    // Determine if this is a directory-based test or file-based test
    let (moca_path, base_path) = if test_path.is_dir() {
        // Directory-based test: look for main.mc as entry point
        let main_moca = test_path.join("main.mc");
        if !main_moca.exists() {
            panic!("Directory test {:?} must contain main.mc", test_path);
        }
        // Expected output files are at the directory level (e.g., testdir.stdout)
        (main_moca, test_path.to_path_buf())
    } else {
        // File-based test
        (test_path.to_path_buf(), test_path.with_extension(""))
    };

    // Determine how to run the test
    let (actual_stdout, actual_stderr, actual_exitcode, jit_compile_count) =
        if let Some(dump_type) = is_dump_test(&base_path) {
            // Dump test (AST or bytecode)
            let (stdout, stderr, exitcode) = run_dump_test(&moca_path, dump_type);
            (stdout, stderr, exitcode, 0)
        } else {
            // Regular test
            let config = get_config_for_dir(dir_name);
            run_moca_file_inprocess(&moca_path, &config)
        };

    // For JIT tests, verify that JIT compilation actually occurred
    if dir_name == "jit" && actual_exitcode == 0 {
        assert!(
            jit_compile_count > 0,
            "JIT test {:?}: no functions were JIT compiled",
            moca_path
        );
    }

    // Check stdout (exact match)
    let stdout_path = base_path.with_extension("stdout");
    if stdout_path.exists() {
        let expected_stdout = fs::read_to_string(&stdout_path)
            .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", stdout_path, e));
        assert_eq!(
            actual_stdout, expected_stdout,
            "stdout mismatch for {:?}\n--- expected ---\n{}\n--- actual ---\n{}",
            moca_path, expected_stdout, actual_stdout
        );
    }

    // Check stderr (partial match - expected must be contained in actual)
    let stderr_path = base_path.with_extension("stderr");
    if stderr_path.exists() {
        let expected_stderr = fs::read_to_string(&stderr_path)
            .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", stderr_path, e));
        assert!(
            actual_stderr.contains(&expected_stderr),
            "stderr mismatch for {:?}\n--- expected (substring) ---\n{}\n--- actual ---\n{}",
            moca_path,
            expected_stderr,
            actual_stderr
        );
    }

    // Check exit code (default: 0)
    let exitcode_path = base_path.with_extension("exitcode");
    let expected_exitcode = if exitcode_path.exists() {
        fs::read_to_string(&exitcode_path)
            .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", exitcode_path, e))
            .trim()
            .parse::<i32>()
            .unwrap_or_else(|e| panic!("Invalid exitcode in {:?}: {}", exitcode_path, e))
    } else {
        0
    };

    assert_eq!(
        actual_exitcode, expected_exitcode,
        "exit code mismatch for {:?}: expected {}, got {}",
        test_path, expected_exitcode, actual_exitcode
    );
}

/// Discover and run all tests in a directory
/// Supports both file-based tests (.mc files) and directory-based tests (subdirectories with main.mc)
fn run_snapshot_dir(dir: &str) {
    let dir_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(dir);

    if !dir_path.exists() {
        return;
    }

    let entries: Vec<_> = fs::read_dir(&dir_path)
        .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", dir_path, e))
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            // Include .mc files
            if path.extension().map_or(false, |ext| ext == "mc") {
                return true;
            }
            // Include directories that contain main.mc (multi-file tests)
            if path.is_dir() && path.join("main.mc").exists() {
                return true;
            }
            false
        })
        .collect();

    for entry in entries {
        let path = entry.path();
        run_snapshot_test(&path, dir);
    }
}

#[test]
fn snapshot_basic() {
    run_snapshot_dir("basic");
}

#[test]
fn snapshot_asm() {
    run_snapshot_dir("asm");
}

#[test]
fn snapshot_errors() {
    run_snapshot_dir("errors");
}

#[test]
fn snapshot_jit() {
    run_snapshot_dir("jit");
}

#[test]
fn snapshot_modules() {
    run_snapshot_dir("modules");
}

#[test]
fn snapshot_ffi() {
    run_snapshot_dir("ffi");
}

#[test]
fn snapshot_generics() {
    run_snapshot_dir("generics");
}

#[test]
fn snapshot_gc() {
    run_gc_snapshot_dir("gc");
}

/// Run GC-specific snapshot tests.
/// For each .mc file, runs with GC enabled (should succeed).
/// If a corresponding .gc_disabled.mc file exists, runs it with GC disabled
/// and a small heap limit (should fail with heap limit exceeded error).
fn run_gc_snapshot_dir(dir: &str) {
    let dir_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(dir);

    if !dir_path.exists() {
        return;
    }

    let entries: Vec<_> = fs::read_dir(&dir_path)
        .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", dir_path, e))
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            // Include .mc files that don't have .gc_disabled in the name
            path.extension().map_or(false, |ext| ext == "mc")
                && !path
                    .file_stem()
                    .map_or(false, |s| s.to_string_lossy().ends_with(".gc_disabled"))
        })
        .collect();

    for entry in entries {
        let path = entry.path();
        run_gc_snapshot_test(&path);
    }
}

/// Run a GC snapshot test for a single .mc file.
fn run_gc_snapshot_test(test_path: &Path) {
    let base_path = test_path.with_extension("");
    let file_stem = test_path.file_stem().unwrap().to_string_lossy();

    // 1. Run with GC enabled (normal mode) - should succeed
    {
        let config = RuntimeConfig::default();
        let (actual_stdout, actual_stderr, actual_exitcode, _) =
            run_moca_file_inprocess(test_path, &config);

        // Check expected stdout if exists
        let stdout_path = base_path.with_extension("stdout");
        if stdout_path.exists() {
            let expected_stdout = fs::read_to_string(&stdout_path)
                .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", stdout_path, e));
            assert_eq!(
                actual_stdout, expected_stdout,
                "stdout mismatch for {:?} (GC enabled)\n--- expected ---\n{}\n--- actual ---\n{}",
                test_path, expected_stdout, actual_stdout
            );
            assert_eq!(
                actual_exitcode, 0,
                "exit code should be 0 for {:?} (GC enabled), stderr: {}",
                test_path, actual_stderr
            );
        } else {
            // Just verify it succeeds
            assert_eq!(
                actual_exitcode, 0,
                "GC enabled test should succeed for {:?}, got error: {}",
                test_path, actual_stderr
            );
        }
    }

    // 2. Check for .gc_disabled.mc file
    let gc_disabled_path = test_path
        .parent()
        .unwrap()
        .join(format!("{}.gc_disabled.mc", file_stem));

    if gc_disabled_path.exists() {
        // Run with GC disabled and small heap limit - should fail
        let config = RuntimeConfig {
            gc_enabled: false,
            heap_limit: Some(50 * 1024), // 50KB heap limit
            ..Default::default()
        };

        let (_, actual_stderr, actual_exitcode, _) =
            run_moca_file_inprocess(&gc_disabled_path, &config);

        // Check expected stderr if exists
        let gc_disabled_base = gc_disabled_path.with_extension("");
        let stderr_path = gc_disabled_base.with_extension("stderr");

        // Should fail (exit code != 0)
        assert_ne!(
            actual_exitcode, 0,
            "GC disabled test should fail for {:?}, but it succeeded",
            gc_disabled_path
        );

        if stderr_path.exists() {
            let expected_stderr = fs::read_to_string(&stderr_path)
                .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", stderr_path, e));
            assert!(
                actual_stderr.contains(&expected_stderr),
                "stderr mismatch for {:?} (GC disabled)\n--- expected (substring) ---\n{}\n--- actual ---\n{}",
                gc_disabled_path,
                expected_stderr,
                actual_stderr
            );
        } else {
            // Just verify it fails with heap limit exceeded
            assert!(
                actual_stderr.contains("heap limit exceeded"),
                "Error should mention heap limit exceeded for {:?}, got: {}",
                gc_disabled_path,
                actual_stderr
            );
        }
    }
}

// ============================================================================
// Lint Snapshot Tests
// ============================================================================

/// Run lint snapshot tests.
///
/// Each test has a `.mc` source file and a `.lint` expected file.
/// The `.lint` file contains one line per expected diagnostic in the format:
///   {rule}:{line}:{column}
/// Empty `.lint` files mean no warnings are expected.
fn run_lint_snapshot_dir(dir: &str) {
    let dir_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(dir);

    if !dir_path.exists() {
        return;
    }

    let entries: Vec<_> = fs::read_dir(&dir_path)
        .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", dir_path, e))
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "mc"))
        .collect();

    for entry in entries {
        let mc_path = entry.path();
        let base_path = mc_path.with_extension("");
        let lint_path = base_path.with_extension("lint");

        // Run the linter
        let (output, count) = match lint_file(&mc_path) {
            Ok(result) => result,
            Err(e) => panic!("lint_file failed for {:?}: {}", mc_path, e),
        };

        // Parse actual diagnostics into (rule, line, column) tuples
        let actual: Vec<(String, usize, usize)> = if count == 0 {
            Vec::new()
        } else {
            parse_lint_output(&output)
        };

        // Parse expected diagnostics from .lint file
        let expected: Vec<(String, usize, usize)> = if lint_path.exists() {
            let content = fs::read_to_string(&lint_path)
                .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", lint_path, e));
            parse_lint_expected(&content)
        } else {
            Vec::new()
        };

        assert_eq!(
            actual, expected,
            "lint mismatch for {:?}\n--- expected ---\n{:?}\n--- actual ---\n{:?}\n--- raw output ---\n{}",
            mc_path, expected, actual, output
        );
    }
}

/// Parse lint output into (rule, line, column) tuples.
///
/// Input format:
///   warning: {rule}: {message}
///     --> {filename}:{line}:{column}
fn parse_lint_output(output: &str) -> Vec<(String, usize, usize)> {
    let mut results = Vec::new();
    let lines: Vec<&str> = output.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix("warning: ") {
            // Extract rule name (everything before the second colon)
            if let Some(colon_pos) = rest.find(':') {
                let rule = rest[..colon_pos].to_string();
                // Next line should be the location
                if i + 1 < lines.len() {
                    let loc_line = lines[i + 1].trim();
                    if let Some(arrow_rest) = loc_line.strip_prefix("--> ") {
                        // Parse {filename}:{line}:{column}
                        let parts: Vec<&str> = arrow_rest.rsplitn(3, ':').collect();
                        if parts.len() == 3 {
                            let col: usize = parts[0].parse().unwrap_or(0);
                            let ln: usize = parts[1].parse().unwrap_or(0);
                            results.push((rule, ln, col));
                        }
                    }
                    i += 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    results
}

/// Parse expected lint file into (rule, line, column) tuples.
///
/// Input format: one line per diagnostic: `{rule}:{line}:{column}`
fn parse_lint_expected(content: &str) -> Vec<(String, usize, usize)> {
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let parts: Vec<&str> = line.trim().splitn(3, ':').collect();
            assert_eq!(
                parts.len(),
                3,
                "invalid lint expected format: '{}' (expected 'rule:line:column')",
                line
            );
            let rule = parts[0].to_string();
            let ln: usize = parts[1]
                .parse()
                .unwrap_or_else(|_| panic!("invalid line number in: {}", line));
            let col: usize = parts[2]
                .parse()
                .unwrap_or_else(|_| panic!("invalid column number in: {}", line));
            (rule, ln, col)
        })
        .collect()
}

#[test]
fn snapshot_lint() {
    run_lint_snapshot_dir("lint");
}

// ============================================================================
// Test Runner Snapshot Tests
// ============================================================================

/// Format test results as CLI output (same format as `moca test` command).
fn format_test_results(results: &moca::compiler::TestResults) -> String {
    let mut output = String::new();

    // Sort results by name for deterministic output
    let mut sorted_results = results.results.clone();
    sorted_results.sort_by(|a, b| a.name.cmp(&b.name));

    for result in &sorted_results {
        if result.passed {
            output.push_str(&format!("\u{2713} {} passed\n", result.name));
        } else {
            let error_msg = result.error.as_deref().unwrap_or("unknown error");
            output.push_str(&format!("\u{2717} {} failed: {}\n", result.name, error_msg));
        }
    }

    output.push('\n');
    output.push_str(&format!(
        "{} passed, {} failed\n",
        results.passed, results.failed
    ));

    output
}

/// Run test runner snapshot test for a given subdirectory.
fn run_test_runner_snapshot(subdir: &str) {
    let base_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("test_runner")
        .join(subdir);

    let config = RuntimeConfig::default();
    let results = run_tests(&base_path, &config).expect("run_tests should succeed");

    let actual_output = format_test_results(&results);

    // Check expected stdout
    let stdout_path = base_path.with_extension("stdout");
    if stdout_path.exists() {
        let expected_output = fs::read_to_string(&stdout_path)
            .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", stdout_path, e));
        assert_eq!(
            actual_output, expected_output,
            "Test runner output mismatch for {:?}\n--- expected ---\n{}\n--- actual ---\n{}",
            subdir, expected_output, actual_output
        );
    } else {
        panic!(
            "Expected stdout file not found: {:?}\nActual output:\n{}",
            stdout_path, actual_output
        );
    }

    // Check expected exit code (0 = all pass, 1 = some fail)
    let exitcode_path = base_path.with_extension("exitcode");
    let expected_exitcode = if exitcode_path.exists() {
        fs::read_to_string(&exitcode_path)
            .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", exitcode_path, e))
            .trim()
            .parse::<i32>()
            .unwrap_or(0)
    } else {
        0
    };

    let actual_exitcode = if results.all_passed() { 0 } else { 1 };
    assert_eq!(
        actual_exitcode, expected_exitcode,
        "Exit code mismatch for {:?}: expected {}, got {}",
        subdir, expected_exitcode, actual_exitcode
    );
}

#[test]
fn snapshot_test_runner_passing() {
    run_test_runner_snapshot("passing");
}

#[test]
fn snapshot_test_runner_failing() {
    run_test_runner_snapshot("failing");
}

#[test]
fn snapshot_test_runner_mixed() {
    run_test_runner_snapshot("mixed");
}

// ============================================================================
// Standard Library Tests
// ============================================================================

/// Run tests for the standard library using the test runner.
/// This ensures all stdlib functions work correctly.
#[test]
fn snapshot_stdlib() {
    let std_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("std");

    let config = RuntimeConfig::default();
    let results = run_tests(&std_path, &config).expect("run_tests should succeed for stdlib");

    // All stdlib tests should pass
    assert!(
        results.all_passed(),
        "All stdlib tests should pass.\n\
         Passed: {}, Failed: {}\n\
         Failed tests:\n{}",
        results.passed,
        results.failed,
        results
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| format!(
                "  - {}: {}",
                r.name,
                r.error.as_deref().unwrap_or("unknown error")
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ============================================================================
// HTTP Server Snapshot Tests
// ============================================================================

/// Test that a Moca HTTP server can accept connections and respond to requests.
/// This test starts a Moca HTTP server in a background thread, sends a request,
/// and verifies the response.
///
/// Uses http_server.mc.template with {{PORT}} placeholder.
#[test]
fn snapshot_http_server() {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::mpsc;
    use std::time::Duration;

    let http_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("http");

    let template_path = http_dir.join("http_server.mc.template");
    if !template_path.exists() {
        panic!("Template file not found: {:?}", template_path);
    }

    // Find an available port
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener); // Release the port so Moca can use it

    // Read template and replace {{PORT}} with actual port
    let template_content = fs::read_to_string(&template_path)
        .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", template_path, e));
    let moca_content = template_content.replace("{{PORT}}", &port.to_string());

    // Write to a temporary file
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join("http_server_test.mc");
    fs::write(&temp_file, &moca_content).expect("Failed to write temp file");

    // Channel to receive server result
    let (tx, rx) = mpsc::channel();

    // Start the Moca server in a background thread
    let temp_file_clone = temp_file.clone();
    let server_thread = std::thread::spawn(move || {
        let config = RuntimeConfig::default();
        let result = run_moca_file_inprocess(&temp_file_clone, &config);
        let _ = tx.send(result);
    });

    // Wait for server to start
    std::thread::sleep(Duration::from_millis(500));

    // Send an HTTP request
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .expect("Failed to connect to Moca server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let request = "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    stream
        .write_all(request.as_bytes())
        .expect("Failed to send request");

    // Read response
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("Failed to read response");

    // Verify HTTP response
    assert!(
        response.contains("HTTP/1.1 200 OK"),
        "Response should contain HTTP 200 OK status: {}",
        response
    );
    assert!(
        response.contains("Hello from Moca!"),
        "Response should contain expected body: {}",
        response
    );

    // Wait for server thread to finish
    server_thread.join().expect("Server thread panicked");

    // Verify server output
    let (actual_stdout, actual_stderr, actual_exitcode, _) =
        rx.recv().expect("Failed to receive server result");

    // Check expected stdout
    let stdout_path = http_dir.join("http_server.stdout");
    if stdout_path.exists() {
        let expected_stdout = fs::read_to_string(&stdout_path)
            .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", stdout_path, e));
        assert_eq!(
            actual_stdout, expected_stdout,
            "stdout mismatch for http_server\n--- expected ---\n{}\n--- actual ---\n{}",
            expected_stdout, actual_stdout
        );
    }

    // Check exit code
    assert_eq!(
        actual_exitcode, 0,
        "Server should exit with code 0, stderr: {}",
        actual_stderr
    );

    // Clean up
    let _ = fs::remove_file(&temp_file);
}

// ============================================================================
// HTTP Client Snapshot Tests
// ============================================================================

/// Run HTTP snapshot tests with a local hyper-based test server.
/// These tests require a running HTTP server and use template files
/// with {{PORT}} placeholder that gets replaced with the actual port.
///
/// Endpoints:
/// - GET / : Returns "Hello from test server!"
/// - POST /echo : Returns the request body as-is
// ============================================================================
// Performance Benchmark Tests
// ============================================================================

/// Performance test configuration
#[cfg(feature = "jit")]
const PERF_WARMUP_RUNS: usize = 3;
#[cfg(feature = "jit")]
const PERF_MEASUREMENT_RUNS: usize = 3;

// Rust reference implementations for correctness verification
// Each function writes output to a Writer to match moca's I/O behavior
#[cfg(feature = "jit")]
fn rust_sum_loop<W: Write>(writer: &mut W) {
    let mut sum: i64 = 0;
    for i in 1..=10_000_000 {
        sum += i;
    }
    writeln!(writer, "{}", sum).unwrap();
}

#[cfg(feature = "jit")]
fn rust_nested_loop<W: Write>(n: i64, writer: &mut W) {
    let mut sum: i64 = 0;
    for i in 0..n {
        for j in 0..n {
            sum = std::hint::black_box(sum + i * j);
        }
    }
    writeln!(writer, "{}", sum).unwrap();
}

#[cfg(feature = "jit")]
fn rust_fibonacci_impl(n: i32) -> i32 {
    if n <= 1 {
        n
    } else {
        rust_fibonacci_impl(n - 1) + rust_fibonacci_impl(n - 2)
    }
}

#[cfg(feature = "jit")]
fn rust_fibonacci<W: Write>(n: i32, writer: &mut W) {
    let result = rust_fibonacci_impl(n);
    writeln!(writer, "{}", result).unwrap();
}

#[cfg(feature = "jit")]
fn rust_mandelbrot<W: Write>(max_iter: i32, writer: &mut W) {
    let width = 80;
    let height = 24;
    let mut escape_count = 0;

    let x_min = -2.0_f64;
    let x_max = 1.0_f64;
    let y_min = -1.0_f64;
    let y_max = 1.0_f64;

    let x_step = (x_max - x_min) / 80.0;
    let y_step = (y_max - y_min) / 24.0;

    let mut cy = y_min;
    for _ in 0..height {
        let mut cx = x_min;
        for _ in 0..width {
            let mut zr = 0.0_f64;
            let mut zi = 0.0_f64;
            let mut iter = 0;

            while iter < max_iter {
                let zr2 = zr * zr;
                let zi2 = zi * zi;

                if zr2 + zi2 > 4.0 {
                    escape_count += 1;
                    break;
                }

                let new_zr = zr2 - zi2 + cx;
                let new_zi = 2.0 * zr * zi + cy;
                zr = new_zr;
                zi = new_zi;
                iter += 1;
            }

            cx += x_step;
        }
        cy += y_step;
    }

    writeln!(writer, "{}", escape_count).unwrap();
}

#[cfg(feature = "jit")]
fn rust_is_even(n: i32) -> i32 {
    if n == 0 {
        1
    } else {
        rust_is_odd(std::hint::black_box(n - 1))
    }
}

#[cfg(feature = "jit")]
fn rust_is_odd(n: i32) -> i32 {
    if n == 0 {
        0
    } else {
        rust_is_even(std::hint::black_box(n - 1))
    }
}

#[cfg(feature = "jit")]
fn rust_mutual_recursion<W: Write>(iterations: i32, writer: &mut W) {
    let mut sum: i64 = 0;
    for i in 0..iterations {
        sum += rust_is_even(std::hint::black_box(i % 200)) as i64;
    }
    writeln!(writer, "{}", sum).unwrap();
}

#[cfg(feature = "jit")]
fn rust_to_letter_index(ch: u8) -> i64 {
    if ch >= 65 && ch <= 90 {
        return (ch - 65) as i64;
    }
    if ch >= 97 && ch <= 122 {
        return (ch - 97) as i64;
    }
    -1
}

#[cfg(feature = "jit")]
fn rust_text_counting<W: Write>(writer: &mut W) {
    let text = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.";

    let labels = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    let mut counts = [0i64; 26];

    for _ in 0..1000 {
        for &ch in text.iter() {
            let idx = rust_to_letter_index(ch);
            if idx >= 0 {
                counts[idx as usize] += 1;
            }
        }
    }

    // Find and print top 10 by frequency
    for _ in 0..10 {
        let mut max_idx = 0;
        let mut max_val = counts[0];
        for j in 1..26 {
            if counts[j] > max_val {
                max_val = counts[j];
                max_idx = j;
            }
        }
        writeln!(writer, "{}: {}", labels[max_idx], max_val).unwrap();
        counts[max_idx] = -1;
    }
}

#[cfg(feature = "jit")]
fn rust_quicksort<W: Write>(writer: &mut W) {
    // Same LCG as moca _perf_lcg_next
    let mut seed: i64 = 42;
    let mut v: Vec<i64> = Vec::with_capacity(1000);
    for _ in 0..1000 {
        seed = (seed * 1103515245 + 12345) % 2147483648;
        if seed < 0 {
            seed = -seed;
        }
        v.push(seed % 10000);
    }

    v.sort();

    for val in &v {
        writeln!(writer, "{}", val).unwrap();
    }
}

/// Run a moca file with JIT enabled and measure execution time
#[cfg(feature = "jit")]
fn run_performance_benchmark(path: &Path) -> (std::time::Duration, String, usize) {
    use std::time::Instant;

    let config = RuntimeConfig {
        jit_mode: JitMode::On,
        jit_threshold: 1,
        ..Default::default()
    };

    let start = Instant::now();
    let (output, result) = run_file_capturing_output(path, &config);
    let elapsed = start.elapsed();

    result.unwrap_or_else(|e| panic!("Benchmark execution failed for {:?}: {}", path, e));

    (elapsed, output.stdout, output.jit_compile_count)
}

/// Run a performance test for a single .mc file
/// Compares moca JIT performance against Rust reference implementation
/// rust_impl writes output to a Writer, which is compared against moca's output
/// to both verify correctness and ensure fair I/O condition comparison
#[cfg(feature = "jit")]
fn run_performance_test<F>(test_path: &Path, rust_impl: F)
where
    F: Fn(&mut Cursor<Vec<u8>>),
{
    use std::time::{Duration, Instant};

    let test_name = test_path.file_stem().unwrap().to_string_lossy().to_string();

    // Warmup runs (discard results)
    for _ in 0..PERF_WARMUP_RUNS {
        run_performance_benchmark(test_path);
        let mut buffer = Cursor::new(Vec::new());
        rust_impl(&mut buffer);
    }

    // Measurement runs
    let mut moca_times: Vec<Duration> = Vec::with_capacity(PERF_MEASUREMENT_RUNS);
    let mut rust_times: Vec<Duration> = Vec::with_capacity(PERF_MEASUREMENT_RUNS);
    let mut moca_output = String::new();
    let mut jit_compile_count = 0;
    let mut rust_result = String::new();

    for _ in 0..PERF_MEASUREMENT_RUNS {
        let (elapsed, output, count) = run_performance_benchmark(test_path);
        moca_times.push(elapsed);
        moca_output = output;
        jit_compile_count = count;

        // Run Rust version with stdout capture (same I/O conditions as moca)
        let start = Instant::now();
        let mut buffer = Cursor::new(Vec::new());
        rust_impl(&mut buffer);
        rust_result = String::from_utf8(buffer.into_inner()).unwrap();
        rust_times.push(start.elapsed());
    }

    // Verify JIT compilation occurred
    assert!(
        jit_compile_count > 0,
        "[{}] JIT was enabled but no functions were compiled",
        test_name
    );

    // Verify output correctness: compare moca output with Rust result
    // This also prevents compiler from optimizing away the Rust computation
    assert_eq!(
        moca_output.trim(),
        rust_result.trim(),
        "[{}] Moca output doesn't match Rust reference implementation",
        test_name
    );

    // Calculate averages
    let moca_avg =
        moca_times.iter().map(|d| d.as_secs_f64()).sum::<f64>() / PERF_MEASUREMENT_RUNS as f64;
    let rust_avg =
        rust_times.iter().map(|d| d.as_secs_f64()).sum::<f64>() / PERF_MEASUREMENT_RUNS as f64;

    let vs_rust = if rust_avg > 0.0 {
        moca_avg / rust_avg
    } else {
        0.0
    };

    println!(
        "[{}] moca: {:.4}s, Rust: {:.4}s, vs_rust: {:.1}x",
        test_name, moca_avg, rust_avg, vs_rust
    );
}

#[test]
#[cfg(feature = "jit")]
fn snapshot_performance() {
    let perf_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("performance");

    if !perf_dir.exists() {
        panic!("Performance test directory not found: {:?}", perf_dir);
    }

    // Test sum_loop with Rust reference
    let sum_loop_path = perf_dir.join("sum_loop.mc");
    run_performance_test(&sum_loop_path, |w| rust_sum_loop(w));

    // Test nested_loop with Rust reference
    let nested_loop_path = perf_dir.join("nested_loop.mc");
    run_performance_test(&nested_loop_path, |w| {
        rust_nested_loop(std::hint::black_box(3000), w)
    });

    // Test mandelbrot with Rust reference
    let mandelbrot_path = perf_dir.join("mandelbrot.mc");
    run_performance_test(&mandelbrot_path, |w| rust_mandelbrot(5000, w));

    // Test fibonacci with Rust reference
    let fibonacci_path = perf_dir.join("fibonacci.mc");
    run_performance_test(&fibonacci_path, |w| {
        rust_fibonacci(std::hint::black_box(35), w)
    });

    // Test mutual recursion (is_even/is_odd) with Rust reference
    let mutual_recursion_path = perf_dir.join("mutual_recursion.mc");
    run_performance_test(&mutual_recursion_path, |w| {
        rust_mutual_recursion(std::hint::black_box(20000), w)
    });

    // Test text character counting with Rust reference
    let text_counting_path = perf_dir.join("text_counting.mc");
    run_performance_test(&text_counting_path, |w| rust_text_counting(w));

    // Test quicksort with Rust reference
    let quicksort_path = perf_dir.join("quicksort.mc");
    run_performance_test(&quicksort_path, |w| rust_quicksort(w));
}

// ============================================================================
// HTTP Client Snapshot Tests
// ============================================================================

#[test]
fn snapshot_http() {
    use http_body_util::{BodyExt, Full};
    use hyper::body::{Bytes, Incoming};
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{Method, Request, Response, StatusCode};
    use hyper_util::rt::TokioIo;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    async fn handle_request(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
        match (req.method(), req.uri().path()) {
            (&Method::GET, "/") => {
                let body = "Hello from test server!";
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "text/plain")
                    .header("Content-Length", body.len())
                    .body(Full::new(Bytes::from(body)))
                    .unwrap())
            }
            (&Method::POST, "/echo") => {
                let body_bytes = req.collect().await?.to_bytes();
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "text/plain")
                    .header("Content-Length", body_bytes.len())
                    .body(Full::new(body_bytes))
                    .unwrap())
            }
            _ => Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::new(Bytes::from("Not Found")))
                .unwrap()),
        }
    }

    let http_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("http");

    if !http_dir.exists() {
        return;
    }

    // Find all .mc.template files (excluding server templates which are tested separately)
    let templates: Vec<_> = fs::read_dir(&http_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e
                .path()
                .file_name()
                .map(|n| n.to_string_lossy().to_string());
            name.as_ref().map_or(false, |n| {
                n.ends_with(".mc.template") && !n.starts_with("http_server")
            })
        })
        .collect();

    // Create a tokio runtime for the HTTP server
    let rt = tokio::runtime::Runtime::new().unwrap();

    for entry in templates {
        let template_path = entry.path();
        let base_name = template_path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .replace(".mc", "");

        // Start hyper HTTP server on a random port
        let (port, shutdown_tx) = rt.block_on(async {
            let addr = SocketAddr::from(([127, 0, 0, 1], 0));
            let listener = TcpListener::bind(addr).await.unwrap();
            let port = listener.local_addr().unwrap().port();

            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            let shutdown_flag = Arc::new(AtomicBool::new(false));
            let shutdown_flag_clone = shutdown_flag.clone();

            tokio::spawn(async move {
                let mut shutdown_rx = shutdown_rx;
                loop {
                    tokio::select! {
                        result = listener.accept() => {
                            if let Ok((stream, _)) = result {
                                let io = TokioIo::new(stream);
                                tokio::spawn(async move {
                                    let _ = http1::Builder::new()
                                        .serve_connection(io, service_fn(handle_request))
                                        .await;
                                });
                            }
                        }
                        _ = &mut shutdown_rx => {
                            shutdown_flag_clone.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                }
            });

            (port, shutdown_tx)
        });

        // Give server time to start
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Read template and replace {{PORT}} with actual port
        let template_content = fs::read_to_string(&template_path)
            .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", template_path, e));
        let moca_content = template_content.replace("{{PORT}}", &port.to_string());

        // Write to a temporary file
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("{}.mc", base_name));
        fs::write(&temp_file, &moca_content)
            .unwrap_or_else(|e| panic!("Failed to write temp file: {}", e));

        // Run the moca file
        let config = RuntimeConfig::default();
        let (actual_stdout, actual_stderr, actual_exitcode, _) =
            run_moca_file_inprocess(&temp_file, &config);

        // Clean up temp file
        let _ = fs::remove_file(&temp_file);

        // Shutdown the server
        let _ = shutdown_tx.send(());

        // Check expected stdout
        let stdout_path = http_dir.join(format!("{}.stdout", base_name));
        if stdout_path.exists() {
            let expected_stdout = fs::read_to_string(&stdout_path)
                .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", stdout_path, e));

            // Remove dynamic 'date:' header line from actual output for comparison
            // Split by \r\n to preserve CRLF line endings
            let actual_stdout_normalized: String = actual_stdout
                .split("\r\n")
                .filter(|line| !line.to_lowercase().starts_with("date:"))
                .collect::<Vec<_>>()
                .join("\r\n");

            assert_eq!(
                actual_stdout_normalized, expected_stdout,
                "stdout mismatch for {:?}\n--- expected ---\n{}\n--- actual ---\n{}",
                template_path, expected_stdout, actual_stdout_normalized
            );
        }

        // Check expected stderr (partial match)
        let stderr_path = http_dir.join(format!("{}.stderr", base_name));
        if stderr_path.exists() {
            let expected_stderr = fs::read_to_string(&stderr_path)
                .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", stderr_path, e));
            assert!(
                actual_stderr.contains(&expected_stderr),
                "stderr mismatch for {:?}\n--- expected (substring) ---\n{}\n--- actual ---\n{}",
                template_path,
                expected_stderr,
                actual_stderr
            );
        }

        // Check exit code (default: 0)
        let exitcode_path = http_dir.join(format!("{}.exitcode", base_name));
        let expected_exitcode = if exitcode_path.exists() {
            fs::read_to_string(&exitcode_path)
                .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", exitcode_path, e))
                .trim()
                .parse::<i32>()
                .unwrap_or(0)
        } else {
            0
        };

        assert_eq!(
            actual_exitcode, expected_exitcode,
            "exit code mismatch for {:?}: expected {}, got {}\nstderr: {}",
            template_path, expected_exitcode, actual_exitcode, actual_stderr
        );
    }
}

// ============================================================================
// Time Syscall Tests
// ============================================================================

/// Format epoch seconds as "YYYY-MM-DD HH:MM:SS" in UTC (same algorithm as VM).
fn format_epoch_secs_utc(epoch_secs: i64) -> String {
    let mut days = epoch_secs / 86400;
    let day_secs = ((epoch_secs % 86400) + 86400) % 86400;
    if epoch_secs < 0 && epoch_secs % 86400 != 0 {
        days -= 1;
    }
    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;
    let second = day_secs % 60;

    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y, m, d, hour, minute, second
    )
}

/// Test time syscalls: verify time accuracy and format correctness.
#[test]
fn snapshot_time_syscall() {
    let test_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("basic")
        .join("syscall_time.mc");

    let config = RuntimeConfig::default();
    let before = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let (stdout, stderr, exitcode, _) = run_moca_file_inprocess(&test_path, &config);
    let after = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

    assert_eq!(
        exitcode, 0,
        "syscall_time.mc failed with exit code {}\nstderr: {}",
        exitcode, stderr
    );

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        4,
        "expected 4 lines of output, got {}: {:?}",
        lines.len(),
        lines
    );

    // Line 0: epoch seconds
    let moca_secs: i64 = lines[0].parse().expect("failed to parse time seconds");
    let before_secs = before.as_secs() as i64;
    let after_secs = after.as_secs() as i64;
    assert!(
        moca_secs >= before_secs - 2 && moca_secs <= after_secs + 2,
        "time() value {} is not within ±2s of Rust time [{}, {}]",
        moca_secs,
        before_secs,
        after_secs
    );

    // Line 1: epoch nanoseconds
    let moca_nanos: i64 = lines[1].parse().expect("failed to parse time_nanos");
    let before_nanos = before.as_nanos() as i64;
    let after_nanos = after.as_nanos() as i64;
    assert!(
        moca_nanos >= before_nanos - 5_000_000_000 && moca_nanos <= after_nanos + 5_000_000_000,
        "time_nanos() value {} is not within ±5s of Rust time [{}, {}]",
        moca_nanos,
        before_nanos,
        after_nanos
    );

    // Line 2: formatted time string — verify against Rust formatting of the same seconds
    let expected_formatted = format_epoch_secs_utc(moca_secs);
    assert_eq!(
        lines[2], expected_formatted,
        "time_format({}) mismatch: moca='{}', rust='{}'",
        moca_secs, lines[2], expected_formatted
    );

    // Line 3: format of epoch 0
    assert_eq!(
        lines[3], "1970-01-01 00:00:00",
        "time_format(0) should be '1970-01-01 00:00:00', got '{}'",
        lines[3]
    );
}
