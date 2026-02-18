// Test nested generic types
struct Option<T> {
    has_value: bool,
    value: T
}

struct Pair<A, B> {
    left: A,
    right: B
}

// Nested generic: Pair with Option
let p = Pair<Option<int>, Option<string>> {
    left: Option<int> { has_value: true, value: 42 },
    right: Option<string> { has_value: true, value: "nested" }
};

print($"{p.left.has_value}");
print($"{p.left.value}");
print($"{p.right.has_value}");
print($"{p.right.value}");
