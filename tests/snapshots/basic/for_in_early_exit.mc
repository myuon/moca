// Test for-in with early exit via return
fun find_first_even(arr: array<int>) -> int {
    for x in arr {
        if x % 2 == 0 {
            return x;
        }
    }
    return -1;
}

print($"{find_first_even([1, 3, 5, 6, 7])}");
print($"{find_first_even([1, 3, 5, 7])}");
print($"{find_first_even([2, 4, 6])}");
