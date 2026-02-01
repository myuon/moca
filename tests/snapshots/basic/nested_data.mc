// Test nested data structures (migrated from object literals)
// Demonstrates accessing nested arrays and maps

// Create a map for point
let point: HashMapAny = map_new();
point.put_string("x", 10);
point.put_string("y", 20);

// Create an array of numbers
let numbers = [1, 2, 3];
print(numbers[1]);

// Access the nested map
print(point.get_string("x"));
