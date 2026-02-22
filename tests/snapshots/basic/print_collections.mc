// Test debug() for collection types

// array
let a = [1, 2, 3];
print_str(debug(a) + "\n");

// vec
let v: Vec<int> = new Vec<int> {};
v.push(10);
v.push(20);
v.push(30);
print_str(debug(v) + "\n");

// empty vec
let ev: Vec<int> = new Vec<int> {};
print_str(debug(ev) + "\n");

// map (single entry to avoid ordering issues)
let m1: Map<string, int> = new Map<string, int> {};
m1.put_string("key", 42);
print_str(debug(m1) + "\n");

// empty map
let em: Map<string, int> = new Map<string, int> {};
print_str(debug(em) + "\n");

// nil
print_str(debug(nil) + "\n");

// nested array
let nested = [[1, 2], [3, 4]];
print_str(debug(nested) + "\n");

// vec of strings
let vs: Vec<string> = new Vec<string> {};
vs.push("hello");
vs.push("world");
print_str(debug(vs) + "\n");

// struct
struct Point {
    x: int,
    y: int
}
let p = Point { x: 1, y: 2 };
print_str(debug(p) + "\n");

// struct-in-struct
struct Line {
    start: Point,
    end: Point
}
let line = Line { start: Point { x: 0, y: 0 }, end: Point { x: 10, y: 20 } };
print_str(debug(line) + "\n");

// Vec<struct>
let vp: Vec<Point> = new Vec<Point> {};
vp.push(Point { x: 1, y: 2 });
vp.push(Point { x: 3, y: 4 });
print_str(debug(vp) + "\n");

// Map<string, struct> (single entry to avoid ordering issues)
let mp: Map<string, Point> = new Map<string, Point> {};
mp.put_string("origin", Point { x: 0, y: 0 });
print_str(debug(mp) + "\n");

// nested Vec<Vec<int>>
let vv: Vec<Vec<int>> = new Vec<Vec<int>> {};
let v1: Vec<int> = new Vec<int> {};
v1.push(1);
v1.push(2);
let v2: Vec<int> = new Vec<int> {};
v2.push(3);
v2.push(4);
vv.push(v1);
vv.push(v2);
print_str(debug(vv) + "\n");

// struct only debugged through Vec (never individually)
struct Color {
    r: int,
    g: int,
    b: int
}
let colors: Vec<Color> = new Vec<Color> {};
colors.push(Color { r: 255, g: 0, b: 0 });
colors.push(Color { r: 0, g: 255, b: 0 });
print_str(debug(colors) + "\n");
