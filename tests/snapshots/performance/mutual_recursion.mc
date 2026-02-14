// Benchmark: mutual recursion (is_even / is_odd)
fun is_even(n: int) -> int {
    if n == 0 {
        return 1;
    }
    return is_odd(n - 1);
}

fun is_odd(n: int) -> int {
    if n == 0 {
        return 0;
    }
    return is_even(n - 1);
}

let sum = 0;
let i = 0;
while i < 20000 {
    sum = sum + is_even(i % 200);
    i = i + 1;
}
print(sum);
