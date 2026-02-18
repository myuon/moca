// Test generic methods on generic structs
struct Container<T> {
    value: T
}

impl<T> Container<T> {
    fun get(self) -> T {
        return self.value;
    }
}

let c1 = Container<int> { value: 10 };
print($"{c1.get()}");

let c2 = Container<string> { value: "hello" };
print($"{c2.get()}");
