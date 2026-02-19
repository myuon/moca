// Test Map<K,V>::`new`() associated function with type inference

// Basic Map<string, int>::`new`() usage with string keys
let m1: Map<string, int> = Map<string, int>::`new`();
m1.put("a", 1);
m1.put("b", 2);
m1.put("c", 3);
print(m1.len());
print(m1.get("a"));
print(m1.get("c"));

// Map<int, string>::`new`() with int keys
let m2: Map<int, string> = Map<int, string>::`new`();
m2.put(1, "one");
m2.put(2, "two");
print(m2.len());
print(m2.get(1));

// With string keys and int values
let m3: Map<string, int> = Map<string, int>::`new`();
m3.put("key", 100);
print(m3.get("key"));
