// Basic Map operations test

// Test with string keys
let m = new Map<string, string> {};

// Test put and get
m["name"] = "Alice";
m["city"] = "Tokyo";

print(m["name"]);
print(m["city"]);
print(m.len());

// Test contains
if m.contains("name") {
    print("has name");
}
if !m.contains("unknown") {
    print("no unknown");
}

// Test overwrite
m["name"] = "Bob";
print(m["name"]);
print(m.len());

// Test remove
let removed = m.remove("city");
if removed {
    print("removed city");
}
print(m.len());

// Test get non-existent key throws
let got_error = false;
try {
    let _v = m["city"];
} catch e {
    got_error = true;
    print(e);
}
print(got_error);
