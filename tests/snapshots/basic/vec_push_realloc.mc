// Test vec_push with reallocation
// Initial capacity is 0, first push allocates 8, then doubles: 8 -> 16 -> 32 -> 64 -> 128
var v = vec_new();

// Push 100 elements (triggers multiple reallocations)
var i = 0;
while (i < 100) {
    vec_push(v, i);
    i = i + 1;
}

print(vec_len(v));

// Verify first and last elements
print(vec_get(v, 0));
print(vec_get(v, 99));

// Verify some middle elements
print(vec_get(v, 50));
print(vec_get(v, 25));
