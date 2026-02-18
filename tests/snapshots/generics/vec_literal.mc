// Test: Vec<T> new literal syntax
// Expected output:
// 3
// 1
// 2
// 3
// 0

// Create a Vec<int> using new literal syntax
let v: Vec<int> = new Vec<int> {1, 2, 3};

// Check length
print($"{v.len()}");

// Check elements
print($"{v.get(0)}");
print($"{v.get(1)}");
print($"{v.get(2)}");

// Test empty Vec literal
let empty: Vec<int> = new Vec<int> {};
print($"{empty.len()}");
