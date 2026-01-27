// Test nested object literals and access
let obj = {
    name: "outer",
    inner: {
        value: 42,
        flag: true
    }
};

print(obj.name);
print(obj.inner.value);
print(obj.inner.flag);

// Deeply nested
let deep = {
    a: {
        b: {
            c: 100
        }
    }
};
print(deep.a.b.c);
