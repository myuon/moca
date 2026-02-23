// Map with integer keys test

let m = new Map<int, string> {};

// Test put and get
m[1] = "one";
m[2] = "two";
m[100] = "hundred";

print(m[1]);
print(m[2]);
print(m[100]);
print(m.len());

// Test contains
if m.contains(1) {
    print("has 1");
}
if !m.contains(999) {
    print("no 999");
}

// Test overwrite
m[1] = "ONE";
print(m[1]);
print(m.len());

// Test remove
let removed = m.remove(2);
if removed {
    print("removed 2");
}
print(m.len());

// Test negative key
m[-5] = "negative";
print(m[-5]);
