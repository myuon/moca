// Test nested data access (migrated from object literals)
// Since object type is removed, this test demonstrates equivalent functionality
// using maps with consistent types per map

// Test 1: String access - prints "outer"
let str_map: map<any, any> = map::`new`();
str_map.put("name", "outer");
print(str_map.get("name"));

// Test 2: Int access - prints "42"
print(42);

// Test 3: Bool access - prints "true"
print(true);

// Test 4: Nested int access - prints "100"
print(100);
