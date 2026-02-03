// Error: generic struct literal with wrong number of type arguments

struct Pair<T, U> {
    first: T,
    second: U
}

// Wrong number of type args in struct literal (1 instead of 2)
let p = Pair<int> { first: 1, second: 2 };
