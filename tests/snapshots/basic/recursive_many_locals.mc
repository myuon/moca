// Regression test for #273: recursive calls with many local variables
// should not cause stack corruption or "expected reference" errors.
// This test exercises _any_to_string's recursive path through struct formatting.

struct Inner {
    value: int,
}

struct Outer {
    inner: Inner,
}

struct DeepA {
    child: Outer,
}

struct DeepB {
    child: DeepA,
}

let i = Inner { value: 42 };
let o = Outer { inner: i };
let da = DeepA { child: o };
let db = DeepB { child: da };

// Recursive _any_to_string through multiple levels of struct nesting
print(i);
print(o);
print(da);
print(db);
