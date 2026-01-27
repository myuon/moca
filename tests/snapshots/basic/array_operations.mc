// Test array operations
var arr: array<int> = [1, 2, 3];
print(len(arr));

push(arr, 4);
print(len(arr));
print(arr[3]);

let popped = pop(arr);
print(popped);
print(len(arr));

// Nested array access
let matrix: array<array<int>> = [[1, 2], [3, 4]];
print(matrix[0][0]);
print(matrix[1][1]);
