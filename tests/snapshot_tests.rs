use std::fs;
use std::path::Path;
use std::process::Command;

use moca::config::RuntimeConfig;
use moca::compiler;

/// Run a .mc file with the moca CLI and return (stdout, stderr, exit_code)
fn run_moca_file(path: &Path, extra_args: &[&str], working_dir: Option<&Path>) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_moca"));
    cmd.arg("run");
    cmd.arg(path);
    // Extra args come after the file (CLI expects: run FILE [OPTIONS])
    for arg in extra_args {
        cmd.arg(arg);
    }

    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    let output = cmd.output().expect("failed to execute moca");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    (stdout, stderr, exit_code)
}

/// Get extra CLI args based on directory name
fn get_args_for_dir(dir_name: &str) -> Vec<&'static str> {
    match dir_name {
        "jit" => vec!["--jit=on"],
        _ => vec![],
    }
}

/// Run a single snapshot test (file-based or directory-based)
fn run_snapshot_test(test_path: &Path, dir_name: &str) {
    let mut extra_args: Vec<String> = get_args_for_dir(dir_name)
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    // Determine if this is a directory-based test or file-based test
    let (moca_path, working_dir, base_path) = if test_path.is_dir() {
        // Directory-based test: look for main.mc as entry point
        let main_moca = test_path.join("main.mc");
        if !main_moca.exists() {
            panic!(
                "Directory test {:?} must contain main.mc",
                test_path
            );
        }
        // Expected output files are at the directory level (e.g., testdir.stdout)
        (main_moca, Some(test_path), test_path.to_path_buf())
    } else {
        // File-based test
        (test_path.to_path_buf(), None, test_path.with_extension(""))
    };

    // Check for .args file with extra CLI arguments
    let args_path = base_path.with_extension("args");
    if args_path.exists() {
        let args_content = fs::read_to_string(&args_path)
            .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", args_path, e));
        for arg in args_content.lines() {
            let arg = arg.trim();
            if !arg.is_empty() && !arg.starts_with('#') {
                extra_args.push(arg.to_string());
            }
        }
    }

    let extra_args_refs: Vec<&str> = extra_args.iter().map(|s| s.as_str()).collect();
    let (actual_stdout, actual_stderr, actual_exitcode) =
        run_moca_file(&moca_path, &extra_args_refs, working_dir);

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
            moca_path, expected_stderr, actual_stderr
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
        let result = compiler::run_file_with_config(test_path, &config);

        // Check expected stdout if exists (via CLI for stdout capture)
        let stdout_path = base_path.with_extension("stdout");
        if stdout_path.exists() {
            // Use CLI for stdout verification
            let (actual_stdout, _, actual_exitcode) = run_moca_file(test_path, &[], None);
            let expected_stdout = fs::read_to_string(&stdout_path)
                .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", stdout_path, e));
            assert_eq!(
                actual_stdout, expected_stdout,
                "stdout mismatch for {:?} (GC enabled)\n--- expected ---\n{}\n--- actual ---\n{}",
                test_path, expected_stdout, actual_stdout
            );
            assert_eq!(
                actual_exitcode, 0,
                "exit code should be 0 for {:?} (GC enabled)",
                test_path
            );
        } else {
            // Just verify it succeeds
            assert!(
                result.is_ok(),
                "GC enabled test should succeed for {:?}, got error: {:?}",
                test_path,
                result.err()
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

        let result = compiler::run_file_with_config(&gc_disabled_path, &config);

        // Check expected stderr if exists
        let gc_disabled_base = gc_disabled_path.with_extension("");
        let stderr_path = gc_disabled_base.with_extension("stderr");

        if stderr_path.exists() {
            let expected_stderr = fs::read_to_string(&stderr_path)
                .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", stderr_path, e));

            match &result {
                Err(err_msg) => {
                    assert!(
                        err_msg.contains(&expected_stderr),
                        "stderr mismatch for {:?} (GC disabled)\n--- expected (substring) ---\n{}\n--- actual ---\n{}",
                        gc_disabled_path,
                        expected_stderr,
                        err_msg
                    );
                }
                Ok(_) => {
                    panic!(
                        "GC disabled test should fail for {:?}, but it succeeded",
                        gc_disabled_path
                    );
                }
            }
        } else {
            // Just verify it fails with heap limit exceeded
            match result {
                Err(err_msg) => {
                    assert!(
                        err_msg.contains("heap limit exceeded"),
                        "Error should mention heap limit exceeded for {:?}, got: {}",
                        gc_disabled_path,
                        err_msg
                    );
                }
                Ok(_) => {
                    panic!(
                        "GC disabled test should fail for {:?}, but it succeeded",
                        gc_disabled_path
                    );
                }
            }
        }
    }
}
