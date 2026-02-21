// Basic Map operations test

// Test with string keys
let m: Map<string, string> = Map<string, string>::`new`();

// Test put and get
m.put("name", "Alice");
m.put("city", "Tokyo");

print(m.get("name"));
print(m.get("city"));
print(m.len());

// Test contains
if m.contains("name") {
    print("has name");
}
if !m.contains("unknown") {
    print("no unknown");
}

// Test overwrite
m.put("name", "Bob");
print(m.get("name"));
print(m.len());

// Test remove
let removed = m.remove("city");
if removed {
    print("removed city");
}
print(m.len());

// Test get non-existent key returns nil
print(m.get("city"));
