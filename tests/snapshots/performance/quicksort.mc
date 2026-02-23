// Benchmark: quicksort 100000 random integers
// Uses LCG for deterministic random number generation.

// JIT-compilable: pure integer arithmetic LCG
fun _perf_lcg_next(seed: int) -> int {
    let s = seed * 1103515245 + 12345;
    s = s % 2147483648;
    if s < 0 {
        s = 0 - s;
    }
    return s;
}

fun quicksort_benchmark() {
    // Generate 100000 random integers using LCG
    let v: Vec<int> = Vec<int> { data: __null_ptr(), len: 0, cap: 0 };
    let seed = 42;
    let i = 0;
    while i < 100000 {
        seed = _perf_lcg_next(seed);
        v.push(seed % 100000);
        i = i + 1;
    }

    // Sort using stdlib quicksort
    sort_int(v);

    // Print a few elements to verify sort correctness
    let n = v.len();
    print(v[0]);
    print(v[n / 4]);
    print(v[n / 2]);
    print(v[n * 3 / 4]);
    print(v[n - 1]);
}

quicksort_benchmark();
