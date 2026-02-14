// Reference capture tests for let variables

// 1. Outer mutation reflected in closure (Spec scenario 1)
let y = 100;
let get_y = fun() -> int { return y; };
y = 200;
print(get_y());

// 2. Closure writes back to outer scope (Spec scenario 2: counter pattern)
let counter = 0;
let inc = fun() -> int {
    counter = counter + 1;
    return counter;
};
print(inc());
print(inc());
print(counter);

// 3. let/let mixed capture (Spec scenario 3)
let a = 10;
let b = 20;
let f = fun() -> int { return a + b; };
b = 30;
print(f());

// 4. let variable stays copy-captured
let c = 50;
let get_c = fun() -> int { return c; };
print(get_c());

// 5. Multiple let captures
let x1 = 1;
let x2 = 2;
let sum = fun() -> int { return x1 + x2; };
x1 = 10;
x2 = 20;
print(sum());

// 6. let not captured by any lambda stays normal (no RefCell overhead)
let normal = 42;
normal = 43;
print(normal);
