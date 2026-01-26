use std::fs;
use std::path::Path;
use std::process::Command;

/// Run a .mica file with the mica CLI and return (stdout, stderr, exit_code)
fn run_mica_file(path: &Path, extra_args: &[&str]) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mica"));
    cmd.arg("run");
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.arg(path);

    let output = cmd.output().expect("failed to execute mica");

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

/// Run a single snapshot test
fn run_snapshot_test(mica_path: &Path, dir_name: &str) {
    let extra_args = get_args_for_dir(dir_name);
    let (actual_stdout, actual_stderr, actual_exitcode) = run_mica_file(mica_path, &extra_args);

    let base_path = mica_path.with_extension("");

    // Check stdout (exact match)
    let stdout_path = base_path.with_extension("stdout");
    if stdout_path.exists() {
        let expected_stdout = fs::read_to_string(&stdout_path)
            .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", stdout_path, e));
        assert_eq!(
            actual_stdout, expected_stdout,
            "stdout mismatch for {:?}\n--- expected ---\n{}\n--- actual ---\n{}",
            mica_path, expected_stdout, actual_stdout
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
            mica_path, expected_stderr, actual_stderr
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
        mica_path, expected_exitcode, actual_exitcode
    );
}

/// Discover and run all .mica files in a directory
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
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "mica"))
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
