// Test Ryū float-to-string implementation
// Verify _float_digit_count and _float_write_to produce the same output as __float_to_string

fun test_float(f: float) {
    let expected = __float_to_string(f);
    let dcount = _float_digit_count(f);
    let buf = __alloc_heap(dcount);
    let end = _float_write_to(buf, 0, f);
    let result = __alloc_string(buf, end);
    if result != expected {
        print("MISMATCH for: " + expected);
        print("  got: " + result);
        print("  dcount=" + to_string(dcount) + " end=" + to_string(end));
    } else {
        print("OK: " + result);
    }
}

// Basic values
test_float(0.0);
test_float(-0.0);
test_float(1.0);
test_float(-1.0);

// Simple decimals
test_float(3.14);
test_float(0.1);
test_float(0.5);
test_float(0.001);
test_float(0.0001);

// Integers as floats
test_float(10.0);
test_float(100.0);
test_float(1000.0);
test_float(1000000.0);

// Edge cases in Ryū range
test_float(9999999999999999.0);
test_float(0.00001);

// Negative values
test_float(-3.14);
test_float(-0.001);
test_float(-100.0);

// Various magnitudes
test_float(1.5);
test_float(2.5);
test_float(12.34);
test_float(123.456);
test_float(1234.5678);
