// Test nested data structures (migrated from object literals)
// Demonstrates accessing nested arrays and maps

// Create a map for point
let point = map_new_any();
map_put_string(point, "x", 10);
map_put_string(point, "y", 20);

// Create an array of numbers
let numbers = [1, 2, 3];
print(numbers[1]);

// Access the nested map
print(map_get_string(point, "x"));
