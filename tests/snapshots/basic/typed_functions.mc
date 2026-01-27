// Test functions with type annotations
fun add(a: int, b: int) -> int {
    return a + b;
}

fun greet(name: string) -> string {
    return "Hello, " + name;
}

fun is_positive(n: int) -> bool {
    return n > 0;
}

fun maybe_value(x: int?) -> int {
    if x == nil {
        return 0;
    }
    return x;
}

print(add(3, 4));
print(greet("World"));
print(is_positive(5));
print(is_positive(-3));
print(maybe_value(nil));
print(maybe_value(42));
