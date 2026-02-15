// Basic variable interpolation
let name = "Alice";
print($"hello {name}");

// Multiple interpolations
let age = 30;
print($"{name} is {age} years old");

// Expression interpolation
print($"{1 + 2}");

// Auto to_string for int/float/bool
let b = true;
let f = 3.14;
print($"bool={b}, float={f}");

// Escape braces
print($"literal: {{not interpolated}}");

// Adjacent interpolations (no literal between)
let x = 10;
let y = 20;
print($"{x}{y}");

// String variable (no extra to_string)
let greeting = "hi";
print($"{greeting} world");

// Interpolation with single variable
print($"{name}");

// Interpolation with arithmetic
print($"{x + 1} is eleven");

// Mixed escapes and interpolation
print($"a {{b}} {name} {{c}}");

// Function call in interpolation
let arr = [1, 2, 3];
print($"length={len(arr)}");

// Normal string (no interpolation)
print("just a plain string");

// Escape sequences inside interpolation strings
print($"line1\nline2");
print($"tab\there");
