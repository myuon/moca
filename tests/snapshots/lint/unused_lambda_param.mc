// Unused lambda parameter — should warn
let f = fun(x: int) -> int { return 42; };
print(f(1));

// Used lambda parameter — no warning
let g = fun(y: int) -> int { return y + 1; };
print(g(1));

// _ prefix suppresses warning
let h = fun(_z: int) -> int { return 0; };
print(h(1));

// Multiple params, some unused
let m = fun(a: int, b: int) -> int { return a; };
print(m(1, 2));

// All params unused
let n = fun(p: int, q: int) -> int { return 99; };
print(n(1, 2));
