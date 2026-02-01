// Basic Map operations test

// Test with string keys
let m: HashMapAny = map_new();

// Test put_string and get_string
m.put_string("name", "Alice");
m.put_string("city", "Tokyo");

print(m.get_string("name"));
print(m.get_string("city"));
print(m.hm_size);

// Test contains_string
if m.contains_string("name") {
    print("has name");
}
if !m.contains_string("unknown") {
    print("no unknown");
}

// Test overwrite
m.put_string("name", "Bob");
print(m.get_string("name"));
print(m.hm_size);

// Test remove_string
let removed = m.remove_string("city");
if removed {
    print("removed city");
}
print(m.hm_size);

// Test get non-existent key returns 0
print(m.get_string("city"));
