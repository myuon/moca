// Nested lambda tests

// 1. Lambda inside lambda
let compose = fun(f: (int) -> int, g: (int) -> int) -> (int) -> int {
    return fun(x: int) -> int {
        return f(g(x));
    };
};
let double = fun(x: int) -> int { return x * 2; };
let inc = fun(x: int) -> int { return x + 1; };
let double_then_inc = compose(inc, double);
print(double_then_inc(5));

// 2. Lambda returning lambda (currying)
let add = fun(a: int) -> (int) -> int {
    return fun(b: int) -> int {
        return a + b;
    };
};
let add3 = add(3);
print(add3(7));

// 3. Three-level nesting
fun make_counter(start: int) -> () -> int {
    let n = start;
    return fun() -> int {
        return n;
    };
}
let counter = make_counter(42);
print(counter());
