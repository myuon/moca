// Test nullable type annotations

fun maybe_double(x: int?) -> int? {
    if x == nil {
        return nil;
    }
    return x * 2;
}

let a: int? = 5;
let b: int? = nil;

print(maybe_double(a));
print(maybe_double(b));

// Assigning non-nil to nullable
let c: int? = 10;
print(c);
