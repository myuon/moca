// Bitwise AND
print($"{255 & 15}");       // 15

// Bitwise OR
print($"{240 | 15}");       // 255

// Bitwise XOR
print($"{255 ^ 15}");       // 240

// Left shift
print($"{1 << 4}");         // 16

// Arithmetic right shift (sign-preserving)
print($"{-16 >> 2}");       // -4

// Combined expression with precedence
print($"{(5 & 3) | (2 ^ 1)}"); // 3

// Shift by zero
print($"{42 << 0}");        // 42
print($"{42 >> 0}");        // 42

// Chained bitwise
let x: int = 65280;
let y: int = 255;
print($"{x | y}");          // 65535
print($"{x & y}");          // 0
print($"{x ^ y}");          // 65535

// Large shifts
print($"{1 << 32}");        // 4294967296
print(debug(1 << 63));       // -9223372036854775808 (overflow to negative, uses debug to avoid i64::MIN negate overflow)

// Precedence: & binds tighter than |
print($"{255 | 15 & 240}");   // 255  (255 | (15 & 240)) = 255 | 0 = 255

// Precedence: << binds tighter than &
print($"{1 << 4 & 255}");   // 16  ((1 << 4) & 255) = 16 & 255 = 16
