// Test input variable and output
let x = 10;
let result = asm(x) -> i64 {
    __emit("PushInt", 5);
    __emit("Add");
};
print($"{result}");
