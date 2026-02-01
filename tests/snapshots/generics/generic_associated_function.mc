// Test generic associated functions (Type::func() syntax)
struct Wrapper<T> {
    data: T
}

impl<T> Wrapper<T> {
    fun create(value: T) -> Wrapper<T> {
        return Wrapper<T> { data: value };
    }
}

// Create instances using associated functions
let w1 = Wrapper<int>::create(123);
let w2 = Wrapper<string>::create("test");
let w3 = Wrapper<bool>::create(false);

print(w1.data);
print(w2.data);
print(w3.data);
