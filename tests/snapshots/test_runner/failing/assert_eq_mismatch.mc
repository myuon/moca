// Test assert_eq with mismatched values - should fail

fun _test_int_mismatch() {
    assert_eq(1, 2, "values should match");
}

fun _test_str_mismatch() {
    assert_eq_str("hello", "world", "strings should match");
}

fun _test_bool_mismatch() {
    assert_eq_bool(true, false, "bools should match");
}
