// Map with integer keys test

let m = map_new_any();

// Test map_put_int and map_get_int
map_put_int(m, 1, "one");
map_put_int(m, 2, "two");
map_put_int(m, 100, "hundred");

print(map_get_int(m, 1));
print(map_get_int(m, 2));
print(map_get_int(m, 100));
print(map_len(m));

// Test map_contains_int
if map_contains_int(m, 1) {
    print("has 1");
}
if !map_contains_int(m, 999) {
    print("no 999");
}

// Test overwrite
map_put_int(m, 1, "ONE");
print(map_get_int(m, 1));
print(map_len(m));

// Test map_remove_int
let removed = map_remove_int(m, 2);
if removed {
    print("removed 2");
}
print(map_len(m));

// Test negative key
map_put_int(m, -5, "negative");
print(map_get_int(m, -5));
