// Test type mismatch error
fun add(a: int, b: int) -> int {
    return a + b;
}

let result = add("hello", 5);
print($"{result}");
