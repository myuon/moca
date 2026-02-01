// Map with integer keys test

let m: HashMapAny = map_new();

// Test put_int and get_int
m.put_int(1, "one");
m.put_int(2, "two");
m.put_int(100, "hundred");

print(m.get_int(1));
print(m.get_int(2));
print(m.get_int(100));
print(m.hm_size);

// Test contains_int
if m.contains_int(1) {
    print("has 1");
}
if !m.contains_int(999) {
    print("no 999");
}

// Test overwrite
m.put_int(1, "ONE");
print(m.get_int(1));
print(m.hm_size);

// Test remove_int
let removed = m.remove_int(2);
if removed {
    print("removed 2");
}
print(m.hm_size);

// Test negative key
m.put_int(-5, "negative");
print(m.get_int(-5));
