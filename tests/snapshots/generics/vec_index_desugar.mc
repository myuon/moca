// Test: Vec<T> index access and assignment via desugar
// This tests that vec[i] is desugared to vec.get(i)
// and vec[i] = v is desugared to vec.set(i, v)
// Expected output:
// 1
// 2
// 3
// 99
// 1
// 3
// 100

// Create a Vec<int> using new literal syntax
let v: Vec<int> = new Vec<int> {1, 2, 3};

// Test index access with [] syntax (desugars to .get())
print($"{v[0]}");
print($"{v[1]}");
print($"{v[2]}");

// Test index assignment with [] syntax (desugars to .set())
v[1] = 99;
print($"{v[1]}");

// Verify other elements unchanged
print($"{v[0]}");
print($"{v[2]}");

// Test with computed index
let i = 0;
v[i] = 100;
print($"{v[i]}");
