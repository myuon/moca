// Test Vec::first method - added to prelude without compiler changes
// This test verifies that new methods in impl blocks are auto-derived

let v: Vec<int> = Vec<int>::`new`();
v.push(42);
v.push(99);
print(v.first());

let vs: Vec<string> = Vec<string>::`new`();
vs.push("hello");
vs.push("world");
print(vs.first());
