// Test dyn-based generic formatter

struct Point { x: int, y: int }

// Basic struct formatting
let p = Point { x: 1, y: 2 } as dyn;
inspect(p);

// Primitive formatting via dyn
inspect(42);
inspect("hello");
inspect(true);
inspect(3.14);
inspect(nil);

// Nested struct formatting
struct Line { a: Point, b: Point }
let l = Line { a: Point { x: 1, y: 2 }, b: Point { x: 3, y: 4 } } as dyn;
inspect(l);

// Struct with string field
struct Named { name: string, age: int }
let n = Named { name: "alice", age: 30 } as dyn;
inspect(n);
