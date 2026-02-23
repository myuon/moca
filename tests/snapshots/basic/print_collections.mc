// Test debug() for collection types

// array
let a = [1, 2, 3];
print_str(debug(a) + "\n");

// vec
let v = new Vec<int> {};
v.push(10);
v.push(20);
v.push(30);
print_str(debug(v) + "\n");

// empty vec
let ev = new Vec<int> {};
print_str(debug(ev) + "\n");

// map (single entry to avoid ordering issues)
let m1 = new Map<string, int> {};
m1.put_string("key", 42);
print_str(debug(m1) + "\n");

// empty map
let em = new Map<string, int> {};
print_str(debug(em) + "\n");

// nil
print_str(debug(nil) + "\n");

// nested array
let nested = [[1, 2], [3, 4]];
print_str(debug(nested) + "\n");

// vec of strings
let vs = new Vec<string> {};
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
let vp = new Vec<Point> {};
vp.push(Point { x: 1, y: 2 });
vp.push(Point { x: 3, y: 4 });
print_str(debug(vp) + "\n");

// Map<string, struct> (single entry to avoid ordering issues)
let mp = new Map<string, Point> {};
mp.put_string("origin", Point { x: 0, y: 0 });
print_str(debug(mp) + "\n");

// nested Vec<Vec<int>>
let vv = new Vec<Vec<int>> {};
let v1 = new Vec<int> {};
v1.push(1);
v1.push(2);
let v2 = new Vec<int> {};
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
let colors = new Vec<Color> {};
colors.push(Color { r: 255, g: 0, b: 0 });
colors.push(Color { r: 0, g: 255, b: 0 });
print_str(debug(colors) + "\n");

// struct with ToString impl: debug() should use to_string()
struct Pair {
    a: int,
    b: int
}
impl ToString for Pair {
    fun to_string(self) -> string {
        return "(" + _int_to_string(self.a) + ", " + _int_to_string(self.b) + ")";
    }
}
let pair = Pair { a: 10, b: 20 };
print_str(debug(pair) + "\n");

// Vec of structs with ToString
let vp2 = new Vec<Pair> {};
vp2.push(Pair { a: 1, b: 2 });
vp2.push(Pair { a: 3, b: 4 });
print_str(debug(vp2) + "\n");
