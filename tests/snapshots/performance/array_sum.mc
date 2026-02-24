// Benchmark: array sequential access (repeated sum scan)
// Tests memory bandwidth - directly affected by per-element size.
// With tag removal (16B → 8B per element), cache utilization improves.

// JIT-compilable: pure integer arithmetic LCG
fun _array_lcg_next(seed: int) -> int {
    let s = seed * 1103515245 + 12345;
    s = s % 2147483648;
    if s < 0 {
        s = 0 - s;
    }
    return s;
}

fun array_sum_benchmark() {
    // Build a 500K-element array
    let v: Vec<int> = Vec<int> { data: __null_ptr(), len: 0, cap: 0 };
    let seed = 42;
    let i = 0;
    let n = 500000;
    while i < n {
        seed = _array_lcg_next(seed);
        v.push(seed % 1000000);
        i = i + 1;
    }

    // 200 sequential scans — exercises HeapLoad2 heavily
    let total = 0;
    let scan = 0;
    while scan < 200 {
        i = 0;
        while i < n {
            total = total + v[i];
            i = i + 1;
        }
        scan = scan + 1;
    }
    print(total);
}

array_sum_benchmark();
