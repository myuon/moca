// Test field access on generic struct

struct Box<T> {
    value: T
}

struct Pair<T, U> {
    first: T,
    second: U
}

let box_int = Box { value: 42 };
print($"{box_int.value}");

let box_str = Box { value: "hello" };
print($"{box_str.value}");

let pair = Pair { first: 1, second: "one" };
print($"{pair.first}");
print($"{pair.second}");
