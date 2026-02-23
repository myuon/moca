// Hash collision test - keys that hash to the same bucket

// ===== Int key collision test =====
// Keys 0, 16, 32 all hash to bucket 0 (key % 16 == 0)
let m1 = new Map<int, string> {};

m1[0] = "zero";
m1[16] = "sixteen";
m1[32] = "thirty-two";

// All three should be retrievable despite collision
print(m1[0]);
print(m1[16]);
print(m1[32]);
print(m1.len());

// Verify contains works for colliding keys
if m1.contains(0) && m1.contains(16) && m1.contains(32) {
    print("all int keys found");
}

// Update a colliding key
m1[16] = "SIXTEEN";
print(m1[16]);
print(m1.len());

// Remove middle element in chain
m1.remove(16);
print(m1.len());
// Other colliding keys should still work
print(m1[0]);
print(m1[32]);

// ===== String key collision test =====
// Keys "a", "q", "A" all hash to bucket 6
let m2 = new Map<string, int> {};

m2["a"] = 100;
m2["q"] = 200;
m2["A"] = 300;

// All three should be retrievable despite collision
print(m2["a"]);
print(m2["q"]);
print(m2["A"]);
print(m2.len());

// Verify contains works for colliding keys
if m2.contains("a") && m2.contains("q") && m2.contains("A") {
    print("all string keys found");
}

// Remove first element (head of chain)
m2.remove("a");
print(m2.len());
// Other colliding keys should still work
print(m2["q"]);
print(m2["A"]);

// Remove last element (tail of chain)
m2.remove("A");
print(m2.len());
print(m2["q"]);

print("collision test passed");
