// Test fixed array operations
var arr: array<int> = [1, 2, 3];
print(len(arr));

// Test Vector push/pop operations
var vec = vec_new();
vec_push(vec, 1);
vec_push(vec, 2);
vec_push(vec, 3);
vec_push(vec, 4);
print(vec_len(vec));
print(vec[3]);

let popped = vec_pop(vec);
print(popped);
print(vec_len(vec));

// Nested array access (fixed arrays)
let matrix: array<array<int>> = [[1, 2], [3, 4]];
print(matrix[0][0]);
print(matrix[1][1]);
