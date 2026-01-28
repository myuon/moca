// Test float comparison operations

let a = 3.14;
let b = 2.71;
let c = 3.14;

// Less than
print(b < a);   // true
print(a < b);   // false
print(a < c);   // false

// Less than or equal
print(b <= a);  // true
print(a <= c);  // true
print(a <= b);  // false

// Greater than
print(a > b);   // true
print(b > a);   // false
print(a > c);   // false

// Greater than or equal
print(a >= b);  // true
print(a >= c);  // true
print(b >= a);  // false

// Equality
print(a == c);  // true
print(a == b);  // false

// Inequality
print(a != b);  // true
print(a != c);  // false

// Negative values comparison
let neg1 = -1.5;
let neg2 = -2.5;
print(neg1 > neg2);   // true
print(neg2 < neg1);   // true
print(neg1 < 0.0);    // true
print(neg1 > -3.0);   // true
