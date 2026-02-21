// Basic asm block with PushInt and ValueToString
asm {
    __emit("PushInt", 42);
    __emit("ValueToString");
};
