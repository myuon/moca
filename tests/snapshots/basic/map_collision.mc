// Hash collision test - keys that hash to the same bucket

// ===== Int key collision test =====
// Keys 0, 16, 32 all hash to bucket 0 (key % 16 == 0)
let m1: map<any, any> = map::`new`();

m1.put(0, "zero");
m1.put(16, "sixteen");
m1.put(32, "thirty-two");

// All three should be retrievable despite collision
print(m1.get(0));
print(m1.get(16));
print(m1.get(32));
print(m1.len());

// Verify contains works for colliding keys
if m1.contains(0) && m1.contains(16) && m1.contains(32) {
    print("all int keys found");
}

// Update a colliding key
m1.put(16, "SIXTEEN");
print(m1.get(16));
print(m1.len());

// Remove middle element in chain
m1.remove(16);
print(m1.len());
// Other colliding keys should still work
print(m1.get(0));
print(m1.get(32));

// ===== String key collision test =====
// Keys "a", "q", "A" all hash to bucket 6
let m2: map<any, any> = map::`new`();

m2.put("a", 100);
m2.put("q", 200);
m2.put("A", 300);

// All three should be retrievable despite collision
print(m2.get("a"));
print(m2.get("q"));
print(m2.get("A"));
print(m2.len());

// Verify contains works for colliding keys
if m2.contains("a") && m2.contains("q") && m2.contains("A") {
    print("all string keys found");
}

// Remove first element (head of chain)
m2.remove("a");
print(m2.len());
// Other colliding keys should still work
print(m2.get("q"));
print(m2.get("A"));

// Remove last element (tail of chain)
m2.remove("A");
print(m2.len());
print(m2.get("q"));

print("collision test passed");
