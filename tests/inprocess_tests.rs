//! In-process integration tests that contribute to coverage.
//!
//! These tests call the compiler/VM APIs directly instead of spawning
//! a separate process, so they are included in coverage measurement.

use std::path::Path;

use moca::compiler::{run_file_with_config, DumpOptions, run_file_with_dump};
use moca::config::RuntimeConfig;

fn run_test_file(name: &str) -> Result<(), String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(name);
    run_file_with_config(&path, &RuntimeConfig::default())
}

fn run_test_file_expect_error(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(name);
    run_file_with_config(&path, &RuntimeConfig::default())
        .expect_err("expected error")
}

// Basic tests
#[test]
fn test_basic_arithmetic() {
    run_test_file("basic/arithmetic.mc").unwrap();
}

#[test]
fn test_basic_boolean_operations() {
    run_test_file("basic/boolean_operations.mc").unwrap();
}

#[test]
fn test_basic_comparison() {
    run_test_file("basic/comparison.mc").unwrap();
}

#[test]
fn test_basic_control_flow() {
    run_test_file("basic/control_flow.mc").unwrap();
}

#[test]
fn test_basic_factorial() {
    run_test_file("basic/factorial.mc").unwrap();
}

#[test]
fn test_basic_fibonacci() {
    run_test_file("basic/fibonacci.mc").unwrap();
}

#[test]
fn test_basic_fizzbuzz() {
    run_test_file("basic/fizzbuzz.mc").unwrap();
}

#[test]
fn test_basic_object_literal() {
    run_test_file("basic/object_literal.mc").unwrap();
}

#[test]
fn test_basic_struct_operations() {
    run_test_file("basic/struct_operations.mc").unwrap();
}

#[test]
fn test_basic_typed_functions() {
    run_test_file("basic/typed_functions.mc").unwrap();
}

#[test]
fn test_basic_array_typed() {
    run_test_file("basic/array_typed.mc").unwrap();
}

#[test]
fn test_basic_try_catch_throw() {
    run_test_file("basic/try_catch_throw.mc").unwrap();
}

#[test]
fn test_basic_unary_not() {
    run_test_file("basic/unary_not.mc").unwrap();
}

#[test]
fn test_basic_while_loops() {
    run_test_file("basic/while_loops.mc").unwrap();
}

#[test]
fn test_basic_index_assign() {
    run_test_file("basic/index_assign.mc").unwrap();
}

#[test]
fn test_basic_function_return_types() {
    run_test_file("basic/function_return_types.mc").unwrap();
}

#[test]
fn test_basic_if_else_chains() {
    run_test_file("basic/if_else_chains.mc").unwrap();
}

#[test]
fn test_basic_modulo_operator() {
    run_test_file("basic/modulo_operator.mc").unwrap();
}

// Error tests - these should fail compilation/type checking
#[test]
fn test_error_division_by_zero() {
    // This one actually runs and throws at runtime
    let result = run_test_file("errors/division_by_zero.mc");
    assert!(result.is_err());
}

#[test]
fn test_error_undefined_variable() {
    let err = run_test_file_expect_error("errors/undefined_variable.mc");
    assert!(err.contains("undefined"));
}

#[test]
fn test_error_undefined_function() {
    let err = run_test_file_expect_error("errors/undefined_function.mc");
    assert!(err.contains("undefined"));
}

#[test]
fn test_error_immutable_assignment() {
    let err = run_test_file_expect_error("errors/immutable_assignment.mc");
    assert!(err.contains("immutable") || err.contains("cannot assign"));
}

#[test]
fn test_error_invalid_and_operator() {
    let err = run_test_file_expect_error("errors/invalid_and_operator.mc");
    assert!(err.contains("&&"));
}

#[test]
fn test_error_invalid_or_operator() {
    let err = run_test_file_expect_error("errors/invalid_or_operator.mc");
    assert!(err.contains("||"));
}

#[test]
fn test_error_invalid_escape_sequence() {
    let err = run_test_file_expect_error("errors/invalid_escape_sequence.mc");
    assert!(err.contains("escape"));
}

#[test]
fn test_error_unexpected_character() {
    let err = run_test_file_expect_error("errors/unexpected_character.mc");
    assert!(err.contains("unexpected"));
}

#[test]
fn test_error_type_mismatch() {
    let err = run_test_file_expect_error("errors/type_mismatch.mc");
    assert!(err.contains("expected") || err.contains("found"));
}
