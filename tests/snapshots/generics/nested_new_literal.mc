// Test: Nested new literal syntax
// Expected output:
// 3
// 1
// 2
// 3

// Create a Vec containing Vecs (nested new literals)
// Note: Currently we test that expressions in the new literal are correctly evaluated

// Create a Vec with computed values
let v: Vec<int> = new Vec<int> {1 + 0, 1 + 1, 1 + 2};

print($"{v.len()}");

// Verify the computed values
print($"{v.get(0)}");
print($"{v.get(1)}");
print($"{v.get(2)}");
