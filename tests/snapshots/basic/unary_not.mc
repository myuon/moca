// Test unary not operator
let a = true;
let b = false;

print(!a);
print(!b);
print(!!a);
print(!(!b));

// Combined with comparisons
let x = 5;
print(!(x > 10));
print(!(x < 3));

// In conditions
if !b {
    print("b is false");
}

if !(x == 0) {
    print("x is not zero");
}
