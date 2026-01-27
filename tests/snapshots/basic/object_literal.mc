// Test object literal and field access
let person = { name: "Alice", age: 30, active: true };
print(person.name);
print(person.age);
print(person.active);

// Nested object
let nested = { outer: { inner: 42 } };
print(nested.outer.inner);

// Object with computed values
let x = 10;
let y = 20;
let point = { x: x, y: y, sum: x + y };
print(point.sum);
