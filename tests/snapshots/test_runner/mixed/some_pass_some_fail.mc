// Mixed test case - some pass, some fail

fun _test_should_pass_1() {
    assert(true, "this should pass");
}

fun _test_should_pass_2() {
    assert_eq(1, 1, "1 equals 1");
}

fun _test_should_fail() {
    assert(false, "this should fail");
}
