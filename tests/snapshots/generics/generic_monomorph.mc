// Test that monomorphisation creates correct specialized versions
// The same generic function with different types should produce different code

fun double<T>(x: T) -> T {
    return x;
}

// These should create separate specialized functions
let a = double<int>(5);
let b = double<string>("five");
let c = double<bool>(true);
let d = double<float>(5.5);

// Use them multiple times to ensure monomorphisation works
let a2 = double<int>(10);
let b2 = double<string>("ten");

print($"{a}");
print($"{b}");
print($"{c}");
print($"{d}");
print($"{a2}");
print($"{b2}");
