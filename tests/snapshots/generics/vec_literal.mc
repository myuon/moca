// Test: Vec<T> new literal syntax
// Expected output:
// 3
// 1
// 2
// 3
// 0

// Create a Vec<int> using new literal syntax
let v = new Vec<int> {1, 2, 3};

// Check length
print(v.len());

// Check elements
print(v[0]);
print(v[1]);
print(v[2]);

// Test empty Vec literal
let empty = new Vec<int> {};
print(empty.len());
