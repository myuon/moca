// Test generic functions with multiple type parameters
fun first<T, U>(a: T, b: U) -> T {
    return a;
}

fun second<T, U>(a: T, b: U) -> U {
    return b;
}

fun swap<A, B>(a: A, b: B) -> B {
    return b;
}

// Explicit type arguments
print($"{first<int, string>(1, "x")}");
print($"{second<int, string>(2, "y")}");

// Type inference
print($"{first(10, "hello")}");
print($"{second(20, "world")}");
print($"{swap("ignored", 999)}");
