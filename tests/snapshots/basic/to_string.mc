// to_string with various types

// int
print(0.to_string());
print(1.to_string());
print((-1).to_string());
print(42.to_string());
print((-999).to_string());
print(1000000.to_string());

// float
print(0.0.to_string());
print(3.14.to_string());
print((-2.5).to_string());
print(1.0.to_string());

// bool
print(true.to_string());
print(false.to_string());

// nil
print("nil");

// string (identity)
print("hello");
print("");

// used in string interpolation
let x = 42;
let y = 3.14;
let b = true;
print($"int={x}, float={y}, bool={b}");

// edge cases for int
print(10.to_string());
print(100.to_string());
print((-10).to_string());
