// Test array operations with type annotations
fun sum_array(arr: array<int>) -> int {
    var total = 0;
    for x in arr {
        total = total + x;
    }
    return total;
}

fun first_or_default(arr: array<int>, default: int) -> int {
    if len(arr) == 0 {
        return default;
    }
    return arr[0];
}

let numbers = [1, 2, 3, 4, 5];
print(sum_array(numbers));
print(first_or_default(numbers, -1));
print(first_or_default([], -1));
