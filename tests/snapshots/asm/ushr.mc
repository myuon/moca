// Test I64ShrU: unsigned (logical) right shift
// -1 as i64 = all bits set = 0xFFFFFFFFFFFFFFFF
// Logical right shift by 1: 0x7FFFFFFFFFFFFFFF = 9223372036854775807
let a = -1;
let one = 1;
let result = asm(a, one) -> i64 {
    __emit("I64ShrU");
};
print(result);

// Arithmetic right shift by 1 would give -1 (sign extension)
// Logical right shift by 1 gives max positive i64
let b = -2;
let result2 = asm(b, one) -> i64 {
    __emit("I64ShrU");
};
// -2 = 0xFFFFFFFFFFFFFFFE, logical >> 1 = 0x7FFFFFFFFFFFFFFF = 9223372036854775807
print(result2);

// Shift by 63: only sign bit remains
let sixty_three = 63;
let result3 = asm(a, sixty_three) -> i64 {
    __emit("I64ShrU");
};
// -1 >>> 63 = 1
print(result3);
