// Test functions with multiple return paths
fun max(a: int, b: int) -> int {
    if a > b {
        return a;
    }
    return b;
}

fun abs(x: int) -> int {
    if x < 0 {
        return -x;
    }
    return x;
}

fun sign(x: int) -> int {
    if x < 0 {
        return -1;
    }
    if x > 0 {
        return 1;
    }
    return 0;
}

print($"{max(10, 20)}");
print($"{max(30, 5)}");
print($"{abs(-15)}");
print($"{abs(15)}");
print($"{sign(-5)}");
print($"{sign(5)}");
print($"{sign(0)}");
