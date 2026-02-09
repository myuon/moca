// Test: concrete generic type parameters support method calls
// Verifies that Vec<int>, Vec<float>, Map<string, int>
// can be used as function parameter types with method calls.

// Vec<int> parameter with method calls
fun vec_int_len(v: Vec<int>) -> int {
    return v.len();
}

fun vec_int_get(v: Vec<int>, i: int) -> int {
    return v[i];
}

fun vec_int_set(v: Vec<int>, i: int, val: int) {
    v[i] = val;
}

let v = new Vec<int> {10, 20, 30};
print(vec_int_len(v));
print(vec_int_get(v, 1));
vec_int_set(v, 1, 99);
print(vec_int_get(v, 1));

// Vec<float> parameter
fun vec_float_len(v: Vec<float>) -> int {
    return v.len();
}

let vf = new Vec<float> {1.5, 2.5};
print(vec_float_len(vf));

// Map parameter with method calls
fun map_len(m: Map<string, int>) -> int {
    return m.len();
}

var m: Map<string, int> = map::`new`();
m["hello"] = 42;
print(map_len(m));

// Named struct parameter (Rand)
fun rand_next(r: Rand) -> int {
    return r.next();
}

var rng: Rand = Rand::`new`(42);
let val = rand_next(rng);
print(val > 0);
