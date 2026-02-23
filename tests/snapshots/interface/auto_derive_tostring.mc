// Auto-derive ToString for structs: simple, nested, generic

struct Point {
    x: int,
    y: int
}

struct Line {
    start: Point,
    end: Point
}

struct Wrapper<T> {
    data: T
}

// Simple struct
let p = Point { x: 1, y: 2 };
print(p);
print(p.to_string());

// Nested struct
let l = Line {
    start: Point { x: 0, y: 0 },
    end: Point { x: 3, y: 4 }
};
print(l);

// Generic struct with int
let w = Wrapper<int> { data: 42 };
print(w);

// Generic struct with string
let ws = Wrapper<string> { data: "hello" };
print(ws);
