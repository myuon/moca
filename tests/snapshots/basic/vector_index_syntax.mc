// Test Vector [] syntax for index access and assignment

// Create a vector and push some elements
let vec: vec<any> = vec::`new`();
vec.push(10);
vec.push(20);
vec.push(30);

// Test index access with [] syntax
print(debug(vec[0]));
print(debug(vec[1]));
print(debug(vec[2]));

// Test index assignment with [] syntax
vec[1] = 99;
print(debug(vec[1]));

// Verify other elements unchanged
print(debug(vec[0]));
print(debug(vec[2]));

// Test with computed index
let i = 2;
print(debug(vec[i]));
vec[i] = 200;
print(debug(vec[i]));

// Test mixing [] syntax with methods
print(debug(vec.len()));
print(debug(vec.get(0)));
