// Error: type mismatch when assigning to generic struct field

struct Box<T> {
    value: T
}

let b = Box { value: 42 };
b.value = "wrong type";
