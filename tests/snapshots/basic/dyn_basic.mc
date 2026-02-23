// Basic dyn type and match dyn test

// Box values as dyn
let d1 = 42 as dyn;
let d2 = "hello" as dyn;
let d3 = true as dyn;
let d4 = 3.14 as dyn;
let d5 = nil as dyn;

// match dyn to dispatch by type
fun print_dyn(d: dyn) {
    match dyn d {
        v: int => { print(v); }
        v: string => { print(v); }
        v: bool => { print(v); }
        v: float => { print(v); }
        _ => { print("unknown"); }
    }
}

print_dyn(d1);
print_dyn(d2);
print_dyn(d3);
print_dyn(d4);
print_dyn(d5);

// match dyn with computation
fun double_int(d: dyn) {
    match dyn d {
        v: int => { print(v * 2); }
        _ => { print("not int"); }
    }
}

double_int(10);
double_int("hello");

// Struct type as dyn
struct Point { x: int, y: int }

fun describe(d: dyn) {
    match dyn d {
        v: int => { print(v); }
        v: Point => { print(v.x + v.y); }
        _ => { print("other"); }
    }
}

describe(42);
describe(Point { x: 10, y: 20 });
describe("hello");

// Reflection
let dr = Point { x: 3, y: 4 } as dyn;
print(__dyn_type_name(dr));
print(__dyn_field_count(dr));
print(__dyn_field_name(dr, 0));
print(__dyn_field_name(dr, 1));

// Generic struct as dyn — type parameters must be included in tag
struct Container<T> { value: T }

fun match_container(d: dyn) {
    match dyn d {
        v: Container<int> => { print(v.value); }
        v: Container<string> => { print(v.value); }
        _ => { print("other"); }
    }
}

match_container(Container<int> { value: 99 });
match_container(Container<string> { value: "world" });
match_container(42);

// Reflection on generic struct
let dg = Container<int> { value: 7 } as dyn;
print(__dyn_type_name(dg));
