// Any type with functions

// Function with any parameter and return type
fun identity(x: any) -> any {
    return x;
}

let a = identity(42);
let b = identity("hello");
let c = identity(nil);
let d = identity(true);
print_debug(a);
print_debug(b);
print_debug(c);
print_debug(d);

// Function that takes any and returns specific type
fun convert(x: any) -> string {
    let s: string = "converted";
    return s;
}
let conv_result = convert(123);
print_debug(conv_result);

// Function that takes specific type and returns any
fun wrap(x: int) -> any {
    return x;
}
let wrapped = wrap(999);
print_debug(wrapped);

// Multiple any parameters
fun pair(a: any, b: any) -> any {
    return a;
}
let p1 = pair(1, "two");
let p2 = pair("first", 2);
print_debug(p1);
print_debug(p2);
