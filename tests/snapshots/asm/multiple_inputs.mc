// Test multiple input variables
let a = 3;
let b = 4;
let sum = asm(a, b) -> i64 {
    __emit("Add");
};
print($"{sum}");
