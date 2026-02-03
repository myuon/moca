// Error: accessing unknown field on generic struct

struct Box<T> {
    value: T
}

let b = Box { value: 42 };
let x = b.unknown;
