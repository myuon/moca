// Test Vec<T>::`new`() associated function with type inference

// Basic Vec<int>::`new`() usage
let v1 = new Vec<int> {};
v1.push(1);
v1.push(2);
v1.push(3);
print(v1.len());
print(v1[0]);
print(v1[2]);

// With string type
let v2 = new Vec<string> {};
v2.push("hello");
v2.push("world");
print(v2.len());
print(v2[0]);
