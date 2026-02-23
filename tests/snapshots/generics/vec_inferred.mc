// Test: new Vec<int> {} without type annotation should allow method calls
let v = new Vec<int> {};
v.push(10);
v.push(20);
v.push(30);
print(v[0]);
print(v[1]);
print(v[2]);
