// Test math operations - all should pass

fun add(a: int, b: int) -> int {
    return a + b;
}

fun _test_add_positive() {
    assert_eq(add(1, 2), 3, "1 + 2 should be 3");
}

fun _test_add_zero() {
    assert_eq(add(0, 0), 0, "0 + 0 should be 0");
}

fun _test_add_negative() {
    assert_eq(add(-1, -2), -3, "-1 + -2 should be -3");
}
