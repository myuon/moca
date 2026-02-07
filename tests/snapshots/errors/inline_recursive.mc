// Test that @inline recursive function produces a compile error
@inline
fun fact(n: int) -> int {
    if n <= 1 {
        return 1;
    }
    return n * fact(n - 1);
}

print(fact(5));
