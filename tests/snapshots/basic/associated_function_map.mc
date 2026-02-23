// Test Map<K,V>::`new`() associated function with type inference

// Basic Map<string, int>::`new`() usage with string keys
let m1 = new Map<string, int> {};
m1["a"] = 1;
m1["b"] = 2;
m1["c"] = 3;
print(m1.len());
print(m1["a"]);
print(m1["c"]);

// Map<int, string>::`new`() with int keys
let m2 = new Map<int, string> {};
m2[1] = "one";
m2[2] = "two";
print(m2.len());
print(m2[1]);

// With string keys and int values
let m3 = new Map<string, int> {};
m3["key"] = 100;
print(m3["key"]);
