// Test basic random number generation with Rand struct

// Test 1: Deterministic output with fixed seed
var rng: Rand = Rand::`new`(42);
print(rng.int(1, 100));
print(rng.int(1, 100));
print(rng.int(1, 100));

// Test 2: rand_float returns values in [0.0, 1.0)
print(rng.float());
print(rng.float());
print(rng.float());

// Test 3: set_seed resets to produce same sequence
rng.set_seed(42);
print(rng.int(1, 100));
print(rng.int(1, 100));
print(rng.int(1, 100));

// Test 4: min == max always returns that value
print(rng.int(5, 5));

// Test 5: Error case - min > max
try {
    rng.int(10, 5);
} catch e {
    print(e);
}

// Test 6: Different seed produces different sequence
var rng2: Rand = Rand::`new`(123);
print(rng2.int(1, 100));
print(rng2.int(1, 100));

// Test 7: Independent instances don't interfere
rng.set_seed(42);
rng2.set_seed(42);
print(rng.int(1, 100));
print(rng2.int(1, 100));
