// Benchmark: print(int) bulk output
// Measures _int_to_string conversion + I/O performance.
// Uses LCG for deterministic number generation (JIT-compilable).

fun _print_lcg_next(seed: int) -> int {
    let s = seed * 1103515245 + 12345;
    s = s % 2147483648;
    if s < 0 {
        s = 0 - s;
    }
    return s;
}

fun print_int_benchmark() {
    let seed = 42;
    let i = 0;
    while i < 100000 {
        seed = _print_lcg_next(seed);
        print(seed % 1000000);
        i = i + 1;
    }
}

print_int_benchmark();
