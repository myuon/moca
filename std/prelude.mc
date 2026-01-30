// Moca Standard Library - Prelude
// This file is automatically loaded when running Moca programs.

// ============================================================================
// I/O Functions (using syscall_write)
// ============================================================================

// Print a string to stdout without a newline.
fun print_str(s: string) {
    let n = len(s);
    syscall_write(1, s, n);
}

// Print a string to stderr without a newline.
fun eprint_str(s: string) {
    let n = len(s);
    syscall_write(2, s, n);
}

// ============================================================================
// Testing / Assertion Functions
// ============================================================================

// Assert that a condition is true. If false, throws an error with the given message.
fun assert(condition: bool, msg: string) {
    if !condition {
        throw msg;
    }
}

// Assert that two values are equal. If not equal, throws an error with the given message.
// Uses to_string for comparison, so works with any type that can be converted to string.
fun assert_eq(actual: int, expected: int, msg: string) {
    if actual != expected {
        throw msg + " (expected: " + to_string(expected) + ", actual: " + to_string(actual) + ")";
    }
}

// Assert that two strings are equal.
fun assert_eq_str(actual: string, expected: string, msg: string) {
    if actual != expected {
        throw msg + " (expected: " + expected + ", actual: " + actual + ")";
    }
}

// Assert that two booleans are equal.
fun assert_eq_bool(actual: bool, expected: bool, msg: string) {
    if actual != expected {
        throw msg + " (expected: " + to_string(expected) + ", actual: " + to_string(actual) + ")";
    }
}

// ============================================================================
// Math Functions
// ============================================================================

fun abs(x: int) -> int {
    if x < 0 {
        return -x;
    }
    return x;
}

fun max(a: int, b: int) -> int {
    if a > b {
        return a;
    }
    return b;
}

fun min(a: int, b: int) -> int {
    if a < b {
        return a;
    }
    return b;
}

// ============================================================================
// String Functions
// ============================================================================

fun str_len(s: string) -> int {
    return len(s);
}

fun str_contains(haystack: string, needle: string) -> bool {
    let haystack_len = len(haystack);
    let needle_len = len(needle);

    if needle_len == 0 {
        return true;
    }
    if needle_len > haystack_len {
        return false;
    }

    var i = 0;
    while i <= haystack_len - needle_len {
        var j = 0;
        var found = true;
        while j < needle_len {
            if haystack[i + j] != needle[j] {
                found = false;
                j = needle_len;
            } else {
                j = j + 1;
            }
        }
        if found {
            return true;
        }
        i = i + 1;
    }
    return false;
}

// ============================================================================
// Vector Functions (low-level implementation using heap intrinsics)
// ============================================================================

// VectorAny struct - compatible with vector internal layout.
// This allows treating vectors as structs for more natural field access.
// Layout: [field_count=3, ptr, len, cap]
struct VectorAny {
    ptr: int,
    len: int,
    cap: int
}

// Internal implementation of vec_push. The vec_push builtin calls this function.
// Vector layout: [field_count=3, ptr, len, cap] (struct-compatible)
fun vec_push_any(v, value) {
    var data_ptr = __heap_load(v, 1);
    var current_len = __heap_load(v, 2);
    var current_cap = __heap_load(v, 3);

    if current_len >= current_cap {
        // Need to grow
        var new_cap = current_cap * 2;
        if new_cap < 8 {
            new_cap = 8;
        }
        let new_data = __alloc_heap(new_cap);

        // Copy old data if data_ptr is not null
        if data_ptr != nil {
            var i = 0;
            while i < current_len {
                let val = __heap_load(data_ptr, i);
                __heap_store(new_data, i, val);
                i = i + 1;
            }
        }

        // Update vector header
        __heap_store(v, 1, new_data);
        __heap_store(v, 3, new_cap);
        data_ptr = new_data;
    }

    // Store the value at data_ptr[current_len]
    __heap_store(data_ptr, current_len, value);
    // Increment len
    __heap_store(v, 2, current_len + 1);
}

// Internal implementation of vec_pop. The vec_pop builtin calls this function.
// Vector layout: [field_count=3, ptr, len, cap] (struct-compatible)
// Returns the popped value, throws if vector is empty.
fun vec_pop_any(v) {
    let current_len = __heap_load(v, 2);

    if current_len == 0 {
        throw "cannot pop from empty vector";
    }

    let new_len = current_len - 1;
    let data_ptr = __heap_load(v, 1);
    let value = __heap_load(data_ptr, new_len);

    // Update len
    __heap_store(v, 2, new_len);

    return value;
}

// Internal implementation of vec_get. The vec_get builtin calls this function.
// Vector layout: [field_count=3, ptr, len, cap] (struct-compatible)
fun vec_get_any(v, index) {
    let data_ptr = __heap_load(v, 1);
    return __heap_load(data_ptr, index);
}

// Internal implementation of vec_set. The vec_set builtin calls this function.
// Vector layout: [field_count=3, ptr, len, cap] (struct-compatible)
fun vec_set_any(v, index, value) {
    let data_ptr = __heap_load(v, 1);
    __heap_store(data_ptr, index, value);
}

// ============================================================================
// Parsing Functions
// ============================================================================

// Check if a byte is a whitespace character (space, tab, newline, carriage return)
fun _is_whitespace(c: int) -> bool {
    return c == 32 || c == 9 || c == 10 || c == 13;
}

// Check if a byte is a digit ('0'-'9')
fun _is_digit(c: int) -> bool {
    return c >= 48 && c <= 57;
}

// Parse a string to an integer.
// Handles leading/trailing whitespace and optional negative sign.
// Throws an error if the string cannot be parsed as an integer.
fun std_parse_int(s: string) -> int {
    let n = len(s);
    var i = 0;

    // Skip leading whitespace
    while i < n && _is_whitespace(s[i]) {
        i = i + 1;
    }

    if i >= n {
        throw "cannot parse empty string as int";
    }

    // Check for negative sign
    var negative = false;
    if s[i] == 45 {
        negative = true;
        i = i + 1;
    }

    if i >= n || !_is_digit(s[i]) {
        throw "cannot parse '" + s + "' as int";
    }

    // Parse digits
    var result = 0;
    while i < n && _is_digit(s[i]) {
        let digit = s[i] - 48;
        result = result * 10 + digit;
        i = i + 1;
    }

    // Skip trailing whitespace
    while i < n && _is_whitespace(s[i]) {
        i = i + 1;
    }

    // Check for trailing non-whitespace characters
    if i < n {
        throw "cannot parse '" + s + "' as int";
    }

    if negative {
        return -result;
    }
    return result;
}
