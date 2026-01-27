// Test nullable types with various operations
fun maybe_value() -> int? {
    return 42;
}

fun no_value() -> int? {
    return nil;
}

let a = maybe_value();
print(a);

let b = no_value();
print(b);

// Nullable in let statement
let c: int? = 100;
print(c);

let d: int? = nil;
print(d);

// Nullable array
let arr: array<int>? = [1, 2, 3];
print(arr);
