// Test fixed array operations
var arr: array<int> = [1, 2, 3];
print(len(arr));

// Test Vector push/pop operations
var vec: vec<any> = vec::new();
vec.push(1);
vec.push(2);
vec.push(3);
vec.push(4);
print(vec.len());
print(vec.get(3));

let popped = vec.pop();
print(popped);
print(vec.len());

// Nested array access (fixed arrays)
let matrix: array<array<int>> = [[1, 2], [3, 4]];
print(matrix[0][0]);
print(matrix[1][1]);
