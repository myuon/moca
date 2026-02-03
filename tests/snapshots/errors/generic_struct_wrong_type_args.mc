// Error: generic struct with wrong number of type arguments

struct Pair<T, U> {
    first: T,
    second: U
}

// Wrong number of type args (1 instead of 2)
let p: Pair<int> = Pair { first: 1, second: 2 };
