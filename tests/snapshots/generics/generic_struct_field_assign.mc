// Test field assignment on generic struct

struct Box<T> {
    value: T
}

let box_int = Box { value: 42 };
print(box_int.value);

box_int.value = 100;
print(box_int.value);

let box_str = Box { value: "hello" };
box_str.value = "world";
print(box_str.value);
