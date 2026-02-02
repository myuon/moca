// Test vec::`new`() associated function with type inference

// Basic vec::`new`() usage
let v1: vec<int> = vec::`new`();
v1.push(1);
v1.push(2);
v1.push(3);
print(v1.len());
print(v1.get(0));
print(v1.get(2));

// With string type
let v2: vec<string> = vec::`new`();
v2.push("hello");
v2.push("world");
print(v2.len());
print(v2.get(0));
