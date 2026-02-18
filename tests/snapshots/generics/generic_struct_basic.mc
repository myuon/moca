// Test generic struct definitions and instantiation
struct Box<T> {
    value: T
}

// Create boxes with different types
let int_box = Box<int> { value: 42 };
let str_box = Box<string> { value: "hello" };
let bool_box = Box<bool> { value: true };

print($"{int_box.value}");
print($"{str_box.value}");
print($"{bool_box.value}");
