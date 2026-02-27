// Test @inline function with early return
@inline
fun abs(x: int) -> int {
    if x < 0 {
        return -x;
    }
    return x;
}

print(abs(5));
print(abs(-5));
print(abs(0));

// Multiple returns in different branches
@inline
fun clamp(x: int, lo: int, hi: int) -> int {
    if x < lo {
        return lo;
    }
    if x > hi {
        return hi;
    }
    return x;
}

print(clamp(5, 0, 10));
print(clamp(-3, 0, 10));
print(clamp(15, 0, 10));

// Return in while loop
@inline
fun find_first_ge(threshold: int) -> int {
    let i = 0;
    while i < 100 {
        if i >= threshold {
            return i;
        }
        i = i + 1;
    }
    return -1;
}

print(find_first_ge(7));

// Regression test for #257: multiple inline expansions with multiple returns
// caused current_local_types / current_locals_count mismatch, leading to
// wrong type inference (Ref instead of I64) and runtime crash.
@inline
fun digit_count(n: int) -> int {
    if n < 10 { return 1; }
    if n < 100 { return 2; }
    if n < 1000 { return 3; }
    return 4;
}

// Call inline function with multiple returns, then use string interpolation
// (which triggers another inline expansion of string_concat)
let x = 42;
let d = digit_count(x);
print(d);
print($"x={x}");

// Multiple inline calls in same expression
let a = digit_count(5) + digit_count(99) + digit_count(500);
print(a);
