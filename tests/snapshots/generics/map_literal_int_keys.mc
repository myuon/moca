// Test: Map<int, string> new literal syntax with int keys
// Expected output:
// 3
// one
// two
// three
// true
// false

// Create a Map<int, string> using new literal syntax
let m: Map<int, string> = new Map<int, string> {1: "one", 2: "two", 3: "three"};

// Check size
print($"{m.len()}");

// Check values
print($"{m.get_int(1)}");
print($"{m.get_int(2)}");
print($"{m.get_int(3)}");

// Check contains
print($"{m.contains_int(1)}");
print($"{m.contains_int(4)}");
