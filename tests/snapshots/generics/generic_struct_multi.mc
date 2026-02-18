// Test generic struct with multiple type parameters
struct Pair<T, U> {
    first: T,
    second: U
}

let p1 = Pair<int, string> { first: 1, second: "one" };
let p2 = Pair<string, bool> { first: "flag", second: true };
let p3 = Pair<float, int> { first: 3.14, second: 314 };

print($"{p1.first}");
print($"{p1.second}");
print($"{p2.first}");
print($"{p2.second}");
print($"{p3.first}");
print($"{p3.second}");
