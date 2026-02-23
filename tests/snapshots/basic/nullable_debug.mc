// Test debug() formatting for nullable types with inner type info

struct Point {
    x: int,
    y: int
}

// Primitive nullable
let a: int? = 42;
let b: int? = nil;
print(debug(a));
print(debug(b));

// String nullable
let s: string? = "hello";
let t: string? = nil;
print(debug(s));
print(debug(t));

// Struct nullable — should format as "Point { x: 1, y: 2 }" not "[1, 2]"
let p: Point? = Point { x: 1, y: 2 };
let q: Point? = nil;
print(debug(p));
print(debug(q));

// Bool nullable
let x: bool? = true;
let y: bool? = nil;
print(debug(x));
print(debug(y));
