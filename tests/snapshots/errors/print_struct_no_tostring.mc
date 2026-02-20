// Error: struct does not implement ToString
struct Point { x: int, y: int }
let p = Point { x: 1, y: 2 };
print(p);
