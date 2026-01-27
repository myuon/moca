// Test function with multiple parameters
fun add_five(a: int, b: int, c: int, d: int, e: int) -> int {
    return a + b + c + d + e;
}

fun concat_three(a: string, b: string, c: string) -> string {
    return a + b + c;
}

print(add_five(1, 2, 3, 4, 5));
print(concat_three("Hello", " ", "World"));
