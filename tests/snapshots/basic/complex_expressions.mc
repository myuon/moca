// Test complex expression combinations
let a = 1 + 2 * 3 - 4 / 2;
print(a);

// Chained comparisons with && and ||
let b = (1 < 2) && (3 > 2);
print(b);
let c = (1 > 2) || (3 > 2);
print(c);

// Nested ternary-like via function calls
fun choose(cond: bool, t: int, f: int) -> int {
    if cond { return t; }
    return f;
}
print(choose(true, 10, 20));
print(choose(false, 10, 20));

// Compound assignment simulation
var x = 5;
x = x + 3;
x = x * 2;
print(x);
