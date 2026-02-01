// Test push with reallocation
// Initial capacity is 0, first push allocates 8, then doubles: 8 -> 16 -> 32 -> 64 -> 128
var v: vec<any> = vec::new();

// Push 100 elements (triggers multiple reallocations)
var i = 0;
while (i < 100) {
    v.push(i);
    i = i + 1;
}

print(v.len());

// Verify first and last elements
print(v.get(0));
print(v.get(99));

// Verify some middle elements
print(v.get(50));
print(v.get(25));
