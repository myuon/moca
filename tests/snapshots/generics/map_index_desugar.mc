// Test: Map<K,V> index access and assignment via desugar
// This tests that map[key] is desugared to map.get(key)
// and map[key] = value is desugared to map.set(key, value)
// Expected output:
// 10
// 20
// 99
// 10
// 30

// Create a Map<string, int> using new literal syntax
let m: Map<string, int> = new Map<string, int> {"a": 10, "b": 20};

// Test index access with [] syntax (desugars to .get())
print($"{m["a"]}");
print($"{m["b"]}");

// Test index assignment with [] syntax (desugars to .set())
m["b"] = 99;
print($"{m["b"]}");

// Verify other elements unchanged
print($"{m["a"]}");

// Test adding new key
m["c"] = 30;
print($"{m["c"]}");
