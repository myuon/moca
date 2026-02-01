// Basic Map operations test

// Test with string keys
let m = map_new_any();

// Test map_put_string and map_get_string
map_put_string(m, "name", "Alice");
map_put_string(m, "city", "Tokyo");

print(map_get_string(m, "name"));
print(map_get_string(m, "city"));
print(map_len(m));

// Test map_contains_string
if map_contains_string(m, "name") {
    print("has name");
}
if !map_contains_string(m, "unknown") {
    print("no unknown");
}

// Test overwrite
map_put_string(m, "name", "Bob");
print(map_get_string(m, "name"));
print(map_len(m));

// Test map_remove_string
let removed = map_remove_string(m, "city");
if removed {
    print("removed city");
}
print(map_len(m));

// Test get non-existent key returns 0
print(map_get_string(m, "city"));
