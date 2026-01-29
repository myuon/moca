// Test Vector [] syntax for index access and assignment

// Create a vector and push some elements
var vec = vec_new();
vec_push(vec, 10);
vec_push(vec, 20);
vec_push(vec, 30);

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

// Test mixing [] syntax with existing builtins
print(vec_len(vec));
print(vec_get(vec, 0));
