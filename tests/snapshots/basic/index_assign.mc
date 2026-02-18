// Test array index assignment
let arr = [1, 2, 3, 4, 5];
print($"{arr[0]}");
print($"{arr[2]}");

arr[0] = 100;
arr[2] = 300;
print($"{arr[0]}");
print($"{arr[2]}");

// Nested array index
let matrix = [[1, 2], [3, 4], [5, 6]];
print($"{matrix[0][0]}");
print($"{matrix[1][1]}");
matrix[1][0] = 99;
print($"{matrix[1][0]}");
