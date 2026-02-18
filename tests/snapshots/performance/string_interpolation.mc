// Benchmark: heavy string interpolation
// Builds formatted strings with computed values in a loop.

// JIT-compilable: compute a value from two integers
// Avoids modulo (I64RemS) which is not JIT-supported
fun compute_val(a: int, b: int) -> int {
    let h = a * 31 + b;
    h = h * 17 + (a + b) * 7;
    if h < 0 {
        h = 0 - h;
    }
    return h;
}

fun string_interp_bench() {
    let total = 0;
    let i = 0;
    while i < 100000 {
        let h = compute_val(i, i * 3 + 7);
        let s = i + h;
        let line = $"item[{i}]: hash={h}, sum={s}";
        total = total + len(line);
        i = i + 1;
    }
    print($"{total}");
}

string_interp_bench();
