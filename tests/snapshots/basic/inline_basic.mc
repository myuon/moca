// Test basic @inline function
@inline
fun add(x: int, y: int) -> int {
    return x + y;
}

@inline
fun mul(x: int, y: int) -> int {
    return x * y;
}

// Single call
print($"{add(1, 2)}");

// Multiple calls
print($"{add(10, 20)}");
print($"{mul(3, 4)}");

// Nested expression with inline functions
print($"{add(mul(2, 3), mul(4, 5))}");

// Inline function with no args
@inline
fun get_answer() -> int {
    return 42;
}
print($"{get_answer()}");

// Inline function with one arg
@inline
fun double(x: int) -> int {
    return x * 2;
}
print($"{double(21)}");
