// Test various recursive patterns
fun sum_to(n: int) -> int {
    if n <= 0 {
        return 0;
    }
    return n + sum_to(n - 1);
}

fun power(base: int, exp: int) -> int {
    if exp == 0 {
        return 1;
    }
    return base * power(base, exp - 1);
}

print(sum_to(5));
print(sum_to(10));
print(power(2, 5));
print(power(3, 4));
