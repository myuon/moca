// Test Ryū float-to-string via string interpolation

// Basic values
print($"{0.0}");
print($"{-0.0}");
print($"{1.0}");
print($"{-1.0}");

// Simple decimals
print($"{3.14}");
print($"{0.1}");
print($"{0.5}");
print($"{0.001}");
print($"{0.0001}");

// Integers as floats
print($"{10.0}");
print($"{100.0}");
print($"{1000.0}");
print($"{1000000.0}");

// Edge cases in Ryū range
print($"{9999999999999999.0}");
print($"{0.00001}");

// Negative values
print($"{-3.14}");
print($"{-0.001}");
print($"{-100.0}");

// Various magnitudes
print($"{1.5}");
print($"{2.5}");
print($"{12.34}");
print($"{123.456}");
print($"{1234.5678}");

// Edge cases: large kk (kk > 16)
print($"{100000000000000000.0}");
print($"{99999999999999980.0}");
print($"{12345678901234568.0}");
print($"{999999999999999900000.0}");

// Edge cases: small kk (kk <= -5)
print($"{0.0000001}");
print($"{0.0000123}");
print($"{0.000001}");
print($"{0.00000099}");
print($"{0.000000000000001}");

// Scientific notation literals
print($"{1e10}");
print($"{2.5e-3}");
print($"{-1e10}");
print($"{1e0}");
print($"{1.5e2}");
print($"{3.14e5}");

// to_string for floats
print($"{to_string(3.14)}");
print($"{to_string(0.0)}");
print($"{to_string(100000000000000000.0)}");
print($"{to_string(0.0000001)}");
print($"{to_string(-42.5)}");
