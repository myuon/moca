// Test __umul128_hi: upper 64 bits of unsigned 64x64 multiply
// 2^32 * 2^32 = 2^64, upper 64 bits = 1
let a = 4294967296; // 2^32
let result = __umul128_hi(a, a);
print($"{result}");

// Small values: upper bits are 0
let result2 = __umul128_hi(100, 200);
print($"{result2}");

// Large values: 0xFFFFFFFFFFFFFFFF * 2 = 0x1FFFFFFFFFFFFFFFE
// Upper 64 bits = 1
let max = -1; // 0xFFFFFFFFFFFFFFFF as unsigned
let two = 2;
let result3 = __umul128_hi(max, two);
print($"{result3}");
