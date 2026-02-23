// Test nested data structures (migrated from object literals)
// Demonstrates accessing nested arrays and maps

// Create a map for point
let point = new Map<string, int> {};
point["x"] = 10;
point["y"] = 20;

// Create an array of numbers
let numbers = [1, 2, 3];
print(numbers[1]);

// Access the nested map
print(point["x"]);
