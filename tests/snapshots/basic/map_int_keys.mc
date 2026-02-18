// Map with integer keys test

let m: map<int, string> = map::`new`();

// Test put and get
m.put(1, "one");
m.put(2, "two");
m.put(100, "hundred");

print($"{m.get(1)}");
print($"{m.get(2)}");
print($"{m.get(100)}");
print($"{m.len()}");

// Test contains
if m.contains(1) {
    print("has 1");
}
if !m.contains(999) {
    print("no 999");
}

// Test overwrite
m.put(1, "ONE");
print($"{m.get(1)}");
print($"{m.len()}");

// Test remove
let removed = m.remove(2);
if removed {
    print("removed 2");
}
print($"{m.len()}");

// Test negative key
m.put(-5, "negative");
print($"{m.get(-5)}");
