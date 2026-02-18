// Test functions with various return types
fun get_int() -> int {
    return 42;
}

fun get_float() -> float {
    return 3.14;
}

fun get_bool() -> bool {
    return true;
}

fun get_string() -> string {
    return "hello";
}

fun get_array() -> array<int> {
    return [1, 2, 3];
}

fun no_return() {
    print("side effect only");
}

print($"{get_int()}");
print($"{get_float()}");
print($"{get_bool()}");
print($"{get_string()}");

let arr = get_array();
print($"{arr[0]}");
print($"{arr[1]}");
print($"{arr[2]}");

no_return();
