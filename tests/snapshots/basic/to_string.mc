// to_string with various types

// int
print(to_string(0));
print(to_string(1));
print(to_string(-1));
print(to_string(42));
print(to_string(-999));
print(to_string(1000000));

// float
print(to_string(0.0));
print(to_string(3.14));
print(to_string(-2.5));
print(to_string(1.0));

// bool
print(to_string(true));
print(to_string(false));

// nil
print(to_string(nil));

// string (identity)
print(to_string("hello"));
print(to_string(""));

// used in string interpolation
let x = 42;
let y = 3.14;
let b = true;
print($"int={x}, float={y}, bool={b}");

// edge cases for int
print(to_string(10));
print(to_string(100));
print(to_string(-10));
