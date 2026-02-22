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
