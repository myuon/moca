// Test F64ReinterpretAsI64: bitcast float bits to integer
let x = 1.0;
let bits = asm(x) -> i64 {
    __emit("F64ReinterpretAsI64");
};
// IEEE 754: 1.0 = 0x3FF0000000000000 = 4607182418800017408
print(bits);

let y = -1.0;
let bits2 = asm(y) -> i64 {
    __emit("F64ReinterpretAsI64");
};
// IEEE 754: -1.0 = 0xBFF0000000000000 = -4616189618054758400
print(bits2);

let z = 0.0;
let bits3 = asm(z) -> i64 {
    __emit("F64ReinterpretAsI64");
};
// IEEE 754: 0.0 = 0x0000000000000000 = 0
print(bits3);
