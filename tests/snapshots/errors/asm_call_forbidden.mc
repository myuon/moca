// Test Call instruction is forbidden
asm {
    __emit("Call", 0, 0);
};
