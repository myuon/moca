// Test generic functions with type inference
fun identity<T>(x: T) -> T {
    return x;
}

// Type argument inferred from usage
print($"{identity(100)}");
print($"{identity("world")}");
print($"{identity(false)}");
print($"{identity(2.71)}");
