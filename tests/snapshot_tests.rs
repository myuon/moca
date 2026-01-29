//! Snapshot tests for the moca compiler.
//!
//! All tests run in-process to contribute to coverage measurement.

use std::fs;
use std::path::Path;

use moca::compiler::{dump_ast, dump_bytecode, run_file_capturing_output, run_tests};
use moca::config::{JitMode, RuntimeConfig};

/// Run a .mc file in-process and return (stdout, stderr, exit_code)
fn run_moca_file_inprocess(path: &Path, config: &RuntimeConfig) -> (String, String, i32) {
    let (output, result) = run_file_capturing_output(path, config);

    match result {
        Ok(()) => (output.stdout, output.stderr, 0),
        Err(e) => {
            // Combine captured stderr with error message
            let stderr = if output.stderr.is_empty() {
                e
            } else {
                format!("{}{}", output.stderr, e)
            };
            (output.stdout, stderr, 1)
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
    let (actual_stdout, actual_stderr, actual_exitcode) =
        if let Some(dump_type) = is_dump_test(&base_path) {
            // Dump test (AST or bytecode)
            run_dump_test(&moca_path, dump_type)
        } else {
            // Regular test
            let config = get_config_for_dir(dir_name);
            run_moca_file_inprocess(&moca_path, &config)
        };

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
        let (actual_stdout, actual_stderr, actual_exitcode) =
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

        let (_, actual_stderr, actual_exitcode) =
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
