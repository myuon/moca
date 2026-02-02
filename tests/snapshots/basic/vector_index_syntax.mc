// Test Vector [] syntax for index access and assignment

// Create a vector and push some elements
var vec: vec<any> = vec::`new`();
vec.push(10);
vec.push(20);
vec.push(30);

// Test index access with [] syntax
print(vec[0]);
print(vec[1]);
print(vec[2]);

// Test index assignment with [] syntax
vec[1] = 99;
print(vec[1]);

// Verify other elements unchanged
print(vec[0]);
print(vec[2]);

// Test with computed index
let i = 2;
print(vec[i]);
vec[i] = 200;
print(vec[i]);

// Test mixing [] syntax with methods
print(vec.len());
print(vec.get(0));
