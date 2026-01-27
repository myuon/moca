// Test struct creation and field operations
struct Point {
    x: int,
    y: int
}

let p1 = Point { x: 1, y: 2 };
let p2 = Point { x: 1, y: 2 };
let p3 = Point { x: 3, y: 4 };

print(p1.x);
print(p1.y);
print(p2.x);
print(p3.x);

// Mutation
var mp = Point { x: 0, y: 0 };
mp.x = 10;
mp.y = 20;
print(mp.x);
print(mp.y);
