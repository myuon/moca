// Test generic functions with explicit type arguments
fun identity<T>(x: T) -> T {
    return x;
}

// Explicit type arguments at call site
print($"{identity<int>(42)}");
print($"{identity<string>("hello")}");
print($"{identity<bool>(true)}");
print($"{identity<float>(3.14)}");
