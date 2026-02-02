// Test: Map<K, V> type literal syntax
// Expected output:
// 2
// 10
// 20
// true
// true

// Create a Map<string, int> using type literal syntax
let m: Map<string, int> = type Map<string, int> {"a": 10, "b": 20};

// Check size
print(m.len());

// Check values
print(m.get_string("a"));
print(m.get_string("b"));

// Check contains
print(m.contains_string("a"));
print(m.contains_string("b"));
