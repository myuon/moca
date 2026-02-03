// Error: assigning to unknown field on generic struct

struct Box<T> {
    value: T
}

let b = Box { value: 42 };
b.unknown = 10;
