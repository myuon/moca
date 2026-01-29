// Tests that should all pass

fun _test_assert_true() {
    assert(true, "true should be true");
}

fun _test_assert_eq_strings() {
    assert_eq_str("hello", "hello", "strings should match");
}

fun _test_assert_eq_bools() {
    assert_eq_bool(true, true, "bools should match");
    assert_eq_bool(false, false, "bools should match");
}
