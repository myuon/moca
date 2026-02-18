let p = __alloc_heap(3);
__heap_store(p, 0, 10);
__heap_store(p, 1, 20);
__heap_store(p, 2, 30);

// p[i] index access
print(debug(p[0]));
print(debug(p[1]));
print(debug(p[2]));

// p[i] = v index assignment
p[1] = 99;
print(debug(p[1]));

// p.offset(n) - returns ptr pointing to slot n onwards
let q = p.offset(1);
print(debug(q[0]));
print(debug(q[1]));

// offset assignment
q[0] = 42;
print(debug(p[1]));

// chained offset
let r = p.offset(2);
print(debug(r[0]));
