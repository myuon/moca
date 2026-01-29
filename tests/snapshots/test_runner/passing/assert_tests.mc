// Test assert functions - all should pass

fun _test_assert_true() {
    assert(true, "true should be true");
}

fun _test_assert_condition() {
    let x = 5;
    assert(x > 0, "x should be positive");
    assert(x < 10, "x should be less than 10");
}

fun _test_assert_eq_ints() {
    assert_eq(42, 42, "42 should equal 42");
    assert_eq(-1, -1, "-1 should equal -1");
}

fun _test_assert_eq_str() {
    assert_eq_str("hello", "hello", "strings should match");
    assert_eq_str("", "", "empty strings should match");
}

fun _test_assert_eq_bool() {
    assert_eq_bool(true, true, "true should equal true");
    assert_eq_bool(false, false, "false should equal false");
}
