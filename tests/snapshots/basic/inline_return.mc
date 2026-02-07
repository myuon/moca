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
    var i = 0;
    while i < 100 {
        if i >= threshold {
            return i;
        }
        i = i + 1;
    }
    return -1;
}

print(find_first_ge(7));
