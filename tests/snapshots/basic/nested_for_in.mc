// Test nested for-in loops
let matrix: array<array<int>> = [[1, 2], [3, 4], [5, 6]];
let sum = 0;
for row in matrix {
    for val in row {
        sum = sum + val;
        print(val);
    }
}
print(sum);
