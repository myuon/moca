// Benchmark: quicksort 1000 random integers
// Uses LCG for deterministic random number generation.

// JIT-compilable: pure integer arithmetic LCG
fun _perf_lcg_next(seed: int) -> int {
    var s = seed * 1103515245 + 12345;
    s = s % 2147483648;
    if s < 0 {
        s = 0 - s;
    }
    return s;
}

fun quicksort_benchmark() {
    // Generate 1000 random integers using LCG
    var v: Vec<int> = Vec<int> { ptr: 0, len: 0, cap: 0 };
    var seed = 42;
    var i = 0;
    while i < 1000 {
        seed = _perf_lcg_next(seed);
        v.push(seed % 10000);
        i = i + 1;
    }

    // Sort using stdlib quicksort
    sort_int(v);

    // Print all sorted elements
    i = 0;
    while i < 1000 {
        print(v[i]);
        i = i + 1;
    }
}

quicksort_benchmark();
