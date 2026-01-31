// Hash collision test - keys that hash to the same bucket

// ===== Int key collision test =====
// Keys 0, 16, 32 all hash to bucket 0 (key % 16 == 0)
let m1 = map_new_any();

map_put_int(m1, 0, "zero");
map_put_int(m1, 16, "sixteen");
map_put_int(m1, 32, "thirty-two");

// All three should be retrievable despite collision
print(map_get_int(m1, 0));
print("\n");
print(map_get_int(m1, 16));
print("\n");
print(map_get_int(m1, 32));
print("\n");
print(map_len(m1));
print("\n");

// Verify contains works for colliding keys
if map_contains_int(m1, 0) && map_contains_int(m1, 16) && map_contains_int(m1, 32) {
    print("all int keys found\n");
}

// Update a colliding key
map_put_int(m1, 16, "SIXTEEN");
print(map_get_int(m1, 16));
print("\n");
print(map_len(m1));
print("\n");

// Remove middle element in chain
map_remove_int(m1, 16);
print(map_len(m1));
print("\n");
// Other colliding keys should still work
print(map_get_int(m1, 0));
print("\n");
print(map_get_int(m1, 32));
print("\n");

// ===== String key collision test =====
// Keys "a", "q", "A" all hash to bucket 6
let m2 = map_new_any();

map_put_string(m2, "a", 100);
map_put_string(m2, "q", 200);
map_put_string(m2, "A", 300);

// All three should be retrievable despite collision
print(map_get_string(m2, "a"));
print("\n");
print(map_get_string(m2, "q"));
print("\n");
print(map_get_string(m2, "A"));
print("\n");
print(map_len(m2));
print("\n");

// Verify contains works for colliding keys
if map_contains_string(m2, "a") && map_contains_string(m2, "q") && map_contains_string(m2, "A") {
    print("all string keys found\n");
}

// Remove first element (head of chain)
map_remove_string(m2, "a");
print(map_len(m2));
print("\n");
// Other colliding keys should still work
print(map_get_string(m2, "q"));
print("\n");
print(map_get_string(m2, "A"));
print("\n");

// Remove last element (tail of chain)
map_remove_string(m2, "A");
print(map_len(m2));
print("\n");
print(map_get_string(m2, "q"));
print("\n");

print("collision test passed\n");
