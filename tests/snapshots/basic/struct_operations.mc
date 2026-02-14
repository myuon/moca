// Test struct definition, creation, and field access
struct Point {
    x: int,
    y: int
}

struct Person {
    name: string,
    age: int
}

// Create struct instances
let p1 = Point { x: 10, y: 20 };
let p2 = Point { x: 3, y: 4 };

print(p1.x);
print(p1.y);
print(p2.x);
print(p2.y);

// Struct with string field
let alice = Person { name: "Alice", age: 30 };
print(alice.name);
print(alice.age);

// Struct field mutation
let mutable_point = Point { x: 0, y: 0 };
mutable_point.x = 5;
mutable_point.y = 15;
print(mutable_point.x);
print(mutable_point.y);
