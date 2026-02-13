// Basic lambda / closure tests

// 1. Simple lambda: assign to variable and call
let add = fun(a: int, b: int) -> int { return a + b; };
print(add(3, 4));

// 2. Lambda capturing a local variable (copy capture)
let x = 10;
let add_x = fun(n: int) -> int { return n + x; };
print(add_x(5));

// 3. Copy capture: mutation of outer variable does NOT affect captured value
var y = 100;
let get_y = fun() -> int { return y; };
y = 200;
print(get_y());

// 4. Higher-order function: pass lambda as argument
fun apply(f: (int) -> int, n: int) -> int {
    return f(n);
}
let double = fun(n: int) -> int { return n * 2; };
print(apply(double, 7));

// 5. Return lambda from function
fun make_adder(n: int) -> (int) -> int {
    return fun(x: int) -> int { return x + n; };
}
let add5 = make_adder(5);
print(add5(10));

// 6. Lambda with no captures
let greet = fun() -> string { return "hello"; };
print(greet());
