// Basic asm block with PushInt and I64Add
asm {
    __emit("PushInt", 42);
    __emit("PushInt", 1);
    __emit("I64Add");
};
