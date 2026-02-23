// Test: new Vec<int> {} without type annotation should allow method calls
let v = new Vec<int> {};
v.push(10);
v.push(20);
v.push(30);
print(v.get(0));
print(v.get(1));
print(v.get(2));
