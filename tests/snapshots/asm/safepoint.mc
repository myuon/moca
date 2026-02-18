// Test __safepoint() instruction
let x = 10;
let result = asm(x) -> i64 {
    __emit("PushInt", 1);
    __emit("Add");
    __safepoint();
    __emit("PushInt", 2);
    __emit("Add");
};
print($"{result}");
