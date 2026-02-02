// Test map::`new`() associated function with type inference

// Basic map::`new`() usage with string keys
let m1: map<string, int> = map::`new`();
m1.put("a", 1);
m1.put("b", 2);
m1.put("c", 3);
print(m1.len());
print(m1.get("a"));
print(m1.get("c"));

// map::`new`() with int keys
let m2: map<int, string> = map::`new`();
m2.put(1, "one");
m2.put(2, "two");
print(m2.len());
print(m2.get(1));

// With any types
let m3: map<any, any> = map::`new`();
m3.put("key", 100);
print(m3.get("key"));
