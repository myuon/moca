// Hash collision test - keys that hash to the same bucket

// ===== Int key collision test =====
// Keys 0, 16, 32 all hash to bucket 0 (key % 16 == 0)
let m1: HashMapAny = map_new();

m1.put_int(0, "zero");
m1.put_int(16, "sixteen");
m1.put_int(32, "thirty-two");

// All three should be retrievable despite collision
print(m1.get_int(0));
print(m1.get_int(16));
print(m1.get_int(32));
print(m1.hm_size);

// Verify contains works for colliding keys
if m1.contains_int(0) && m1.contains_int(16) && m1.contains_int(32) {
    print("all int keys found");
}

// Update a colliding key
m1.put_int(16, "SIXTEEN");
print(m1.get_int(16));
print(m1.hm_size);

// Remove middle element in chain
m1.remove_int(16);
print(m1.hm_size);
// Other colliding keys should still work
print(m1.get_int(0));
print(m1.get_int(32));

// ===== String key collision test =====
// Keys "a", "q", "A" all hash to bucket 6
let m2: HashMapAny = map_new();

m2.put_string("a", 100);
m2.put_string("q", 200);
m2.put_string("A", 300);

// All three should be retrievable despite collision
print(m2.get_string("a"));
print(m2.get_string("q"));
print(m2.get_string("A"));
print(m2.hm_size);

// Verify contains works for colliding keys
if m2.contains_string("a") && m2.contains_string("q") && m2.contains_string("A") {
    print("all string keys found");
}

// Remove first element (head of chain)
m2.remove_string("a");
print(m2.hm_size);
// Other colliding keys should still work
print(m2.get_string("q"));
print(m2.get_string("A"));

// Remove last element (tail of chain)
m2.remove_string("A");
print(m2.hm_size);
print(m2.get_string("q"));

print("collision test passed");
