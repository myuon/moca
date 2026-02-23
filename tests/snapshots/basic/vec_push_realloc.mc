// Test push with reallocation
// Initial capacity is 0, first push allocates 8, then doubles: 8 -> 16 -> 32 -> 64 -> 128
let v = new Vec<int> {};

// Push 100 elements (triggers multiple reallocations)
let i = 0;
while (i < 100) {
    v.push(i);
    i = i + 1;
}

print(v.len());

// Verify first and last elements
print(v[0]);
print(v[99]);

// Verify some middle elements
print(v[50]);
print(v[25]);
