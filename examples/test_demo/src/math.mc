// Math functions and their tests

fun add(a: int, b: int) -> int {
    return a + b;
}

fun sub(a: int, b: int) -> int {
    return a - b;
}

// Test functions
fun _test_add() {
    assert_eq(add(1, 2), 3, "1 + 2 should be 3");
    assert_eq(add(0, 0), 0, "0 + 0 should be 0");
    assert_eq(add(-1, 1), 0, "-1 + 1 should be 0");
}

fun _test_sub() {
    assert_eq(sub(5, 3), 2, "5 - 3 should be 2");
    assert_eq(sub(0, 0), 0, "0 - 0 should be 0");
}

fun _test_fail_example() {
    assert_eq(1, 2, "this should fail");
}
