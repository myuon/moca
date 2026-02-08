// Tests for Moca Standard Library (prelude.mc)
// These tests verify that all stdlib functions work correctly.

// ============================================================================
// Math Functions Tests
// ============================================================================

fun _test_abs_positive() {
    assert_eq(abs(5), 5, "abs of positive should be positive");
    assert_eq(abs(100), 100, "abs of 100 should be 100");
}

fun _test_abs_negative() {
    assert_eq(abs(-5), 5, "abs of -5 should be 5");
    assert_eq(abs(-100), 100, "abs of -100 should be 100");
}

fun _test_abs_zero() {
    assert_eq(abs(0), 0, "abs of 0 should be 0");
}

fun _test_max() {
    assert_eq(max(1, 2), 2, "max(1, 2) should be 2");
    assert_eq(max(5, 3), 5, "max(5, 3) should be 5");
    assert_eq(max(-1, -5), -1, "max(-1, -5) should be -1");
    assert_eq(max(10, 10), 10, "max(10, 10) should be 10");
}

fun _test_min() {
    assert_eq(min(1, 2), 1, "min(1, 2) should be 1");
    assert_eq(min(5, 3), 3, "min(5, 3) should be 3");
    assert_eq(min(-1, -5), -5, "min(-1, -5) should be -5");
    assert_eq(min(10, 10), 10, "min(10, 10) should be 10");
}

// ============================================================================
// String Functions Tests
// ============================================================================

fun _test_str_len() {
    assert_eq(str_len(""), 0, "empty string length should be 0");
    assert_eq(str_len("hello"), 5, "length of 'hello' should be 5");
    assert_eq(str_len("a"), 1, "length of 'a' should be 1");
    assert_eq(str_len("hello world"), 11, "length of 'hello world' should be 11");
}

fun _test_str_contains_found() {
    assert_eq_bool(str_contains("hello world", "world"), true, "'hello world' contains 'world'");
    assert_eq_bool(str_contains("hello world", "hello"), true, "'hello world' contains 'hello'");
    assert_eq_bool(str_contains("hello world", "o w"), true, "'hello world' contains 'o w'");
    assert_eq_bool(str_contains("hello", "hello"), true, "'hello' contains 'hello'");
}

fun _test_str_contains_not_found() {
    assert_eq_bool(str_contains("hello world", "xyz"), false, "'hello world' does not contain 'xyz'");
    assert_eq_bool(str_contains("hello", "world"), false, "'hello' does not contain 'world'");
    assert_eq_bool(str_contains("abc", "abcd"), false, "'abc' does not contain 'abcd'");
}

fun _test_str_contains_empty() {
    assert_eq_bool(str_contains("hello", ""), true, "any string contains empty string");
    assert_eq_bool(str_contains("", ""), true, "empty string contains empty string");
    assert_eq_bool(str_contains("", "a"), false, "empty string does not contain 'a'");
}

// ============================================================================
// Parsing Functions Tests
// ============================================================================

fun _test_std_parse_int_positive() {
    assert_eq(std_parse_int("42"), 42, "parse '42' as 42");
    assert_eq(std_parse_int("0"), 0, "parse '0' as 0");
    assert_eq(std_parse_int("12345"), 12345, "parse '12345' as 12345");
}

fun _test_std_parse_int_negative() {
    assert_eq(std_parse_int("-42"), -42, "parse '-42' as -42");
    assert_eq(std_parse_int("-1"), -1, "parse '-1' as -1");
    assert_eq(std_parse_int("-12345"), -12345, "parse '-12345' as -12345");
}

fun _test_std_parse_int_whitespace() {
    assert_eq(std_parse_int("  42"), 42, "parse '  42' with leading whitespace");
    assert_eq(std_parse_int("42  "), 42, "parse '42  ' with trailing whitespace");
    assert_eq(std_parse_int("  42  "), 42, "parse '  42  ' with both whitespace");
    assert_eq(std_parse_int("  -42  "), -42, "parse '  -42  ' with whitespace");
}

// ============================================================================
// Assertion Functions Tests (testing that they work correctly)
// ============================================================================

fun _test_assert_true_condition() {
    assert(1 == 1, "1 equals 1");
    assert(true, "true is true");
    assert(5 > 3, "5 is greater than 3");
}

fun _test_assert_eq_int() {
    assert_eq(1 + 1, 2, "1 + 1 equals 2");
    assert_eq(10 - 5, 5, "10 - 5 equals 5");
    assert_eq(3 * 4, 12, "3 * 4 equals 12");
}

fun _test_assert_eq_str_basic() {
    assert_eq_str("hello", "hello", "identical strings");
    assert_eq_str("", "", "empty strings");
    assert_eq_str("a", "a", "single char strings");
}

fun _test_assert_eq_bool_basic() {
    assert_eq_bool(true, true, "true equals true");
    assert_eq_bool(false, false, "false equals false");
    assert_eq_bool(1 < 2, true, "1 < 2 is true");
    assert_eq_bool(1 > 2, false, "1 > 2 is false");
}

// ============================================================================
// Random Number Generation Tests
// ============================================================================

// Helper: get bucket index (0-9) for a float in [0.0, 1.0)
fun _float_bucket(val: float) -> int {
    var i = 0;
    while i < 10 {
        let threshold = _int_to_float(i + 1) / 10.0;
        if val < threshold {
            return i;
        }
        i = i + 1;
    }
    return 9;
}

// Generate rand_int(1,10) 10000 times and check frequency uniformity (max/min <= 1.2)
fun _test_rand_int_distribution() {
    var rng: Rand = Rand::`new`(42);

    var counts: Vec<int> = new Vec<int> { 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 };

    var i = 0;
    while i < 10000 {
        let val = rng.int(1, 10);
        let idx = val - 1;
        counts.set(idx, counts.get(idx) + 1);
        i = i + 1;
    }

    var max_count = counts.get(0);
    var min_count = counts.get(0);
    i = 1;
    while i < 10 {
        let c = counts.get(i);
        if c > max_count { max_count = c; }
        if c < min_count { min_count = c; }
        i = i + 1;
    }

    // max/min <= 1.2  <=>  max * 5 <= min * 6
    assert(max_count * 5 <= min_count * 6,
        "rand_int distribution: max/min ratio should be within 20% (max=" + to_string(max_count) + ", min=" + to_string(min_count) + ")");
}

// Generate rand_float() 10000 times into 10 buckets and check frequency uniformity (max/min <= 1.2)
fun _test_rand_float_distribution() {
    var rng: Rand = Rand::`new`(42);

    var counts: Vec<int> = new Vec<int> { 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 };

    var i = 0;
    while i < 10000 {
        let val = rng.float();
        let idx = _float_bucket(val);
        counts.set(idx, counts.get(idx) + 1);
        i = i + 1;
    }

    var max_count = counts.get(0);
    var min_count = counts.get(0);
    i = 1;
    while i < 10 {
        let c = counts.get(i);
        if c > max_count { max_count = c; }
        if c < min_count { min_count = c; }
        i = i + 1;
    }

    // max/min <= 1.2  <=>  max * 5 <= min * 6
    assert(max_count * 5 <= min_count * 6,
        "rand_float distribution: max/min ratio should be within 20% (max=" + to_string(max_count) + ", min=" + to_string(min_count) + ")");
}
