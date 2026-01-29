// Moca Standard Library - Prelude
// This file is automatically loaded when running Moca programs.

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
// Parsing Functions
// ============================================================================

// Check if a byte is a whitespace character (space, tab, newline, carriage return)
fun _is_whitespace(c: int) -> bool {
    if c == 32 { return true; }
    if c == 9 { return true; }
    if c == 10 { return true; }
    if c == 13 { return true; }
    return false;
}

// Check if a byte is a digit ('0'-'9')
fun _is_digit(c: int) -> bool {
    if c < 48 { return false; }
    if c > 57 { return false; }
    return true;
}

// Parse a string to an integer.
// Handles leading/trailing whitespace and optional negative sign.
// Throws an error if the string cannot be parsed as an integer.
fun std_parse_int(s: string) -> int {
    let n = len(s);
    var i = 0;

    // Skip leading whitespace
    var continue_ws = true;
    while continue_ws {
        if i >= n {
            continue_ws = false;
        } else {
            if _is_whitespace(s[i]) {
                i = i + 1;
            } else {
                continue_ws = false;
            }
        }
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

    if i >= n {
        throw "cannot parse '" + s + "' as int";
    }
    if !_is_digit(s[i]) {
        throw "cannot parse '" + s + "' as int";
    }

    // Parse digits
    var result = 0;
    var continue_digits = true;
    while continue_digits {
        if i >= n {
            continue_digits = false;
        } else {
            if _is_digit(s[i]) {
                let digit = s[i] - 48;
                result = result * 10 + digit;
                i = i + 1;
            } else {
                continue_digits = false;
            }
        }
    }

    // Skip trailing whitespace
    var continue_ws2 = true;
    while continue_ws2 {
        if i >= n {
            continue_ws2 = false;
        } else {
            if _is_whitespace(s[i]) {
                i = i + 1;
            } else {
                continue_ws2 = false;
            }
        }
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
